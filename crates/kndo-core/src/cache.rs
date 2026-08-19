//! `.kndo/cache/` — the project cache (ADR 0004, RFC 0004 §2–3): a facts layer (this file's
//! original scope) plus a graph snapshot (`graph.bin`) that lets a warm run skip assembly
//! entirely, not just re-parsing. The patch algorithm and dirty-region incrementality (RFC 0004
//! §4–6) — reusing *part* of a stale graph — aren't implemented yet: today it's all-or-nothing,
//! either every input matches the last snapshot exactly, or the graph is rebuilt from scratch
//! (cache-warm parsing still applies during that rebuild). The findings snapshot (needed for
//! diff-mode derived effects, RFC 0004 §6) doesn't exist yet either.
//!
//! Layout, keying, and format decisions mirror ADR 0004 exactly:
//! - Facts are content-addressed by `(adapter id, adapter facts-schema version, file content
//!   hash)` — renames, branch switches, and `git stash` all hit the cache; a file reverted to
//!   an old version re-hits its old entry. `bincode` — no zero-copy win at this per-file size.
//! - The graph snapshot is keyed by a single digest folding in the *whole* discovered file set
//!   (every path + content hash — RFC 0004 §3's "set of (path, content hash)" already subsumes
//!   "manifest hashes": a manifest is just one more discovered file) plus each registered
//!   adapter's id and facts-schema version plus [`crate::graph::GRAPH_SCHEMA_VERSION`]. Two
//!   inputs RFC 0004 §3 also lists — a kndo config hash and the active plugin set — don't exist
//!   as subsystems yet, so they're honestly absent from the key rather than faked; extending it
//!   is required before either subsystem ships. Any key mismatch is a full rebuild, never a
//!   partial patch. `rkyv` + `mmap`, per ADR 0004 exactly — `get_graph` maps `graph.bin` and
//!   validates directly against the mapped bytes; nothing is read into a heap buffer first, so
//!   loading really is "mmap + validate," not a copy dressed up as one.
//! - Every artifact — facts entry and graph snapshot alike — carries a magic + format-version
//!   header; any mismatch, including a kndo upgrade that changed the on-disk shape, silently
//!   rebuilds that layer rather than erroring or migrating in place. The cache is explicitly
//!   disposable.
//! - Single-writer advisory lock; a concurrent run degrades to read-only cache use instead of
//!   racing writes.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::adapter::{Diagnostic, FileFacts};
use crate::graph::{
    DeclaredDependency, DependencyNode, FileNode, PackageNode, ProjectGraph, SymbolNode,
};
use crate::vocab::{Edge, PackageId};
use smol_str::SmolStr;

/// Facts-entry envelope header: bumped whenever the serialized shape changes, independent of
/// any adapter's own `facts_schema_version` (which already keys the entry's path) — this is
/// the belt to that suspenders, guarding against a kndo binary upgrade whose `FileFacts` type
/// changed shape while an adapter's declared version didn't move.
const ENTRY_FORMAT_VERSION: u32 = 1;
const FACTS_MAGIC: [u8; 4] = *b"KNF1";
const HEADER_LEN: usize = FACTS_MAGIC.len() + 4;

/// ADR 0004's default facts-store cap; `prune` enforces it, LRU-by-mtime.
pub const DEFAULT_CAP_BYTES: u64 = 256 * 1024 * 1024;

/// Graph-snapshot envelope header: magic + format version (belt to the content-key's
/// suspenders, same role as [`ENTRY_FORMAT_VERSION`]) + the 32-byte key itself, kept in this
/// plain, unarchived prefix so a key mismatch — the common case any time a file changed — is a
/// handful of byte comparisons, never a full `rkyv` validation of a payload about to be thrown
/// away.
const GRAPH_MAGIC: [u8; 4] = *b"KNG1";
const GRAPH_FORMAT_VERSION: u32 = 1;
const GRAPH_KEY_LEN: usize = 32;
const GRAPH_HEADER_LEN: usize = GRAPH_MAGIC.len() + 4 + GRAPH_KEY_LEN;

