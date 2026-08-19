//! `.kndo/cache/` — the facts layer of the project cache (ADR 0004, RFC 0004 §2–3).
//!
//! This is the foundation the rest of RFC 0004 (graph snapshot, findings snapshot, dirty-region
//! incrementality, diff-mode derived effects) builds on: skip-reparsing-unchanged-files is
//! already most of the warm-path win, since parsing dominates cold-run cost (spike 0001). The
//! graph/findings snapshots and the patch algorithm (RFC 0004 §4–6) are not implemented yet —
//! every run still re-assembles the graph from (cached-or-fresh) `FileFacts`.
//!
//! Layout, keying, and format decisions here mirror ADR 0004 exactly:
//! - Content-addressed by `(adapter id, adapter facts-schema version, file content hash)` —
//!   renames, branch switches, and `git stash` all hit the cache; a file reverted to an old
//!   version re-hits its old entry.
//! - `bincode` for these small per-file entries — no zero-copy win at this size (`rkyv` is for
//!   `graph.bin`/`findings.bin`, which don't exist yet).
//! - Every artifact carries a magic + format-version header; any mismatch — including a kndo
//!   upgrade that changed the on-disk shape — silently rebuilds that entry rather than erroring
//!   or migrating in place. The cache is explicitly disposable.
//! - Single-writer advisory lock; a concurrent run degrades to read-only cache use instead of
//!   racing writes.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::adapter::FileFacts;

/// Facts-entry envelope header: bumped whenever the serialized shape changes, independent of
/// any adapter's own `facts_schema_version` (which already keys the entry's path) — this is
/// the belt to that suspenders, guarding against a kndo binary upgrade whose `FileFacts` type
/// changed shape while an adapter's declared version didn't move.
const ENTRY_FORMAT_VERSION: u32 = 1;
const FACTS_MAGIC: [u8; 4] = *b"KNF1";
const HEADER_LEN: usize = FACTS_MAGIC.len() + 4;

/// ADR 0004's default facts-store cap; `prune` enforces it, LRU-by-mtime.
pub const DEFAULT_CAP_BYTES: u64 = 256 * 1024 * 1024;

struct LockFile(PathBuf);

impl Drop for LockFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// One project's on-disk facts cache handle — `.kndo/cache/facts/` under the project root.
/// Read/write methods take `&self` and touch only per-entry files named by content hash, so
/// concurrent calls from rayon workers on distinct files never race (the only shared mutable
/// state is the hit counter, which is atomic).
pub struct FactsCache {
    facts_dir: PathBuf,
    /// Kept for the future graph/findings snapshot paths (RFC 0004 §2) and for tests; not read
    /// by the facts layer itself, which only ever needs `facts_dir`.
    #[allow(dead_code)]
    cache_dir: PathBuf,
    /// `false` when another process already holds the write lock, or the cache directory
    /// couldn't be created (read-only filesystem, permissions…) — reads still work in either
    /// case, writes silently no-op. A disposable cache degrading instead of failing the run is
    /// the point (ADR 0004): analysis correctness never depends on the cache being writable.
    writable: bool,
    _lock: Option<LockFile>,
    hits: AtomicU64,
}

