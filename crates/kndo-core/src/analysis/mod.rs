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

use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;

use rayon::prelude::*;

use crate::adapter::Diagnostic;
use crate::analysis::reachability::ReachabilityMap;
use crate::coverage::CoverageMap;
use crate::engine::Finding;
use crate::graph::{PackageNode, ProjectGraph};
use crate::vocab::{FileId, PackageId, SymbolId};

/// The five hash inputs of [`finding_id`], named so they can't be silently transposed at a
/// call site the way five adjacent `&str` positional parameters could — finding identity
/// (baseline matching, suppression stability) depends on getting this exactly right.
pub struct FindingIdParts<'a> {
    pub category: &'a crate::vocab::Category,
    pub subject_kind: &'a crate::vocab::SubjectKind,
    pub path: &'a str,
    pub symbol_path: &'a str,
    pub discriminator: &'a str,
}

/// The stable finding id: `"kndo-" + blake3(category,
/// subject_kind, path, symbol path, discriminator)[..12 hex]`. Line/column never participate,
/// so reformatting never changes an id; a rename or move does, because it changes `path`/
/// `symbol_path`. Hash input bytes are unchanged from before `FindingIdParts` existed —
/// `Category`/`SubjectKind`'s `Display` is exactly the string each used to be.
pub fn finding_id(parts: FindingIdParts<'_>) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in [
        parts.category.as_str(),
        parts.subject_kind.as_str(),
        parts.path,
        parts.symbol_path,
        parts.discriminator,
    ] {
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

/// Analysis-level tuning, resolved by the engine from `kndo.toml` (built-in defaults
/// otherwise). A separate input for the same reason `coverage` is: every knob acts strictly
/// post-assembly, so none of this belongs in the graph or its cache key.
#[derive(Debug, Clone)]
pub struct AnalysisTuning {
    /// `[analysis.crap] threshold` — scores above it are findings; also health's axis unit.
    pub crap_threshold: f64,
    /// `[analysis.duplicate] min-tokens` — smaller functions are not clone-matched. Never
    /// below the extraction floor ([`crate::config::DUPLICATE_MIN_TOKENS_FLOOR`]): under it
    /// the facts carry no fingerprints to match.
    pub duplicate_min_tokens: u32,
}

impl Default for AnalysisTuning {
    fn default() -> Self {
        AnalysisTuning {
            crap_threshold: crap::CRAP_THRESHOLD,
            duplicate_min_tokens: crate::config::DUPLICATE_MIN_TOKENS_FLOOR,
        }
    }
}

/// Everything one analysis needs to read — the graph, its precomputed reachability, the run's
/// ingested coverage, and the resolved tuning knobs. Shared, read-only, borrowed once per run.
pub(crate) struct AnalysisCtx<'a> {
    pub(crate) graph: &'a ProjectGraph,
    pub(crate) reach: &'a ReachabilityMap,
    pub(crate) coverage: &'a CoverageMap,
    pub(crate) tuning: &'a AnalysisTuning,
}

/// One analysis's contribution — findings plus whatever aux data `health` needs from it.
/// `cycle_files`/`duplicated` are populated by exactly one analysis each (`cyclic`,
/// `duplicate-functions`); every other analysis leaves them empty, which is why `Default`
/// merges cleanly regardless of which analysis produced a given output.
#[derive(Default)]
pub(crate) struct AnalysisOutput {
    pub(crate) findings: Vec<Finding>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) cycle_files: HashSet<FileId>,
    pub(crate) duplicated: Vec<(SymbolId, u32)>,
}

impl AnalysisOutput {
    fn findings(findings: Vec<Finding>) -> Self {
        AnalysisOutput {
            findings,
            ..Default::default()
        }
    }
}

