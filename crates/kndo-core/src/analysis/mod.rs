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
pub mod unresolved;
pub mod untested;
pub mod unused;
pub mod version_skew;

use rustc_hash::FxHashSet as HashSet;

use rayon::prelude::*;

use crate::adapter::Diagnostic;
use crate::analysis::reachability::ReachabilityMap;
use crate::coverage::CoverageMap;
use crate::engine::Finding;
use crate::graph::{PackageNode, ProjectGraph};
use crate::vocab::{Category, FileId, PackageId, SymbolId};

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
fn test_root_symbols(graph: &ProjectGraph) -> std::collections::HashSet<crate::vocab::SymbolId> {
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
fn package_discriminator(graph: &ProjectGraph, package: PackageId) -> String {
    match graph.packages[package.0 as usize].manifest.as_ref() {
        Some(path) => path.0.to_string(),
        None => String::new(),
    }
}

fn package_label(graph: &ProjectGraph, package: PackageId) -> String {
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
    /// `[[externally-invoked]]` — the project's own declaration of which markers mean "an
    /// entry point reached from outside the analyzed source". Belongs here, not in the graph,
    /// for the reason the struct doc gives: it is interpretation, applied post-assembly, so a
    /// change to it must never invalidate a cached graph. See
    /// [`reachability::externally_invoked_symbols`].
    pub externally_invoked: Vec<crate::config::ExternallyInvokedRule>,
    /// `[analysis.duplicate] min-tokens` — smaller functions are not clone-matched. Never
    /// below the extraction floor ([`crate::config::DUPLICATE_MIN_TOKENS_FLOOR`]): under it
    /// the facts carry no fingerprints to match.
    pub duplicate_min_tokens: u32,
    /// `--strict`: the analyses RFC 0005 marks as promotable raise their severity.
    ///
    /// Deliberately read by the analyses that own a promotable verdict rather than applied as
    /// a blanket post-pass: severity is part of what a verdict *means*, and a central table
    /// mapping every category to a strict severity would be a second place to keep in sync
    /// with the analysis that decides the ordinary one. Exactly one reads it today
    /// (`undeclared`), and adding a second is one line in that analysis.
    pub strict: bool,
}

impl Default for AnalysisTuning {
    fn default() -> Self {
        AnalysisTuning {
            crap_threshold: crap::CRAP_THRESHOLD,
            duplicate_min_tokens: crate::config::DUPLICATE_MIN_TOKENS_FLOOR,
            externally_invoked: Vec::new(),
            strict: false,
        }
    }
}

/// Everything one analysis needs to read — the graph, its precomputed reachability, the run's
/// ingested coverage, and the resolved tuning knobs. Shared, read-only, borrowed once per run.
struct AnalysisCtx<'a> {
    pub(crate) graph: &'a ProjectGraph,
    pub(crate) reach: &'a ReachabilityMap,
    pub(crate) coverage: &'a CoverageMap,
    pub(crate) tuning: &'a AnalysisTuning,
}

/// Whether an analysis's findings are the whole truth about its categories this run.
///
/// The distinction the finding list cannot carry: "no `crap` findings" means *clean* when the
/// analysis judged and *unknown* when it didn't, and every consumer that reads absence as
/// cleanliness is wrong in the second case — `stale` accusing a live pragma of acknowledging a
/// gone issue, `health` penalizing zero for an axis nobody measured, an agent reading the JSON.
#[derive(Debug, Default)]
pub enum Verdict {
    /// Judged. An absent finding means the code is clean.
    #[default]
    Judged,
    /// Did not judge — the input the verdict needs is absent this run (no coverage report, no
    /// test roots). Carries the ONE diagnostic that explains it, which is precisely why the
    /// text a user reads and the flag `suppression`/`health` consult cannot disagree: they are
    /// the same value, not two spellings of it.
    Abstained(Diagnostic),
}

