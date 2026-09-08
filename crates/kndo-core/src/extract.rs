//! Claim + extract: first adapter whose claim globs match owns the file (M1 rule; the
//! priority-as-data registry arrives with real languages) — unless the path is one
//! its language's own tool never compiles ([`PluginSpec::ignores`]) or one THIS
//! PROJECT excludes ([`ManifestEvidence::ignores`]), which it then leaves
//! unclaimed. Both ignores stop at the claim: the file stays DISCOVERED, so an
//! unclaimed path under one casts no doubt on a dependency and a manifest that
//! names it is still read. Extraction runs in parallel, and the reduce is
//! deterministic — results are collected and consumed in path order, never in
//! completion order.
//!
//! [`PluginSpec::ignores`]: kndo_contract::plugin::PluginSpec::ignores
//! [`ManifestEvidence::ignores`]: kndo_contract::manifest::ManifestEvidence::ignores

use crate::cache::EvidenceCache;
use crate::discover::DiscoveredFile;
use kndo_contract::adapter::SourceFile;
use kndo_contract::evidence::{DiagnosticLevel, EvidenceSink, FileEvidence};
use kndo_contract::plugin::Plugin;
use rayon::prelude::*;
use smol_str::SmolStr;

pub struct ClaimedFile {
    pub file_index: usize,
    pub adapter_index: usize,
}

pub fn claim(
    files: &[DiscoveredFile],
    adapters: &[Box<dyn Plugin>],
    project_ignores: &globset::GlobSet,
) -> Vec<ClaimedFile> {
    let sets: Vec<globset::GlobSet> = adapters
        .iter()
        .map(|a| glob_set(a.spec().claims(), globset::Glob::new))
        .collect();
    let ignored = ignore_sets(adapters);

    files
        .iter()
        .enumerate()
        .filter(|(_, f)| !project_ignores.is_match(f.path.as_str()))
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
pub(crate) fn ignore_sets(adapters: &[Box<dyn Plugin>]) -> Vec<globset::GlobSet> {
    adapters
        .iter()
        .map(|a| ignore_set(a.spec().ignores()))
        .collect()
}

/// A set of globs over project paths, `*` staying inside one segment — the
/// spelling every path-shaped capability uses, so `**/*_test.go` reads the
/// same way in an ignore and in a file role.
pub(crate) fn path_glob_set<'a>(globs: impl Iterator<Item = &'a str>) -> globset::GlobSet {
    let owned: Vec<smol_str::SmolStr> = globs.map(smol_str::SmolStr::new).collect();
    ignore_set(&owned)
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
    adapters: &[Box<dyn Plugin>],
    cache: &EvidenceCache,
) -> Vec<FileEvidence> {
    claims
        .par_iter()
        .map(|c| {
            extract_one(
                &files[c.file_index],
                adapters[c.adapter_index].as_ref(),
                adapters,
                cache,
            )
        })
        .collect()
}

/// One file's evidence: the claiming adapter reads the file, then every
/// embedded region it reported is read by the extension claiming that
/// language's suffix, into the same evidence at the file's offsets. A region
/// of a language nothing claims is left unread, with a diagnostic on the file.
pub fn extract_one(
    file: &DiscoveredFile,
    adapter: &dyn Plugin,
    adapters: &[Box<dyn Plugin>],
    cache: &EvidenceCache,
) -> FileEvidence {
    let spec = adapter.spec();
    if let Some(hit) = cache.get(spec, &file.path, &file.hash, adapters) {
        return hit;
    }
    let mut sink = EvidenceSink::new(file.content.len() as u32, spec.emits().clone());
    adapter.extract(
        &SourceFile {
            path: &file.path,
            content: &file.content,
            region: None,
        },
        &mut sink,
    );
    let mut extractors: Vec<(SmolStr, u32)> = Vec::new();
    for (id, region) in sink.regions() {
        let Some(ix) = claimant_of_suffix(adapters, &region.language) else {
            sink.diagnostic(
                DiagnosticLevel::Warn,
                format!(
                    "no extension claims `{}` files — the embedded region at {}..{} is left unread",
                    region.language, region.span.start, region.span.end
                ),
                Some(region.span),
            );
            continue;
        };
        let extractor = adapters[ix].as_ref();
        let content = &file.content[region.span.start as usize..region.span.end as usize];
        sink.within(id, |sink| {
            extractor.extract(
                &SourceFile {
                    path: &file.path,
                    content,
                    region: Some(&region),
                },
                sink,
            )
        });
        extractors.push((
            SmolStr::new(extractor.spec().coordinate()),
            extractor.spec().version(),
        ));
    }
    extractors.sort_unstable();
    extractors.dedup();
    let evidence = sink.finish();
    cache.put(spec, &file.path, &file.hash, &evidence, &extractors);
    evidence
}

/// The extension that claims a file of `suffix` — the one an embedded region
/// of that language is handed to, and the one that resolves what the region
/// imports. The first registered wins, as it does for a file's claim.
pub(crate) fn claimant_of_suffix(adapters: &[Box<dyn Plugin>], suffix: &str) -> Option<usize> {
    adapters
        .iter()
        .position(|a| a.spec().suffixes().iter().any(|s| s == suffix))
}
