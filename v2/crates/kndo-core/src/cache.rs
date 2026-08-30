//! The evidence cache: content-addressed, disposable, and versioned by the contract
//! fingerprint — a shape change in any evidence type invalidates every entry with no
//! constant to remember. The key also folds the adapter's id, `semantics_version` and
//! declared streams, so a behavior or declaration change invalidates exactly that
//! adapter's entries. Every failure path degrades to a miss or a skipped write; the
//! cache can slow a run down, never change it.

use kndo_contract::adapter::AdapterSpec;
use kndo_contract::evidence::FileEvidence;
use std::path::PathBuf;

const MAGIC: &[u8; 4] = b"KNE1";

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

    fn entry_path(&self, spec: &AdapterSpec, content_hash: &[u8; 32]) -> Option<PathBuf> {
        let dir = self.dir.as_ref()?;
        let mut h = blake3::Hasher::new();
        h.update(spec.id().as_bytes());
        h.update(&spec.semantics_version().to_le_bytes());
        h.update(&self.fingerprint);
        h.update(serde_json::to_string(spec.emits()).ok()?.as_bytes());
        h.update(content_hash);
        let hex: String = h
            .finalize()
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Some(dir.join(format!("{hex}.bin")))
    }

    pub fn get(&self, spec: &AdapterSpec, content_hash: &[u8; 32]) -> Option<FileEvidence> {
        let path = self.entry_path(spec, content_hash)?;
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

    pub fn put(&self, spec: &AdapterSpec, content_hash: &[u8; 32], evidence: &FileEvidence) {
        let Some(path) = self.entry_path(spec, content_hash) else {
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