fn hex32(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

impl FactsCache {
    /// Opens (creating if needed) the cache under `root`. Never fails the caller — an
    /// unwritable or uncreatable cache directory just yields a read-mostly-empty, write-nothing
    /// handle rather than aborting analysis (RFC 0001 §6's "diagnostics degrade, never vanish"
    /// spirit, applied to a subsystem that's allowed to not exist at all).
    pub fn open(root: &Path) -> FactsCache {
        let kndo_dir = root.join(".kndo");
        let cache_dir = kndo_dir.join("cache");
        let facts_dir = cache_dir.join("facts");
        if fs::create_dir_all(&facts_dir).is_err() {
            return FactsCache {
                facts_dir,
                cache_dir,
                writable: false,
                _lock: None,
                hits: AtomicU64::new(0),
            };
        }
        // Makes the cache disposable regardless of the project's own root `.gitignore` — a
        // `kndo init` hook installer is a separate M2 deliverable (the pre-commit hook), but a
        // cache that could get committed by accident is a correctness bug on its own, not
        // something worth waiting on that command to prevent.
        let _ = fs::write(kndo_dir.join(".gitignore"), "cache/\n");
        let lock_path = cache_dir.join("lock");
        let lock = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .ok()
            .map(|_| LockFile(lock_path));
        let writable = lock.is_some();
        FactsCache {
            facts_dir,
            cache_dir,
            writable,
            _lock: lock,
            hits: AtomicU64::new(0),
        }
    }

    fn entry_path(
        &self,
        adapter_id: &str,
        facts_schema_version: u32,
        content_hash: &[u8; 32],
    ) -> PathBuf {
        self.facts_dir.join(adapter_id).join(format!(
            "{facts_schema_version}-{}.bin",
            hex32(content_hash)
        ))
    }

    /// Number of `get` calls this handle served from disk — the Engine's only signal for
    /// whether a run was actually warm (`run.cache` in the JSON envelope), since a cache can be
    /// open-but-empty on a project's first run.
    pub fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    /// Fetch cached facts for this exact `(adapter, schema version, content)` triple. A
    /// missing, unreadable, or version-mismatched entry is a plain miss — never an error the
    /// caller has to handle; the entry is simply rebuilt from source (ADR 0004).
    pub fn get(
        &self,
        adapter_id: &str,
        facts_schema_version: u32,
        content_hash: &[u8; 32],
    ) -> Option<FileFacts> {
        let path = self.entry_path(adapter_id, facts_schema_version, content_hash);
        let bytes = fs::read(&path).ok()?;
        let facts = decode(&bytes)?;
        self.hits.fetch_add(1, Ordering::Relaxed);
        Some(facts)
    }

    /// Store facts for this triple. No-op (silently) when the cache opened read-only, when
    /// encoding fails (never expected for `FileFacts`, but a serialization bug should degrade
    /// to "just don't cache it" rather than crash a check run), or on a write race — a
    /// write-then-rename keeps a concurrent reader from ever observing a partial file.
    pub fn put(
        &self,
        adapter_id: &str,
        facts_schema_version: u32,
        content_hash: &[u8; 32],
        facts: &FileFacts,
    ) {
        if !self.writable {
            return;
        }
        let path = self.entry_path(adapter_id, facts_schema_version, content_hash);
        let Some(parent) = path.parent() else { return };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        let Some(bytes) = encode(facts) else { return };
        let tmp = path.with_extension("bin.tmp");
        if fs::write(&tmp, &bytes).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }

    /// Best-effort LRU-by-mtime prune down to `cap_bytes` (ADR 0004 default: [`DEFAULT_CAP_BYTES`]).
    /// Called once per run after writes land — never on the hot get/put path — and only when
    /// this handle holds the write lock; a read-only handle has nothing it's entitled to delete.
    pub fn prune(&self, cap_bytes: u64) {
        if !self.writable {
            return;
        }
        let mut entries: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
        let mut total: u64 = 0;
        let mut stack = vec![self.facts_dir.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(read_dir) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in read_dir.flatten() {
                let path = entry.path();
                let Ok(meta) = entry.metadata() else { continue };
                if meta.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "bin") {
                    let size = meta.len();
                    let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                    total += size;
                    entries.push((path, size, mtime));
                }
            }
        }
        if total <= cap_bytes {
            return;
        }
        entries.sort_by_key(|(_, _, mtime)| *mtime); // oldest first
        for (path, size, _) in entries {
            if total <= cap_bytes {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(size);
            }
        }
    }

    /// Every `facts/<adapter>/` prefix this handle currently sees on disk — used only to size
    /// prune-related tests; not part of the hot path.
    #[cfg(test)]
    fn known_adapter_dirs(&self) -> std::collections::HashSet<String> {
        fs::read_dir(&self.facts_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .collect()
    }

    #[cfg(test)]
    fn cache_root(&self) -> &Path {
        &self.cache_dir
    }
}

fn encode(facts: &FileFacts) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(&FACTS_MAGIC);
    out.extend_from_slice(&ENTRY_FORMAT_VERSION.to_le_bytes());
    bincode::serialize_into(&mut out, facts).ok()?;
    Some(out)
}