/// One category nobody judged this run, and why — [`Verdict::Abstained`] crossed with the
/// abstaining analysis's [`Analysis::categories`]. Reaches `RunResult` and the JSON envelope
/// because a consumer seeing zero findings in a category otherwise cannot tell "clean" from
/// "not measured", nor what would make it measurable.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Abstention {
    pub category: crate::vocab::Category,
    /// The abstaining diagnostic's message, verbatim.
    pub reason: String,
}

/// One analysis's contribution — findings plus whatever aux data `health` needs from it.
/// `cycle_files`/`duplicated` are populated by exactly one analysis each (`cyclic`,
/// `duplicate-functions`); every other analysis leaves them empty, which is why `Default`
/// merges cleanly regardless of which analysis produced a given output.
#[derive(Default)]
struct AnalysisOutput {
    pub(crate) findings: Vec<Finding>,
    /// Diagnostics the analysis emits *while judging*. An abstention's own diagnostic does
    /// NOT belong here — it lives in `verdict`, and `run_all` folds it into the stream, so
    /// there is exactly one place that text exists.
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) verdict: Verdict,
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
trait Analysis: Send + Sync {
    /// Also the `--verbose` timings label — matches the historical timing phase names. Not a
    /// category: one analysis may own several (`dependencies` emits three).
    fn id(&self) -> &'static str;
    /// Every category this analysis can emit — what an abstention makes unknown. **No
    /// default**, deliberately, for the same reason [`crate::plugin::Plugin::mutates_graph`]
    /// has none: an empty list silently turns an abstention into a no-op, and a wrong list
    /// silences pragmas for a category that *was* judged. Decide it, don't inherit it.
    fn categories(&self) -> &'static [Category];
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput;
}

/// Per-analysis category tables. `static`, not an inline `&[…]`: [`Category`] wraps a
/// `SmolStr`, so a borrowed array literal is a temporary that cannot be promoted to `'static`.
static UNUSED_CATEGORIES: [Category; 1] = [Category::UNUSED];
static TEST_ONLY_CATEGORIES: [Category; 1] = [Category::TEST_ONLY];
/// `dependencies` owns four: `undeclared` and `version-skew` outright, plus the `unused`/
/// `test-only` facets its hygiene pass emits about dependencies (which is why a category is
/// only unknown when *every* owning analysis abstains — see `run_all`).
static DEPENDENCIES_CATEGORIES: [Category; 4] = [
    Category::UNDECLARED,
    Category::VERSION_SKEW,
    Category::UNUSED,
    Category::TEST_ONLY,
];
static DUPLICATE_CATEGORIES: [Category; 1] = [Category::DUPLICATE];
static INTERNAL_ONLY_CATEGORIES: [Category; 1] = [Category::INTERNAL_ONLY];
static PRIVATE_TYPE_LEAK_CATEGORIES: [Category; 1] = [Category::PRIVATE_TYPE_LEAK];
static DEEP_IMPORT_CATEGORIES: [Category; 1] = [Category::DEEP_IMPORT];
static CYCLIC_CATEGORIES: [Category; 1] = [Category::CYCLIC];
static CRAP_CATEGORIES: [Category; 1] = [Category::CRAP];
static UNTESTED_CATEGORIES: [Category; 1] = [Category::UNTESTED];
static UNRESOLVED_CATEGORIES: [Category; 1] = [Category::UNRESOLVED];

struct UnusedAnalysis;
impl Analysis for UnusedAnalysis {
    fn id(&self) -> &'static str {
        "unused"
    }
    fn categories(&self) -> &'static [Category] {
        &UNUSED_CATEGORIES
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
    fn categories(&self) -> &'static [Category] {
        &TEST_ONLY_CATEGORIES
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
    fn categories(&self) -> &'static [Category] {
        &DEPENDENCIES_CATEGORIES
    }
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput {
        let mut f = undeclared::find_undeclared_dependencies(ctx.graph, ctx.tuning.strict);
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
    fn categories(&self) -> &'static [Category] {
        &DUPLICATE_CATEGORIES
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
    fn categories(&self) -> &'static [Category] {
        &DUPLICATE_CATEGORIES
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
    fn categories(&self) -> &'static [Category] {
        &INTERNAL_ONLY_CATEGORIES
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
    fn categories(&self) -> &'static [Category] {
        &PRIVATE_TYPE_LEAK_CATEGORIES
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
    fn categories(&self) -> &'static [Category] {
        &DEEP_IMPORT_CATEGORIES
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
    fn categories(&self) -> &'static [Category] {
        &CYCLIC_CATEGORIES
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
    fn categories(&self) -> &'static [Category] {
        &CRAP_CATEGORIES
    }
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput {
        let (findings, verdict) =
            crap::find_crap(ctx.graph, ctx.coverage, ctx.tuning.crap_threshold);
        AnalysisOutput {
            findings,
            verdict,
            ..Default::default()
        }
    }
}

