//! Analyses: pure functions over the [`crate::graph::ProjectGraph`]. Each analysis
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

/// The stable finding id: `"kndo-" + blake3(category,
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

/// Symbols that are themselves `Test` roots — inline test infrastructure (`#[test]`
/// functions and everything an adapter roots inside a `#[cfg(test)]` module; Rust is the
/// first language whose tests live inside production files). `test-only` and `untested`
/// exempt them: a test being reachable only from tests is the definition of a test, not a
/// finding.
pub(crate) fn test_root_symbols(
    graph: &ProjectGraph,
) -> std::collections::HashSet<crate::vocab::SymbolId> {
    use crate::vocab::{EdgeKind, NodeRef, RootKind};
    graph
        .edges
        .iter()
        .filter_map(|e| match e.kind {
            EdgeKind::Root {
                kind: RootKind::Test,
                target: NodeRef::Symbol(s),
            } => Some(s),
            _ => None,
        })
        .collect()
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
/// `coverage` is the run's ingested coverage — a separate input rather than part of
/// the graph, because report freshness varies independently of source content hashes and must
/// never be cached into a snapshot.
pub fn run_all(
    graph: &crate::graph::ProjectGraph,
    coverage: &crate::coverage::CoverageMap,
) -> AnalysisOutcome {
    let mut timings = Timings::new();
    let reach = timings.time("reachability", || reachability::compute(graph));
    let reach = &reach;

    // Independent analyses run concurrently (the inter-analysis parallelism) via an
    // explicit join tree — parallel compute, deterministic reduce: every result lands in
    // a named slot, findings are extended in the same fixed order as ever (and id-sorted
    // below regardless), and per-phase timings are pushed in that fixed order after the join.
    // The timing values themselves are each phase's own elapsed time — under parallelism they
    // overlap, so the `--verbose` block's total exceeds the wall clock by design.
    let timed = |f: &dyn Fn() -> Vec<Finding>| {
        let start = std::time::Instant::now();
        (f(), start.elapsed().as_micros() as u64)
    };
    #[allow(clippy::type_complexity)]
    let (
        ((unused_r, test_only_r), (dependencies_r, duplicate_files_r)),
        (
            ((duplicate_fn_r, duplicate_fn_us), internal_only_r),
            ((ptl_r, deep_import_r), ((cyclic_r, cyclic_us), (crap_r, untested_r))),
        ),
    ) = rayon::join(
        || {
            rayon::join(
                || {
                    rayon::join(
                        || {
                            timed(&|| {
                                let mut f = unused::find_unused_files(graph, reach);
                                f.extend(unused::find_unused_symbols(graph, reach));
                                f
                            })
                        },
                        || {
                            timed(&|| {
                                let mut f = test_only::find_test_only_files(graph, reach);
                                f.extend(test_only::find_test_only_symbols(graph, reach));
                                f
                            })
                        },
                    )
                },
                || {
                    rayon::join(
                        || {
                            let start = std::time::Instant::now();
                            let mut f = undeclared::find_undeclared_dependencies(graph);
                            f.extend(version_skew::find_version_skew(graph));
                            let (hygiene_findings, hygiene_diagnostic) =
                                dependency_hygiene::find_dependency_hygiene(graph);
                            f.extend(hygiene_findings);
                            ((f, hygiene_diagnostic), start.elapsed().as_micros() as u64)
                        },
                        || timed(&|| duplicate::find_duplicate_files(graph)),
                    )
                },
            )
        },
        || {
            rayon::join(
                || {
                    rayon::join(
                        || {
                            let start = std::time::Instant::now();
                            let out = duplicate::find_duplicate_functions(graph);
                            (out, start.elapsed().as_micros() as u64)
                        },
                        || timed(&|| internal_only::find_internal_only(graph, reach)),
                    )
                },
                || {
                    rayon::join(
                        || {
                            rayon::join(
                                || timed(&|| private_type_leak::find_private_type_leaks(graph)),
                                || timed(&|| deep_import::find_deep_imports(graph)),
                            )
                        },
                        || {
                            rayon::join(
                                || {
                                    let start = std::time::Instant::now();
                                    let out = cyclic::find_cycles(graph);
                                    (out, start.elapsed().as_micros() as u64)
                                },
                                || {
                                    rayon::join(
                                        || {
                                            let start = std::time::Instant::now();
                                            let out = crap::find_crap(graph, coverage);
                                            (out, start.elapsed().as_micros() as u64)
                                        },
                                        || {
                                            let start = std::time::Instant::now();
                                            let out = untested::find_untested(graph, reach);
                                            (out, start.elapsed().as_micros() as u64)
                                        },
                                    )
                                },
                            )
                        },
                    )
                },
            )
        },
    );

    let (duplicate_findings, duplicated) = duplicate_fn_r;
    let (cycle_findings, cycle_files) = cyclic_r;
    let ((crap_findings, crap_diagnostic), crap_us) = crap_r;
    let ((untested_findings, untested_diagnostic), untested_us) = untested_r;
    let ((dependency_findings, hygiene_diagnostic), dependencies_us) = dependencies_r;

    let mut findings = unused_r.0;
    timings.entries.push(("unused", unused_r.1));
    findings.extend(test_only_r.0);
    timings.entries.push(("test-only", test_only_r.1));
    findings.extend(dependency_findings);
    timings.entries.push(("dependencies", dependencies_us));
    findings.extend(duplicate_files_r.0);
    timings
        .entries
        .push(("duplicate-files", duplicate_files_r.1));
    findings.extend(duplicate_findings);
    timings
        .entries
        .push(("duplicate-functions", duplicate_fn_us));
    findings.extend(internal_only_r.0);
    timings.entries.push(("internal-only", internal_only_r.1));
    findings.extend(ptl_r.0);
    timings.entries.push(("private-type-leak", ptl_r.1));
    findings.extend(deep_import_r.0);
    timings.entries.push(("deep-import", deep_import_r.1));
    findings.extend(cycle_findings);
    timings.entries.push(("cyclic", cyclic_us));
    findings.extend(crap_findings);
    timings.entries.push(("crap", crap_us));
    findings.extend(untested_findings);
    timings.entries.push(("untested", untested_us));
    timings.time("sort-findings", || {
        findings.sort_unstable_by(|a, b| a.id.cmp(&b.id))
    });

    // Health is computed over the pre-suppression findings (the score measures the codebase,
    // not what's been acknowledged away) and the aux stats the analyses just produced.
    let health = timings.time("health", || {
        health::compute(
            graph,
            reach,
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
        diagnostics: crap_diagnostic
            .into_iter()
            .chain(untested_diagnostic)
            .chain(hygiene_diagnostic)
            .collect(),
        health,
        timings: timings.entries,
    }
}

/// Everything one analysis pass produces: findings (id-sorted), analysis-side diagnostics,
/// the health score computed from the same primitives, and per-phase wall
/// times (the `--verbose` timings; the profiling discipline needs the
/// numbers to be one flag away, not a rebuild away).
pub struct AnalysisOutcome {
    pub findings: Vec<Finding>,
    pub diagnostics: Vec<Diagnostic>,
    pub health: health::Health,
    /// `(phase, duration in µs)` in execution order. Never serialized into the JSON envelope —
    /// wall times are run metadata, not analysis output, and the determinism matrix compares
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