fn decode(bytes: &[u8]) -> Option<FileFacts> {
    if bytes.len() < HEADER_LEN || bytes[..FACTS_MAGIC.len()] != FACTS_MAGIC {
        return None;
    }
    let version = u32::from_le_bytes(bytes[FACTS_MAGIC.len()..HEADER_LEN].try_into().ok()?);
    if version != ENTRY_FORMAT_VERSION {
        return None;
    }
    bincode::deserialize(&bytes[HEADER_LEN..]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::Declaration;
    use crate::vocab::SymbolKind;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kndo-cache-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_facts() -> FileFacts {
        FileFacts {
            declarations: vec![Declaration {
                name: "f".into(),
                kind: SymbolKind::Function,
                span: Default::default(),
                exported: true,
                visibility: crate::adapter::VisibilityLevel(1),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn miss_on_empty_cache_then_hit_after_put() {
        let dir = tmp("hit-miss");
        let cache = FactsCache::open(&dir);
        let hash = [1u8; 32];
        assert!(cache.get("js-ts", 1, &hash).is_none());
        assert_eq!(cache.hits(), 0);

        cache.put("js-ts", 1, &hash, &sample_facts());
        let round_tripped = cache.get("js-ts", 1, &hash).expect("should hit after put");
        assert_eq!(round_tripped.declarations.len(), 1);
        assert_eq!(round_tripped.declarations[0].name, "f");
        assert_eq!(cache.hits(), 1);
    }

    #[test]
    fn distinct_hashes_and_adapters_never_collide() {
        let dir = tmp("distinct-keys");
        let cache = FactsCache::open(&dir);
        let a = [1u8; 32];
        let b = [2u8; 32];
        cache.put("js-ts", 1, &a, &sample_facts());
        assert!(cache.get("js-ts", 1, &b).is_none());
        assert!(cache.get("go", 1, &a).is_none()); // different adapter, same hash
        assert!(cache.get("js-ts", 2, &a).is_none()); // different schema version, same hash
    }

    #[test]
    fn a_second_writer_degrades_to_read_only() {
        let dir = tmp("second-writer");
        let first = FactsCache::open(&dir);
        assert!(first.writable);
        let second = FactsCache::open(&dir);
        assert!(!second.writable);

        let hash = [7u8; 32];
        second.put("js-ts", 1, &hash, &sample_facts());
        assert!(second.get("js-ts", 1, &hash).is_none()); // put was a silent no-op

        first.put("js-ts", 1, &hash, &sample_facts());
        // The read-only handle still reads what the writer produced (shared filesystem state).
        assert!(second.get("js-ts", 1, &hash).is_some());
    }

    #[test]
    fn dropping_the_writer_releases_the_lock_for_the_next_open() {
        let dir = tmp("lock-release");
        {
            let first = FactsCache::open(&dir);
            assert!(first.writable);
        } // dropped — lock file removed
        let second = FactsCache::open(&dir);
        assert!(second.writable);
    }

    #[test]
    fn corrupt_entry_is_a_silent_miss_not_an_error() {
        let dir = tmp("corrupt");
        let cache = FactsCache::open(&dir);
        let hash = [3u8; 32];
        cache.put("js-ts", 1, &hash, &sample_facts());
        let path = cache.entry_path("js-ts", 1, &hash);
        fs::write(&path, b"not a valid cache entry at all").unwrap();
        assert!(cache.get("js-ts", 1, &hash).is_none());
    }

    #[test]
    fn stale_format_version_is_a_silent_miss() {
        let dir = tmp("stale-version");
        let cache = FactsCache::open(&dir);
        let hash = [4u8; 32];
        let mut bytes = encode(&sample_facts()).unwrap();
        // Corrupt just the format-version field to simulate a future kndo build's layout.
        bytes[FACTS_MAGIC.len()..HEADER_LEN].copy_from_slice(&999u32.to_le_bytes());
        let path = cache.entry_path("js-ts", 1, &hash);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(cache.get("js-ts", 1, &hash).is_none());
    }

    #[test]
    fn gitignore_makes_the_cache_disposable_regardless_of_the_project_gitignore() {
        let dir = tmp("gitignore");
        let _cache = FactsCache::open(&dir);
        let contents = fs::read_to_string(dir.join(".kndo/.gitignore")).unwrap();
        assert!(contents.contains("cache/"));
    }

    #[test]
    fn prune_evicts_oldest_entries_first_down_to_the_cap() {
        let dir = tmp("prune");
        let cache = FactsCache::open(&dir);
        // Distinct content per entry so sizes differ enough to matter, and put() calls stagger
        // mtimes in insertion order (filesystem mtime resolution is coarse but monotonic here).
        for i in 0..5u8 {
            let hash = [i; 32];
            let mut facts = sample_facts();
            facts.declarations[0].name = format!("f{i}").repeat(50).into();
            cache.put("js-ts", 1, &hash, &facts);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(cache.known_adapter_dirs().len(), 1);

        let sizes: Vec<u64> = fs::read_dir(cache.cache_root().join("facts/js-ts"))
            .unwrap()
            .flatten()
            .map(|e| e.metadata().unwrap().len())
            .collect();
        let total_before: u64 = sizes.iter().sum();
        assert!(total_before > 0);

        // Entries are equal-sized (same shape, same-length names) — a cap just over two
        // entries' worth leaves room for exactly the two newest, evicting the rest.
        let per_entry = total_before / sizes.len() as u64;
        cache.prune(per_entry * 2 + 1);
        let remaining: Vec<_> = fs::read_dir(cache.cache_root().join("facts/js-ts"))
            .unwrap()
            .flatten()
            .collect();
        assert!(remaining.len() < 5, "prune should have evicted something");
        // The very first entries written (oldest mtime) must be gone; the last must survive.
        assert!(cache.get("js-ts", 1, &[0u8; 32]).is_none());
        assert!(cache.get("js-ts", 1, &[1u8; 32]).is_none());
        assert!(cache.get("js-ts", 1, &[4u8; 32]).is_some());
    }

    #[test]
    fn prune_is_a_noop_under_the_cap() {
        let dir = tmp("prune-noop");
        let cache = FactsCache::open(&dir);
        let hash = [9u8; 32];
        cache.put("js-ts", 1, &hash, &sample_facts());
        cache.prune(DEFAULT_CAP_BYTES);
        assert!(cache.get("js-ts", 1, &hash).is_some());
    }
}