/// `script_invoked_dependencies` is a `HashSet<(PackageId, SmolStr)>` on the live graph — rkyv
/// can't derive `Archive` for a bare tuple with a `#[rkyv(with = ..)]`-annotated element (the
/// attribute only attaches to a named struct/enum field), and a `HashSet` needs converting to a
/// sequence either way, so this tiny struct is both the `with`-attachment point and the
/// sequence element.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct ScriptInvokedDepSnap {
    package: PackageId,
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    name: SmolStr,
}

/// The archived payload (everything after the header) — deliberately a standalone type rather
/// than deriving `Archive` on [`ProjectGraph`] itself: `ProjectGraph::file_index` is a derived
/// index (rebuilt on load, RFC 0004 §2 — no reason to pay to persist it), and `diagnostics`
/// lives outside `ProjectGraph` entirely on the live path (`assemble`'s second return value) but
/// belongs in the snapshot so a full-hit warm run doesn't silently drop them (RFC 0001 §6:
/// diagnostics degrade, never vanish — including across a "nothing changed" cache hit).
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct GraphSnapshot {
    files: Vec<FileNode>,
    symbols: Vec<SymbolNode>,
    dependencies: Vec<DependencyNode>,
    declared_dependencies: Vec<DeclaredDependency>,
    script_invoked_dependencies: Vec<ScriptInvokedDepSnap>,
    packages: Vec<PackageNode>,
    edges: Vec<Edge>,
    diagnostics: Vec<Diagnostic>,
}

struct LockFile(PathBuf);

impl Drop for LockFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// One project's on-disk cache handle — `.kndo/cache/` under the project root: `facts/` (one
/// entry per file) plus `graph.bin` (one snapshot for the whole project). Facts read/write
/// methods take `&self` and touch only per-entry files named by content hash, so concurrent
/// calls from rayon workers on distinct files never race; the graph snapshot is written once,
/// after assembly, never mid-assembly (the only shared mutable state during assembly is the
/// facts hit counter, which is atomic).
pub struct ProjectCache {
    facts_dir: PathBuf,
    cache_dir: PathBuf,
    /// `false` when another process already holds the write lock, or the cache directory
    /// couldn't be created (read-only filesystem, permissions…) — reads still work in either
    /// case, writes silently no-op. A disposable cache degrading instead of failing the run is
    /// the point (ADR 0004): analysis correctness never depends on the cache being writable.
    writable: bool,
    _lock: Option<LockFile>,
    hits: AtomicU64,
    /// Separate from `hits`: a graph-snapshot hit skips the facts layer entirely (nothing to
    /// look up per file when the whole graph is already known-current), so it needs its own
    /// signal — `Engine`'s warm/cold reporting checks both (`hits() + graph_hits() > 0`).
    graph_hits: AtomicU64,
}