struct UnresolvedAnalysis;
impl Analysis for UnresolvedAnalysis {
    fn id(&self) -> &'static str {
        "unresolved"
    }
    fn categories(&self) -> &'static [Category] {
        &UNRESOLVED_CATEGORIES
    }
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput {
        AnalysisOutput::findings(unresolved::find_unresolved(ctx.graph))
    }
}

struct UntestedAnalysis;
impl Analysis for UntestedAnalysis {
    fn id(&self) -> &'static str {
        "untested"
    }
    fn categories(&self) -> &'static [Category] {
        &UNTESTED_CATEGORIES
    }
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput {
        let (findings, verdict) = untested::find_untested(ctx.graph, ctx.reach);
        AnalysisOutput {
            findings,
            verdict,
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
        Box::new(UnresolvedAnalysis),
    ]
}

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
    let reach = timings.time("reachability", || {
        let declared = reachability::externally_invoked_symbols(graph, &tuning.externally_invoked);
        reachability::compute_with_roots(graph, &declared)
    });
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
    let results: Vec<(&dyn Analysis, AnalysisOutput, u64)> = registry
        .par_iter()
        .map(|a| {
            let start = std::time::Instant::now();
            let out = a.run(&ctx);
            (a.as_ref(), out, start.elapsed().as_micros() as u64)
        })
        .collect();

    let mut findings = Vec::new();
    let mut cycle_files: HashSet<FileId> = HashSet::default();
    let mut duplicated: Vec<(SymbolId, u32)> = Vec::new();
    // Registry order for both, so `--verbose` timings and the diagnostics stream read in the
    // same order — and, unlike the hand-maintained order list this replaces, an analysis that
    // starts emitting a diagnostic can no longer have it silently dropped for not being named.
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    // A category is unknown only when EVERY analysis that can emit it abstained: `unused` and
    // `test-only` each have two owners (the symbol/file analysis and `dependencies`' hygiene
    // pass), and one owner's abstention says nothing about what the other judged.
    let mut abstained: Vec<(Category, String)> = Vec::new();
    let mut judged: HashSet<&'static str> = HashSet::default();
    for (analysis, out, us) in results {
        findings.extend(out.findings);
        cycle_files.extend(out.cycle_files);
        duplicated.extend(out.duplicated);
        diagnostics.extend(out.diagnostics);
        match out.verdict {
            Verdict::Judged => judged.extend(analysis.categories().iter().map(Category::as_str)),
            Verdict::Abstained(diagnostic) => {
                abstained.extend(
                    analysis
                        .categories()
                        .iter()
                        .map(|c| (c.clone(), diagnostic.message.clone())),
                );
                diagnostics.push(diagnostic);
            }
        }
        timings.entries.push((analysis.id(), us));
    }
    let mut seen: HashSet<String> = HashSet::default();
    let abstained: Vec<Abstention> = abstained
        .into_iter()
        .filter(|(category, _)| {
            !judged.contains(category.as_str()) && seen.insert(category.as_str().to_string())
        })
        .map(|(category, reason)| Abstention { category, reason })
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
                abstained: &abstained,
            },
        )
    });

    AnalysisOutcome {
        findings,
        diagnostics,
        abstained,
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
    /// Categories no analysis judged this run, each with the reason — registry order, one entry
    /// per category. Empty is the normal case and means every category was judged.
    pub abstained: Vec<Abstention>,
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
