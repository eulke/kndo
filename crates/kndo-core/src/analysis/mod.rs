//! Analyses: pure functions over the [`crate::graph::ProjectGraph`] (RFC 0005). Each analysis
//! consumes the graph plus whatever shared engines it needs (reachability, dup-detection, …)
//! and produces [`crate::engine::Finding`]s — never source text, never I/O.

pub mod crap;
pub mod cyclic;
pub mod deep_import;
pub mod dependency_hygiene;
pub mod duplicate;
pub mod health;
pub mod internal_only;
pub mod private_type_leak;
pub mod reachability;
mod rollup;
pub mod test_only;
pub mod undeclared;
pub mod untested;
pub mod unused;
pub mod version_skew;

use crate::adapter::Diagnostic;
use crate::engine::Finding;
use crate::graph::{PackageNode, ProjectGraph};
use crate::vocab::PackageId;

/// The stable finding id (contracts/output-schema.md §5): `"kndo-" + blake3(category,
/// subject_kind, path, symbol path, discriminator)[..12 hex]`. Line/column never participate,
/// so reformatting never changes an id; a rename or move does, because it changes `path`/
/// `symbol_path`.
pub fn finding_id(
    category: &str,
    subject_kind: &str,
    path: &str,
    symbol_path: &str,
    discriminator: &str,
) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in [category, subject_kind, path, symbol_path, discriminator] {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    let digest = hasher.finalize();
    format!("kndo-{}", &digest.to_hex()[..12])
}

/// The stable, empty-for-the-implicit-package identity used in finding ids — deliberately
/// *not* the human-readable label (which can be absent or a display name), so ids stay stable
/// across packages that share a name but not a manifest path. Shared by every per-package
/// dependency analysis (`undeclared`, `dependency_hygiene`).
pub(crate) fn package_discriminator(graph: &ProjectGraph, package: PackageId) -> String {
    match graph.packages[package.0 as usize].manifest.as_ref() {
        Some(path) => path.0.to_string(),
        None => String::new(),
    }
}

pub(crate) fn package_label(graph: &ProjectGraph, package: PackageId) -> String {
    match &graph.packages[package.0 as usize] {
        PackageNode {
            name: Some(name), ..
        } => name.to_string(),
        PackageNode {
            manifest: Some(path),
            ..
        } => path.0.to_string(),
        PackageNode { .. } => "the project (no manifest)".to_string(),
    }
}

/// Runs every analysis and returns their findings, sorted by id for deterministic output.
/// `coverage` is the run's ingested coverage (ADR 0005) — a separate input rather than part of
/// the graph, because report freshness varies independently of source content hashes and must
/// never be cached into a snapshot.
pub fn run_all(
    graph: &crate::graph::ProjectGraph,
    coverage: &crate::coverage::CoverageMap,
) -> AnalysisOutcome {
    let mut timings = Timings::new();
    let reach = timings.time("reachability", || reachability::compute(graph));

    let mut findings = timings.time("unused", || {
        let mut f = unused::find_unused_files(graph, &reach);
        f.extend(unused::find_unused_symbols(graph, &reach));
        f
    });
    findings.extend(timings.time("test-only", || {
        let mut f = test_only::find_test_only_files(graph, &reach);
        f.extend(test_only::find_test_only_symbols(graph, &reach));
        f
    }));
    findings.extend(timings.time("dependencies", || {
        let mut f = undeclared::find_undeclared_dependencies(graph);
        f.extend(version_skew::find_version_skew(graph));
        f.extend(dependency_hygiene::find_dependency_hygiene(graph));
        f
    }));
    findings.extend(timings.time("duplicate-files", || duplicate::find_duplicate_files(graph)));
    let (duplicate_findings, duplicated) = timings.time("duplicate-functions", || {
        duplicate::find_duplicate_functions(graph)
    });
    findings.extend(duplicate_findings);
    findings.extend(timings.time("internal-only", || {
        internal_only::find_internal_only(graph, &reach)
    }));
    findings.extend(timings.time("private-type-leak", || {
        private_type_leak::find_private_type_leaks(graph)
    }));
    findings.extend(timings.time("deep-import", || deep_import::find_deep_imports(graph)));
    let (cycle_findings, cycle_files) = timings.time("cyclic", || cyclic::find_cycles(graph));
    findings.extend(cycle_findings);
    findings.extend(timings.time("crap", || crap::find_crap(graph, coverage)));
    let (untested_findings, untested_diagnostic) =
        timings.time("untested", || untested::find_untested(graph, &reach));
    findings.extend(untested_findings);
    timings.time("sort-findings", || {
        findings.sort_unstable_by(|a, b| a.id.cmp(&b.id))
    });

    // Health is computed over the pre-suppression findings (the score measures the codebase,
    // not what's been acknowledged away) and the aux stats the analyses just produced.
    let health = timings.time("health", || {
        health::compute(
            graph,
            &reach,
            &findings,
            &health::HealthInputs {
                coverage,
                cycle_files: &cycle_files,
                duplicated: &duplicated,
            },
        )
    });

    AnalysisOutcome {
        findings,
        diagnostics: untested_diagnostic.into_iter().collect(),
        health,
        timings: timings.entries,
    }
}

/// Everything one analysis pass produces: findings (id-sorted), analysis-side diagnostics,
/// the health score computed from the same primitives (RFC 0005 §11), and per-phase wall
/// times (RFC 0009 §6's `--verbose` timings; RFC 0008 §7's profiling discipline needs the
/// numbers to be one flag away, not a rebuild away).
pub struct AnalysisOutcome {
    pub findings: Vec<Finding>,
    pub diagnostics: Vec<Diagnostic>,
    pub health: health::Health,
    /// `(phase, duration in µs)` in execution order. Never serialized into the JSON envelope —
    /// wall times are run metadata, not analysis output, and the §4 determinism matrix compares
    /// envelopes byte-for-byte.
    pub timings: Vec<(&'static str, u64)>,
}

/// Tiny collector for the per-phase wall times above.
struct Timings {
    entries: Vec<(&'static str, u64)>,
}

impl Timings {
    fn new() -> Timings {
        Timings {
            entries: Vec::new(),
        }
    }

    fn time<T>(&mut self, phase: &'static str, f: impl FnOnce() -> T) -> T {
        let start = std::time::Instant::now();
        let out = f();
        self.entries
            .push((phase, start.elapsed().as_micros() as u64));
        out
    }
}