/// One analysis, over a shared [`AnalysisCtx`]. Internal (contract §6: "may change any
/// release") — the uniform [`AnalysisOutput`] return type is what replaces the six divergent
/// shapes (`Vec<Finding>`, `(Vec<Finding>, Option<Diagnostic>)`, …) the underlying `find_*`
/// functions still return; this trait is the seam between them and [`run_all`]'s registry,
/// not a rewrite of the analyses themselves.
pub(crate) trait Analysis: Send + Sync {
    /// Also the `--verbose` timings label and (for `run_all`'s diagnostic-order lookup) the
    /// stable key into the collected outputs — matches the historical timing phase names.
    fn id(&self) -> &'static str;
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput;
}

struct UnusedAnalysis;
impl Analysis for UnusedAnalysis {
    fn id(&self) -> &'static str {
        "unused"
    }
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput {
        let mut f = unused::find_unused_files(ctx.graph, ctx.reach);
        f.extend(unused::find_unused_symbols(ctx.graph, ctx.reach));
        AnalysisOutput::findings(f)
    }
}

struct TestOnlyAnalysis;
impl Analysis for TestOnlyAnalysis {
    fn id(&self) -> &'static str {
        "test-only"
    }
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput {
        let mut f = test_only::find_test_only_files(ctx.graph, ctx.reach);
        f.extend(test_only::find_test_only_symbols(ctx.graph, ctx.reach));
        AnalysisOutput::findings(f)
    }
}

struct DependenciesAnalysis;
impl Analysis for DependenciesAnalysis {
    fn id(&self) -> &'static str {
        "dependencies"
    }
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput {
        let mut f = undeclared::find_undeclared_dependencies(ctx.graph);
        f.extend(version_skew::find_version_skew(ctx.graph));
        let (hygiene_findings, hygiene_diagnostic) =
            dependency_hygiene::find_dependency_hygiene(ctx.graph);
        f.extend(hygiene_findings);
        AnalysisOutput {
            findings: f,
            diagnostics: hygiene_diagnostic.into_iter().collect(),
            ..Default::default()
        }
    }
}

struct DuplicateFilesAnalysis;
impl Analysis for DuplicateFilesAnalysis {
    fn id(&self) -> &'static str {
        "duplicate-files"
    }
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput {
        AnalysisOutput::findings(duplicate::find_duplicate_files(ctx.graph))
    }
}

struct DuplicateFunctionsAnalysis;
impl Analysis for DuplicateFunctionsAnalysis {
    fn id(&self) -> &'static str {
        "duplicate-functions"
    }
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput {
        let (findings, duplicated) =
            duplicate::find_duplicate_functions(ctx.graph, ctx.tuning.duplicate_min_tokens);
        AnalysisOutput {
            findings,
            duplicated,
            ..Default::default()
        }
    }
}

struct InternalOnlyAnalysis;
impl Analysis for InternalOnlyAnalysis {
    fn id(&self) -> &'static str {
        "internal-only"
    }
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput {
        AnalysisOutput::findings(internal_only::find_internal_only(ctx.graph, ctx.reach))
    }
}

struct PrivateTypeLeakAnalysis;
impl Analysis for PrivateTypeLeakAnalysis {
    fn id(&self) -> &'static str {
        "private-type-leak"
    }
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput {
        AnalysisOutput::findings(private_type_leak::find_private_type_leaks(ctx.graph))
    }
}

struct DeepImportAnalysis;
impl Analysis for DeepImportAnalysis {
    fn id(&self) -> &'static str {
        "deep-import"
    }
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput {
        AnalysisOutput::findings(deep_import::find_deep_imports(ctx.graph))
    }
}

struct CyclicAnalysis;
impl Analysis for CyclicAnalysis {
    fn id(&self) -> &'static str {
        "cyclic"
    }
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput {
        let (findings, cycle_files) = cyclic::find_cycles(ctx.graph);
        AnalysisOutput {
            findings,
            cycle_files,
            ..Default::default()
        }
    }
}

