//! The two caches, both content-addressed, disposable, and versioned by the contract
//! fingerprint — a shape change in any evidence type invalidates every entry with no
//! constant to remember. The evidence cache's key also folds the adapter's id,
//! `version` and declared streams, so a behavior or declaration change
//! invalidates exactly that adapter's entries — and an entry remembers which
//! extensions its embedded regions were handed to, by coordinate and version,
//! so a change in one of those misses too; the graph cache's key folds the whole
//! adapter set and `GRAPH_SEMANTICS_VERSION`. Every failure path degrades to a miss
//! or a skipped write; a cache can slow a run down, never change it.

use crate::graph::Graph;
use kndo_contract::evidence::FileEvidence;
use kndo_contract::plugin::{Plugin, PluginSpec};
use kndo_contract::vocab::ProjectPath;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::path::PathBuf;

const MAGIC: &[u8; 4] = b"KNE1";
const GRAPH_MAGIC: &[u8; 4] = b"KNG1";

pub struct EvidenceCache {
    dir: Option<PathBuf>,
    fingerprint: [u8; 32],
}

impl EvidenceCache {
    /// `dir = None` disables the cache (the `--no-cache` path).
    pub fn new(dir: Option<PathBuf>, fingerprint: [u8; 32]) -> Self {
        if let Some(d) = &dir {
            let _ = std::fs::create_dir_all(d);
        }
        EvidenceCache { dir, fingerprint }
    }

    fn entry_path(
        &self,
        spec: &PluginSpec,
        path: &ProjectPath,
        content_hash: &[u8; 32],
    ) -> Option<PathBuf> {
        let dir = self.dir.as_ref()?;
        let mut h = blake3::Hasher::new();
        h.update(spec.coordinate().as_bytes());
        h.update(&spec.version().to_le_bytes());
        h.update(&self.fingerprint);
        h.update(serde_json::to_string(spec.emits()).ok()?.as_bytes());
        // The path participates: extraction sees it, and adapters emit
        // path-conditional evidence (Swift's namespace clause IS its SwiftPM
        // target, spelled by the layout), so identical content at two paths is
        // not interchangeable.
        h.update(path.as_str().as_bytes());
        h.update(content_hash);
        let hex: String = h
            .finalize()
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Some(dir.join(format!("{hex}.bin")))
    }

    /// The cached evidence for `path` under `spec`, if the entry exists and the
    /// extensions its embedded regions were handed to are still loaded at the
    /// versions that produced it — the one part of the key extraction alone
    /// could learn, so it is checked here instead of hashed.
    pub fn get(
        &self,
        spec: &PluginSpec,
        path: &ProjectPath,
        content_hash: &[u8; 32],
        extensions: &[Box<dyn Plugin>],
    ) -> Option<FileEvidence> {
        let path = self.entry_path(spec, path, content_hash)?;
        let bytes = std::fs::read(path).ok()?;
        let (magic, rest) = bytes.split_at_checked(4)?;
        if magic != MAGIC {
            return None;
        }
        let (fp, payload) = rest.split_at_checked(32)?;
        if fp != self.fingerprint {
            return None;
        }
        let cached: CachedEvidence = bincode::deserialize(payload).ok()?;
        let current = cached.extractors.iter().all(|(coordinate, version)| {
            extensions
                .iter()
                .any(|e| e.spec().coordinate() == coordinate && e.spec().version() == *version)
        });
        current.then_some(cached.evidence)
    }

    /// Writes `evidence` for `path` under `spec`, remembering `extractors` —
    /// the (coordinate, version) of every extension an embedded region was
    /// handed to — for [`EvidenceCache::get`] to check.
    pub fn put(
        &self,
        spec: &PluginSpec,
        path: &ProjectPath,
        content_hash: &[u8; 32],
        evidence: &FileEvidence,
        extractors: &[(SmolStr, u32)],
    ) {
        let Some(path) = self.entry_path(spec, path, content_hash) else {
            return;
        };
        let entry = CachedEvidenceRef {
            extractors,
            evidence,
        };
        let Ok(payload) = bincode::serialize(&entry) else {
            return;
        };
        let mut bytes = Vec::with_capacity(4 + 32 + payload.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&self.fingerprint);
        bytes.extend_from_slice(&payload);
        write_atomically(&path, &bytes);
    }
}

