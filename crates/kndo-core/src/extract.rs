//! Claim + extract: first adapter whose claim globs match owns the file (M1 rule; the
//! priority-as-data registry arrives with real languages) — unless the path is one
//! its language's own tool never compiles ([`ExtensionSpec::ignores`]), which it
//! then leaves unclaimed. Extraction runs in parallel, and the reduce is
//! deterministic — results are collected and consumed in path order, never in
//! completion order.
//!
//! [`ExtensionSpec::ignores`]: kndo_contract::extension::ExtensionSpec::ignores

use crate::cache::EvidenceCache;
use crate::discover::DiscoveredFile;
use kndo_contract::adapter::SourceFile;
use kndo_contract::evidence::{EvidenceSink, FileEvidence};
use kndo_contract::extension::Extension;
use rayon::prelude::*;

pub struct ClaimedFile {
    pub file_index: usize,
    pub adapter_index: usize,
}

pub fn claim(files: &[DiscoveredFile], adapters: &[Box<dyn Extension>]) -> Vec<ClaimedFile> {
    let sets: Vec<globset::GlobSet> = adapters
        .iter()
        .map(|a| glob_set(a.spec().claims(), globset::Glob::new))
        .collect();
    let ignored = ignore_sets(adapters);

    files
        .iter()
        .enumerate()
        .filter_map(|(file_index, f)| {
            sets.iter()
                .zip(&ignored)
                .position(|(claims, ignores)| {
                    claims.is_match(f.path.as_str()) && !ignores.is_match(f.path.as_str())
                })
                .map(|adapter_index| ClaimedFile {
                    file_index,
                    adapter_index,
                })
        })
        .collect()
}

/// Per adapter, the paths its language's own tool never compiles — what the
/// claim pass and every manifest pass leave alone. An ignore names path
/// segments: `*` stays inside one, so `**/_*.go` is a file whose name begins
/// with `_`, never a file under a directory that does.
pub(crate) fn ignore_sets(adapters: &[Box<dyn Extension>]) -> Vec<globset::GlobSet> {
    adapters
        .iter()
        .map(|a| ignore_set(a.spec().ignores()))
        .collect()
}

/// One adapter's ignores as a set — see [`ignore_sets`].
pub(crate) fn ignore_set(globs: &[smol_str::SmolStr]) -> globset::GlobSet {
    glob_set(globs, |g| {
        globset::GlobBuilder::new(g).literal_separator(true).build()
    })
}

/// The set of every glob `parse` accepts — an unparsable one is skipped, never
/// a refusal to run.
fn glob_set(
    globs: &[smol_str::SmolStr],
    parse: impl Fn(&str) -> Result<globset::Glob, globset::Error>,
) -> globset::GlobSet {
    let mut b = globset::GlobSetBuilder::new();
    for g in globs {
        if let Ok(glob) = parse(g) {
            b.add(glob);
        }
    }
    b.build().unwrap_or_else(|_| globset::GlobSet::empty())
}

/// Extraction through the cache: a hit skips the adapter entirely (the key already
/// folds everything that could change the output); a miss extracts through a sink
/// primed with the adapter's declared streams and writes back best-effort.
pub fn extract(
    files: &[DiscoveredFile],
    claims: &[ClaimedFile],
    adapters: &[Box<dyn Extension>],
    cache: &EvidenceCache,
) -> Vec<FileEvidence> {
    claims
        .par_iter()
        .map(|c| {
            extract_one(
                &files[c.file_index],
                adapters[c.adapter_index].as_ref(),
                cache,
            )
        })
        .collect()
}

pub fn extract_one(
    file: &DiscoveredFile,
    adapter: &dyn Extension,
    cache: &EvidenceCache,
) -> FileEvidence {
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
}