struct CrapAnalysis;
impl Analysis for CrapAnalysis {
    fn id(&self) -> &'static str {
        "crap"
    }
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput {
        let (findings, diagnostic) =
            crap::find_crap(ctx.graph, ctx.coverage, ctx.tuning.crap_threshold);
        AnalysisOutput {
            findings,
            diagnostics: diagnostic.into_iter().collect(),
            ..Default::default()
        }
    }
}

struct UntestedAnalysis;
impl Analysis for UntestedAnalysis {
    fn id(&self) -> &'static str {
        "untested"
    }
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput {
        let (findings, diagnostic) = untested::find_untested(ctx.graph, ctx.reach);
        AnalysisOutput {
            findings,
            diagnostics: diagnostic.into_iter().collect(),
            ..Default::default()
        }
    }
}

/// Registry order fixes the `--verbose` timings order — matches the historical join-tree push
/// order exactly, so `--verbose` output is unchanged even though the join tree is gone.
fn registry() -> Vec<Box<dyn Analysis>> {
    vec![
        Box::new(UnusedAnalysis),
        Box::new(TestOnlyAnalysis),
        Box::new(DependenciesAnalysis),
        Box::new(DuplicateFilesAnalysis),
        Box::new(DuplicateFunctionsAnalysis),
        Box::new(InternalOnlyAnalysis),
        Box::new(PrivateTypeLeakAnalysis),
        Box::new(DeepImportAnalysis),
        Box::new(CyclicAnalysis),
        Box::new(CrapAnalysis),
        Box::new(UntestedAnalysis),
    ]
}

/// Diagnostic order predates the registry (`crap`, `untested`, `dependencies`-hygiene) —
/// preserved exactly rather than falling out of registry order, since only these three ever
/// emit one and nothing about their relative order is registry-position-derived.
const DIAGNOSTIC_ORDER: [&str; 3] = ["crap", "untested", "dependencies"];

/// Runs every analysis and returns their findings, sorted by id for deterministic output.
/// `coverage` is the run's ingested coverage — a separate input rather than part of
/// the graph, because report freshness varies independently of source content hashes and must
/// never be cached into a snapshot.
pub fn run_all(
    graph: &crate::graph::ProjectGraph,
    coverage: &crate::coverage::CoverageMap,
    tuning: &AnalysisTuning,
) -> AnalysisOutcome {
    let mut timings = Timings::new();
    let reach = timings.time("reachability", || reachability::compute(graph));
    let ctx = AnalysisCtx {
        graph,
        reach: &reach,
        coverage,
        tuning,
    };

    // Independent analyses run concurrently — a registry, not a hand-built join tree: each
    // entry's own elapsed time is real (they overlap under parallelism, so `--verbose`'s
    // total exceeds the wall clock by design, same as before). `par_iter().map().collect()`
    // preserves registry order regardless of completion order, so the reduce below stays
    // deterministic without an explicit sort.
    let registry = registry();
    let results: Vec<(&'static str, AnalysisOutput, u64)> = registry
        .par_iter()
        .map(|a| {
            let start = std::time::Instant::now();
            let out = a.run(&ctx);
            (a.id(), out, start.elapsed().as_micros() as u64)
        })
        .collect();

    let mut findings = Vec::new();
    let mut cycle_files: HashSet<FileId> = HashSet::default();
    let mut duplicated: Vec<(SymbolId, u32)> = Vec::new();
    let mut diagnostics_by_id: HashMap<&'static str, Vec<Diagnostic>> = HashMap::default();
    for (id, out, us) in results {
        findings.extend(out.findings);
        cycle_files.extend(out.cycle_files);
        duplicated.extend(out.duplicated);
        diagnostics_by_id.insert(id, out.diagnostics);
        timings.entries.push((id, us));
    }
    let diagnostics: Vec<Diagnostic> = DIAGNOSTIC_ORDER
        .into_iter()
        .filter_map(|id| diagnostics_by_id.remove(id))
        .flatten()
        .collect();

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
                crap_threshold: tuning.crap_threshold,
            },
        )
    });

    AnalysisOutcome {
        findings,
        diagnostics,
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