/// One evidence entry as stored: the evidence and the extensions its embedded
/// regions were handed to, which the key could not fold since only
/// extraction learns them.
#[derive(Deserialize)]
struct CachedEvidence {
    extractors: Vec<(SmolStr, u32)>,
    evidence: FileEvidence,
}

/// [`CachedEvidence`] by reference, for writing without a clone; the two
/// serialize identically.
#[derive(Serialize)]
struct CachedEvidenceRef<'a> {
    extractors: &'a [(SmolStr, u32)],
    evidence: &'a FileEvidence,
}

/// A graph plus what its manifest-derived parts were computed from, so a later run
/// can tell whether they still hold.
#[derive(Serialize, Deserialize)]
pub struct PersistedGraph {
    /// Hash over every discovered manifest file's path and raw content, in path
    /// order — anchored roots and the package map are pure functions of it.
    pub manifest_state: [u8; 32],
    pub graph: Graph,
}

/// The persisted graph: one snapshot per project, keyed by everything that could
/// change how the same tree assembles (contract fingerprint, graph semantics,
/// the full adapter set).
pub struct GraphCache {
    file: Option<PathBuf>,
    key: [u8; 32],
}

impl GraphCache {
    pub fn new(dir: Option<PathBuf>, key: [u8; 32]) -> Self {
        if let Some(d) = &dir {
            let _ = std::fs::create_dir_all(d);
        }
        GraphCache {
            file: dir.map(|d| d.join("graph.bin")),
            key,
        }
    }

    pub fn load(&self) -> Option<PersistedGraph> {
        let bytes = std::fs::read(self.file.as_ref()?).ok()?;
        let (magic, rest) = bytes.split_at_checked(4)?;
        if magic != GRAPH_MAGIC {
            return None;
        }
        let (key, payload) = rest.split_at_checked(32)?;
        if key != self.key {
            return None;
        }
        bincode::deserialize(payload).ok()
    }

    pub fn store(&self, persisted: &PersistedGraph) {
        let Some(file) = &self.file else { return };
        let Ok(payload) = bincode::serialize(persisted) else {
            return;
        };
        let mut bytes = Vec::with_capacity(4 + 32 + payload.len());
        bytes.extend_from_slice(GRAPH_MAGIC);
        bytes.extend_from_slice(&self.key);
        bytes.extend_from_slice(&payload);
        write_atomically(file, &bytes);
    }
}

/// Temp-then-rename in the destination directory: a cache file is either the
/// old bytes or the new bytes, never a torn mix — two sessions on one root
/// (parallel tests, a user's second terminal) must not be able to hand each
/// other a half-written entry. Failures stay silent: a cache that cannot write
/// is a cache that misses.
fn write_atomically(path: &std::path::Path, bytes: &[u8]) {
    let Some(dir) = path.parent() else { return };
    let tmp = dir.join(format!(
        ".tmp-{}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Pinned sides ([`crate::session::PinnedSide`]): one small JSON file per
/// (analysis identity, tree) under `pinned/`, named by the key. Entries are
/// kilobytes — findings and a health block, never a graph — and the directory
/// is capped so a long-lived project does not grow it without bound: beyond the
/// cap the oldest by modification time go, a housekeeping order that never
/// touches what any run reports.
pub struct PinnedCache {
    dir: PathBuf,
}

const PINNED_CAP: usize = 32;

impl PinnedCache {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        PinnedCache { dir }
    }

    fn path(&self, key: &[u8; 32]) -> PathBuf {
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        self.dir.join(format!("{hex}.json"))
    }

    pub fn load(&self, key: &[u8; 32]) -> Option<crate::session::PinnedSide> {
        let bytes = std::fs::read(self.path(key)).ok()?;
        crate::session::PinnedSide::from_json(&bytes)
    }

    pub fn store(&self, key: &[u8; 32], side: &crate::session::PinnedSide) {
        let Some(bytes) = side.to_json() else {
            return;
        };
        write_atomically(&self.path(key), &bytes);
        self.evict();
    }

    fn evict(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter_map(|p| {
                let modified = std::fs::metadata(&p).ok()?.modified().ok()?;
                Some((modified, p))
            })
            .collect();
        if files.len() <= PINNED_CAP {
            return;
        }
        files.sort();
        for (_, path) in files.iter().take(files.len() - PINNED_CAP) {
            let _ = std::fs::remove_file(path);
        }
    }
}
