//! The two caches, both content-addressed, disposable, and versioned by the contract
//! fingerprint — a shape change in any evidence type invalidates every entry with no
//! constant to remember. The evidence cache's key also folds the adapter's id,
//! `semantics_version` and declared streams, so a behavior or declaration change
//! invalidates exactly that adapter's entries; the graph cache's key folds the whole
//! adapter set and `GRAPH_SEMANTICS_VERSION`. Every failure path degrades to a miss
//! or a skipped write; a cache can slow a run down, never change it.

use crate::graph::Graph;
use kndo_contract::adapter::AdapterSpec;
use kndo_contract::evidence::FileEvidence;
use kndo_contract::vocab::ProjectPath;
use serde::{Deserialize, Serialize};
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
        spec: &AdapterSpec,
        path: &ProjectPath,
        content_hash: &[u8; 32],
    ) -> Option<PathBuf> {
        let dir = self.dir.as_ref()?;
        let mut h = blake3::Hasher::new();
        h.update(spec.id().as_bytes());
        h.update(&spec.semantics_version().to_le_bytes());
        h.update(&self.fingerprint);
        h.update(serde_json::to_string(spec.emits()).ok()?.as_bytes());
        // The path participates: extraction sees it, and adapters emit
        // path-conditional evidence (a test-glob root), so identical content at two
        // paths is not interchangeable.
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

    pub fn get(
        &self,
        spec: &AdapterSpec,
        path: &ProjectPath,
        content_hash: &[u8; 32],
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
        bincode::deserialize(payload).ok()
    }

    pub fn put(
        &self,
        spec: &AdapterSpec,
        path: &ProjectPath,
        content_hash: &[u8; 32],
        evidence: &FileEvidence,
    ) {
        let Some(path) = self.entry_path(spec, path, content_hash) else {
            return;
        };
        let Ok(payload) = bincode::serialize(evidence) else {
            return;
        };
        let mut bytes = Vec::with_capacity(4 + 32 + payload.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&self.fingerprint);
        bytes.extend_from_slice(&payload);
        let _ = std::fs::write(path, bytes);
    }
}

/// A graph plus what its manifest-derived parts were computed from, so a later run
/// can tell whether they still hold.
#[derive(Serialize, Deserialize)]
pub struct PersistedGraph {
    /// Hash over every discovered manifest file's (path, content hash), in path
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
        let _ = std::fs::write(file, bytes);
    }
}