fn hex32(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

impl ProjectCache {
    /// Opens (creating if needed) the cache under `root`. Never fails the caller — an
    /// unwritable or uncreatable cache directory just yields a read-mostly-empty, write-nothing
    /// handle rather than aborting analysis (RFC 0001 §6's "diagnostics degrade, never vanish"
    /// spirit, applied to a subsystem that's allowed to not exist at all).
    pub fn open(root: &Path) -> ProjectCache {
        let kndo_dir = root.join(".kndo");
        let cache_dir = kndo_dir.join("cache");
        let facts_dir = cache_dir.join("facts");
        if fs::create_dir_all(&facts_dir).is_err() {
            return ProjectCache {
                facts_dir,
                cache_dir,
                writable: false,
                _lock: None,
                hits: AtomicU64::new(0),
                graph_hits: AtomicU64::new(0),
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
        ProjectCache {
            facts_dir,
            cache_dir,
            writable,
            _lock: lock,
            hits: AtomicU64::new(0),
            graph_hits: AtomicU64::new(0),
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

    /// Number of `get_graph` calls served from the snapshot. Separate from [`Self::hits`]: a
    /// graph-snapshot hit skips the facts layer entirely, so `Engine`'s warm/cold reporting
    /// checks `hits() + graph_hits() > 0`, not `hits()` alone.
    pub fn graph_hits(&self) -> u64 {
        self.graph_hits.load(Ordering::Relaxed)
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

    fn graph_path(&self) -> PathBuf {
        self.cache_dir.join("graph.bin")
    }

    /// Load the graph snapshot iff its stored key exactly matches `key` — the caller (`graph.rs`)
    /// computes `key` from the current discovered file set + adapter versions +
    /// [`crate::graph::GRAPH_SCHEMA_VERSION`]; any other value means *something* in that input
    /// changed since the snapshot was written, so this is a plain miss, not an error, exactly
    /// like a facts-entry miss (ADR 0004: any mismatch ⇒ silently rebuild).
    pub fn get_graph(&self, key: &[u8; GRAPH_KEY_LEN]) -> Option<(ProjectGraph, Vec<Diagnostic>)> {
        let file = fs::File::open(self.graph_path()).ok()?;
        let len = file.metadata().ok()?.len();
        if len < GRAPH_HEADER_LEN as u64 {
            return None; // no snapshot yet, or a partial one — either way, nothing to map
        }

        // SAFETY: `graph.bin` is only ever replaced by `put_graph`'s write-to-tmp-then-rename,
        // which is atomic on every platform kndo targets — a concurrent writer's rename can
        // only swap this mapping onto a *complete*, previously-finished file; it can never
        // truncate or mutate the bytes of the inode currently mapped. `ProjectCache` also holds
        // a single-writer advisory lock for the whole cache (ADR 0004 §7), so no other kndo
        // process is writing this file at the same time in the first place. The one hazard
        // `Mmap::map` genuinely can't rule out — some other, non-kndo process truncating or
        // overwriting the file in place while it's mapped — is the same hazard any mmap-based
        // cache accepts; `rkyv::access` below still validates every byte before trusting any of
        // them, so even that failure mode surfaces as a clean miss, never memory-unsafe.
        let mmap = unsafe { memmap2::Mmap::map(&file) }.ok()?;

        if mmap[..GRAPH_MAGIC.len()] != GRAPH_MAGIC {
            return None;
        }
        let version_start = GRAPH_MAGIC.len();
        let key_start = version_start + 4;
        let version = u32::from_le_bytes(mmap[version_start..key_start].try_into().ok()?);
        if version != GRAPH_FORMAT_VERSION {
            return None;
        }
        if mmap[key_start..GRAPH_HEADER_LEN] != *key {
            return None;
        }

        // Genuinely zero-copy: `payload` is a slice straight into the OS page cache via `mmap`,
        // never a heap buffer kndo allocated and filled in first. `GRAPH_HEADER_LEN` (40 bytes)
        // is a multiple of every alignment this archive's plain-data fields need, and `mmap`
        // hands back a page-aligned base (verified empirically: rkyv's `access` rejects a
        // misaligned slice outright rather than silently miscompiling, so this isn't a
        // "probably fine" assumption), so the offset payload stays aligned too — `access`
        // validates in place with no copy at all.
        let archived =
            rkyv::access::<ArchivedGraphSnapshot, rkyv::rancor::Error>(&mmap[GRAPH_HEADER_LEN..])
                .ok()?;
        let snapshot: GraphSnapshot =
            rkyv::deserialize::<GraphSnapshot, rkyv::rancor::Error>(archived).ok()?;

        let script_invoked_dependencies = snapshot
            .script_invoked_dependencies
            .into_iter()
            .map(|d| (d.package, d.name))
            .collect();
        let graph = ProjectGraph::from_snapshot_parts(
            snapshot.files,
            snapshot.symbols,
            snapshot.dependencies,
            snapshot.declared_dependencies,
            script_invoked_dependencies,
            snapshot.packages,
            snapshot.edges,
        );
        self.graph_hits.fetch_add(1, Ordering::Relaxed);
        Some((graph, snapshot.diagnostics))
    }

    /// Persist `graph`/`diagnostics` under `key`. No-op when the cache opened read-only or
    /// encoding fails, same silent-degrade contract as [`Self::put`].
    pub fn put_graph(
        &self,
        key: &[u8; GRAPH_KEY_LEN],
        graph: &ProjectGraph,
        diagnostics: &[Diagnostic],
    ) {
        if !self.writable {
            return;
        }
        let snapshot = GraphSnapshot {
            files: graph.files.clone(),
            symbols: graph.symbols.clone(),
            dependencies: graph.dependencies.clone(),
            declared_dependencies: graph.declared_dependencies.clone(),
            script_invoked_dependencies: graph
                .script_invoked_dependencies
                .iter()
                .cloned()
                .map(|(package, name)| ScriptInvokedDepSnap { package, name })
                .collect(),
            packages: graph.packages.clone(),
            edges: graph.edges.clone(),
            diagnostics: diagnostics.to_vec(),
        };
        let Ok(bytes) = rkyv::to_bytes::<rkyv::rancor::Error>(&snapshot) else {
            return;
        };
        let mut out = Vec::with_capacity(GRAPH_HEADER_LEN + bytes.len());
        out.extend_from_slice(&GRAPH_MAGIC);
        out.extend_from_slice(&GRAPH_FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(key);
        out.extend_from_slice(&bytes);

        let path = self.graph_path();
        let tmp = path.with_extension("bin.tmp");
        if fs::write(&tmp, &out).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
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
        let cache = ProjectCache::open(&dir);
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
        let cache = ProjectCache::open(&dir);
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
        let first = ProjectCache::open(&dir);
        assert!(first.writable);
        let second = ProjectCache::open(&dir);
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
            let first = ProjectCache::open(&dir);
            assert!(first.writable);
        } // dropped — lock file removed
        let second = ProjectCache::open(&dir);
        assert!(second.writable);
    }

    #[test]
    fn corrupt_entry_is_a_silent_miss_not_an_error() {
        let dir = tmp("corrupt");
        let cache = ProjectCache::open(&dir);
        let hash = [3u8; 32];
        cache.put("js-ts", 1, &hash, &sample_facts());
        let path = cache.entry_path("js-ts", 1, &hash);
        fs::write(&path, b"not a valid cache entry at all").unwrap();
        assert!(cache.get("js-ts", 1, &hash).is_none());
    }

    #[test]
    fn stale_format_version_is_a_silent_miss() {
        let dir = tmp("stale-version");
        let cache = ProjectCache::open(&dir);
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
        let _cache = ProjectCache::open(&dir);
        let contents = fs::read_to_string(dir.join(".kndo/.gitignore")).unwrap();
        assert!(contents.contains("cache/"));
    }

    #[test]
    fn prune_evicts_oldest_entries_first_down_to_the_cap() {
        let dir = tmp("prune");
        let cache = ProjectCache::open(&dir);
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
        let cache = ProjectCache::open(&dir);
        let hash = [9u8; 32];
        cache.put("js-ts", 1, &hash, &sample_facts());
        cache.prune(DEFAULT_CAP_BYTES);
        assert!(cache.get("js-ts", 1, &hash).is_some());
    }

    fn sample_graph() -> ProjectGraph {
        use crate::adapter::ProjectPath;
        use crate::vocab::{Confidence, Edge, EdgeKind, FileId, Provenance, SymbolId};

        ProjectGraph::for_test(
            vec![FileNode {
                path: ProjectPath("a.mock".into()),
                content_hash: [1u8; 32],
                language: Some("mock".into()),
                class: None,
                package: PackageId(0),
            }],
            vec![SymbolNode {
                file: FileId(0),
                name: "x".into(),
                kind: crate::vocab::SymbolKind::Function,
                span: Default::default(),
                exported: true,
                visibility: crate::adapter::VisibilityLevel(0),
            }],
            vec![DependencyNode {
                name: "lodash".into(),
            }],
            vec![Edge {
                kind: EdgeKind::Declares {
                    file: FileId(0),
                    symbol: SymbolId(0),
                },
                confidence: Confidence::Certain,
                source: Provenance::Adapter("mock".into()),
            }],
        )
        .with_script_invoked_dependencies(vec![(PackageId(0), "xo".into())])
        .with_declared_dependencies(vec![DeclaredDependency {
            package: PackageId(0),
            manifest: ProjectPath("package.json".into()),
            name: "lodash".into(),
            version_req: "^4".into(),
            scope: crate::vocab::DependencyScope::Prod,
        }])
    }

    #[test]
    fn graph_round_trips_including_diagnostics_and_misses_on_key_mismatch() {
        let dir = tmp("graph-roundtrip");
        let cache = ProjectCache::open(&dir);
        let key = [5u8; GRAPH_KEY_LEN];
        let graph = sample_graph();
        let diagnostics = vec![Diagnostic {
            level: crate::adapter::DiagnosticLevel::Warn,
            path: Some(crate::adapter::ProjectPath("a.mock".into())),
            message: "example diagnostic".to_string(),
            span: None,
        }];

        assert!(cache.get_graph(&key).is_none());
        assert_eq!(cache.graph_hits(), 0);

        cache.put_graph(&key, &graph, &diagnostics);

        let (restored, restored_diagnostics) = cache.get_graph(&key).expect("should hit");
        assert_eq!(cache.graph_hits(), 1);
        assert_eq!(restored.files.len(), 1);
        assert_eq!(restored.files[0].path.0, "a.mock");
        assert_eq!(restored.symbols.len(), 1);
        assert_eq!(restored.symbols[0].name, "x");
        assert_eq!(restored.dependencies.len(), 1);
        assert_eq!(restored.declared_dependencies.len(), 1);
        assert_eq!(restored.edges.len(), 1);
        assert_eq!(
            restored.script_invoked_dependencies,
            [(PackageId(0), SmolStr::new("xo"))]
                .into_iter()
                .collect::<std::collections::HashSet<_>>()
        );
        assert_eq!(
            restored.file_id(&crate::adapter::ProjectPath("a.mock".into())),
            Some(crate::vocab::FileId(0))
        );
        assert_eq!(restored_diagnostics.len(), 1);
        assert_eq!(restored_diagnostics[0].message, "example diagnostic");

        // A different key (any input change) is a plain miss, not a stale hit.
        let other_key = [6u8; GRAPH_KEY_LEN];
        assert!(cache.get_graph(&other_key).is_none());
        assert_eq!(cache.graph_hits(), 1); // unchanged — the miss above didn't count
    }

    #[test]
    fn a_second_writer_never_writes_a_graph_snapshot() {
        let dir = tmp("graph-read-only");
        let first = ProjectCache::open(&dir);
        let second = ProjectCache::open(&dir);
        assert!(!second.writable);

        let key = [1u8; GRAPH_KEY_LEN];
        second.put_graph(&key, &sample_graph(), &[]);
        assert!(second.get_graph(&key).is_none());

        first.put_graph(&key, &sample_graph(), &[]);
        assert!(second.get_graph(&key).is_some()); // shared filesystem state, same as facts
    }

    #[test]
    fn corrupt_graph_snapshot_is_a_silent_miss() {
        let dir = tmp("graph-corrupt");
        let cache = ProjectCache::open(&dir);
        let key = [2u8; GRAPH_KEY_LEN];
        cache.put_graph(&key, &sample_graph(), &[]);
        fs::write(cache.graph_path(), b"not a valid snapshot").unwrap();
        assert!(cache.get_graph(&key).is_none());
    }
}
