//! Claim + extract: first adapter whose claim globs match owns the file (M1 rule; the
//! priority-as-data registry arrives with real languages), extraction runs in
//! parallel, and the reduce is deterministic — results are collected and consumed in
//! path order, never in completion order.

use crate::cache::EvidenceCache;
use crate::discover::DiscoveredFile;
use kndo_contract::adapter::{LanguageAdapter, SourceFile};
use kndo_contract::evidence::{EvidenceSink, FileEvidence};
use rayon::prelude::*;

pub struct ClaimedFile {
    pub file_index: usize,
    pub adapter_index: usize,
}

pub fn claim(files: &[DiscoveredFile], adapters: &[Box<dyn LanguageAdapter>]) -> Vec<ClaimedFile> {
    let sets: Vec<globset::GlobSet> = adapters
        .iter()
        .map(|a| {
            let mut b = globset::GlobSetBuilder::new();
            for g in a.spec().claims() {
                if let Ok(glob) = globset::Glob::new(g) {
                    b.add(glob);
                }
            }
            b.build().unwrap_or_else(|_| globset::GlobSet::empty())
        })
        .collect();

    files
        .iter()
        .enumerate()
        .filter_map(|(file_index, f)| {
            sets.iter()
                .position(|s| s.is_match(f.path.as_str()))
                .map(|adapter_index| ClaimedFile {
                    file_index,
                    adapter_index,
                })
        })
        .collect()
}

/// Extraction through the cache: a hit skips the adapter entirely (the key already
/// folds everything that could change the output); a miss extracts through a sink
/// primed with the adapter's declared streams and writes back best-effort.
pub fn extract(
    files: &[DiscoveredFile],
    claims: &[ClaimedFile],
    adapters: &[Box<dyn LanguageAdapter>],
    cache: &EvidenceCache,
) -> Vec<FileEvidence> {
    claims
        .par_iter()
        .map(|c| {
            let file = &files[c.file_index];
            let adapter = &adapters[c.adapter_index];
            let spec = adapter.spec();
            if let Some(hit) = cache.get(spec, &file.path, &file.hash) {
                return hit;
            }
            let mut sink = EvidenceSink::new(file.content.len() as u32, spec.emits().clone());
            adapter.extract(
                &SourceFile {
                    path: &file.path,
                    content: &file.content,
                },
                &mut sink,
            );
            let evidence = sink.finish();
            cache.put(spec, &file.path, &file.hash, &evidence);
            evidence
        })
        .collect()
}
