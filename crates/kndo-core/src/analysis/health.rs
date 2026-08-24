//! `health` — the 0–100 composite project score:
//! `health = 100 − Σ weight × saturating_ratio(category)`, deterministic and documented so
//! trends are meaningful. The weights and saturation constants below are the contract;
//! `saturating_ratio(r) = min(r / saturation, 1)` — linear near zero (small improvements
//! always move the score) and capped at the category's full weight (a single bad file can't
//! zero the score).
//!
//! Health is computed from the same primitives the analyses use (reachability colors, metrics,
//! coverage, and the findings where the analysis itself owns the verdict), **before** baseline
//! and suppression are applied: the score measures the codebase's state, not how much of it
//! has been acknowledged away. Two gates mirror their analyses: the test-blind-spot category
//! is skipped entirely when the project has no test roots (the "a repo without tests gets one
//! diagnostic, not a thousand findings" — the same honesty applies to a penalty that would
//! otherwise saturate by definition), and cycle participation counts only tolerance-reported
//! cycles (a cycle the language declares impossible or idiomatic isn't a penalty).
//!
//! Grades: A ≥ 90, B ≥ 80, C ≥ 65, D ≥ 50, F below. The per-package
//! breakdown is the same penalties grouped by owning package — never a different metric — and is
//! included whenever the project has more than one package owning claimed files.

use rustc_hash::FxHashSet as HashSet;

use crate::analysis::crap::crap_score;
use crate::analysis::reachability::{Reachability, ReachabilityMap};
use crate::coverage::CoverageMap;
use crate::engine::Finding;
use crate::graph::ProjectGraph;
use crate::vocab::{
    EdgeKind, FileId, FileOrigin, FileRole, NodeRef, PackageId, RootKind, SymbolId,
};

/// One category's weight and the ratio at which its full weight saturates (the
/// "per-category curve", as documented constants).
struct CategorySpec {
    name: &'static str,
    weight: f64,
    saturation: f64,
}

const UNUSED_SYMBOLS: CategorySpec = CategorySpec {
    name: "unused-symbols",
    weight: 25.0,
    saturation: 0.25,
};
const UNUSED_DEPENDENCIES: CategorySpec = CategorySpec {
    name: "unused-dependencies",
    weight: 15.0,
    saturation: 0.5,
};
const UNUSED_FILES: CategorySpec = CategorySpec {
    name: "unused-files",
    weight: 10.0,
    saturation: 0.25,
};
const TEST_ONLY: CategorySpec = CategorySpec {
    name: "test-only",
    weight: 10.0,
    saturation: 0.25,
};
const DUPLICATION: CategorySpec = CategorySpec {
    name: "duplication",
    weight: 20.0,
    saturation: 0.3,
};
const CRAP: CategorySpec = CategorySpec {
    name: "crap",
    weight: 20.0,
    saturation: 0.5,
};
const CYCLES: CategorySpec = CategorySpec {
    name: "cycles",
    weight: 5.0,
    saturation: 0.25,
};
const INTERNAL_ONLY: CategorySpec = CategorySpec {
    name: "internal-only",
    weight: 5.0,
    saturation: 0.5,
};
const UNTESTED: CategorySpec = CategorySpec {
    name: "untested",
    weight: 5.0,
    saturation: 0.5,
};

/// The health block. `previous` is filled by the engine (before-side in diff modes, the
/// stored snapshot in full mode) — computation here is always "this graph, now".
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Health {
    /// 0–100, one-decimal precision.
    pub score: f64,
    pub grade: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<HealthSummary>,
    pub categories: Vec<HealthCategory>,
    /// Per-package breakdown — present when more than one package owns claimed
    /// files; the single-package (and no-manifest) common case omits it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<PackageHealth>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthSummary {
    pub score: f64,
    pub grade: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthCategory {
    pub category: String,
    /// The raw ratio (numerator / denominator), before the saturation curve.
    pub ratio: f64,
    pub penalty: f64,
    /// Subjects counted by the numerator (dead symbols, orphan files, misdeclared deps, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_duplicated: Option<u64>,
    /// Σ CRAP score over threshold-exceeding functions (the hotlist's total load).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crapload: Option<f64>,
    /// Ingested coverage provenance ("coverage-lcov coverage/lcov.info (2d old)"), or "none".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PackageHealth {
    pub package: String,
    pub score: f64,
    pub grade: String,
}

pub fn grade(score: f64) -> &'static str {
    if score >= 90.0 {
        "A"
    } else if score >= 80.0 {
        "B"
    } else if score >= 65.0 {
        "C"
    } else if score >= 50.0 {
        "D"
    } else {
        "F"
    }
}

/// Everything `run_all` hands over beyond the graph itself: aux stats individual analyses
/// already computed (never recomputed here) plus the run's ingested coverage.
pub struct HealthInputs<'a> {
    pub coverage: &'a CoverageMap,
    /// Files participating in tolerance-reported cycles (from `cyclic`).
    pub cycle_files: &'a HashSet<FileId>,
    /// Redundant clone instances with token counts (from `duplicate`).
    pub duplicated: &'a [(SymbolId, u32)],
    /// The effective CRAP threshold (`AnalysisTuning::crap_threshold`) — the same value the
    /// `crap` analysis judged with, so the axis and the findings can never disagree.
    pub crap_threshold: f64,
}

pub fn compute(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    findings: &[Finding],
    inputs: &HealthInputs<'_>,
) -> Health {
    let (categories, score) = tally(graph, reach, findings, inputs, None);

    // Per-package breakdown: only packages that own claimed files, only when there are ≥ 2
    // (a single-package repo's breakdown is the global score restated).
    let mut owning: Vec<PackageId> = Vec::new();
    let mut seen: HashSet<u32> = HashSet::default();
    for file in &graph.files {
        if file.language.is_some() && seen.insert(file.package.0) {
            owning.push(file.package);
        }
    }
    let mut packages = Vec::new();
    if owning.len() >= 2 {
        for &p in &owning {
            let (_, pkg_score) = tally(graph, reach, findings, inputs, Some(p));
            packages.push(PackageHealth {
                package: crate::analysis::package_label(graph, p),
                score: pkg_score,
                grade: grade(pkg_score).to_string(),
            });
        }
        packages.sort_by(|a, b| a.package.cmp(&b.package));
    }

    Health {
        score,
        grade: grade(score).to_string(),
        previous: None,
        categories,
        packages,
    }
}

/// A file that participates in the ratios at all: claimed, authored, not test-role.
fn eligible_file(graph: &ProjectGraph, file: FileId, scope: Option<PackageId>) -> bool {
    let f = &graph.files[file.0 as usize];
    if scope.is_some_and(|p| f.package != p) {
        return false;
    }
    let Some(class) = f.class else { return false };
    f.language.is_some() && class.role != FileRole::Test && class.origin == FileOrigin::Authored
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn tally(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    findings: &[Finding],
    inputs: &HealthInputs<'_>,
    scope: Option<PackageId>,
) -> (Vec<HealthCategory>, f64) {
    let scope_label = scope.map(|p| crate::analysis::package_label(graph, p));
    let in_scope_finding = |f: &Finding| match &scope_label {
        None => true,
        Some(label) => f.location.package.as_deref() == Some(label.as_str()),
    };

    // Symbol-axis counts in one pass.
    let mut total_symbols = 0usize;
    let mut dead_symbols = 0usize;
    let mut test_only_symbols = 0usize;
    let mut production_symbols = 0usize;
    let mut untested_symbols = 0usize;
    let mut exported_symbols = 0usize;
    let has_test_roots = graph.edges.iter().any(|e| {
        matches!(
            e.kind,
            EdgeKind::Root {
                kind: RootKind::Test,
                ..
            }
        )
    });
    for (index, symbol) in graph.symbols.iter().enumerate() {
        if !eligible_file(graph, symbol.file, scope) {
            continue;
        }
        // Sub-file test regions (FileFacts::test_spans): inline test infrastructure in a
        // production file is out of the ratios entirely, exactly as test files are — it is
        // neither dead weight nor untested production code.
        if crate::graph::span_in_test_region(
            &graph.files[symbol.file.0 as usize].test_spans,
            symbol.span,
        ) {
            continue;
        }
        total_symbols += 1;
        if symbol.exported {
            exported_symbols += 1;
        }
        let node = NodeRef::Symbol(SymbolId(index as u32));
        match reach.get(node).0 {
            Reachability::Unreachable => dead_symbols += 1,
            Reachability::TestOnly => test_only_symbols += 1,
            Reachability::Production => {
                production_symbols += 1;
                if has_test_roots && !reach.reachable_from(RootKind::Test, node) {
                    untested_symbols += 1;
                }
            }
            Reachability::ToolingOnly => {}
        }
    }

    // File axis.
    let mut total_files = 0usize;
    let mut orphan_files = 0usize;
    let mut cycle_files = 0usize;
    for index in 0..graph.files.len() {
        let file_id = FileId(index as u32);
        if !eligible_file(graph, file_id, scope) {
            continue;
        }
        total_files += 1;
        if reach.get(NodeRef::File(file_id)).0 == Reachability::Unreachable {
            orphan_files += 1;
        }
        if inputs.cycle_files.contains(&file_id) {
            cycle_files += 1;
        }
    }

    // Dependency axis: the analyses own these verdicts (per-ecosystem semantics live there),
    // so the numerator counts their findings rather than re-deriving them.
    let declared = graph
        .declared_dependencies
        .iter()
        .filter(|d| scope.is_none_or(|p| d.package == p))
        .count();
    let misdeclared = findings
        .iter()
        .filter(|f| {
            f.subject_kind == "dependency"
                && matches!(f.category.as_str(), "unused" | "test-only" | "undeclared")
                && in_scope_finding(f)
        })
        .count();

    // Duplication axis (tokens): redundant copies over the total normalized
    // stream, same generated/vendored exemption as the duplicate analysis itself.
    let mut total_tokens = 0u64;
    for (symbol_id, metrics) in &graph.function_metrics {
        let symbol = &graph.symbols[symbol_id.0 as usize];
        let file = &graph.files[symbol.file.0 as usize];
        if scope.is_some_and(|p| file.package != p) {
            continue;
        }
        let Some(class) = file.class else { continue };
        if matches!(class.origin, FileOrigin::Generated | FileOrigin::Vendored) {
            continue;
        }
        total_tokens += u64::from(metrics.token_count);
    }
    let duplicated_tokens: u64 = inputs
        .duplicated
        .iter()
        .filter(|(symbol_id, _)| {
            scope.is_none_or(|p| {
                graph.files[graph.symbols[symbol_id.0 as usize].file.0 as usize].package == p
            })
        })
        .map(|&(_, tokens)| u64::from(tokens))
        .sum();

    // CRAP axis: excess over the threshold, normalized by threshold-units per function.
    // With no coverage ingested the crap analysis is skipped (its diagnostic says so), and
    // the score must not silently punish what the check deliberately didn't measure — the
    // axis contributes zero penalty and the category row reports the absence explicitly.
    let crap_measured = !inputs.coverage.is_empty();
    let mut crap_functions = 0usize;
    let mut crapload = 0.0f64;
    let mut crap_excess = 0.0f64;
    let mut crap_over = 0usize;
    if crap_measured {
        for (symbol_id, metrics) in &graph.function_metrics {
            let symbol = &graph.symbols[symbol_id.0 as usize];
            if !eligible_file(graph, symbol.file, scope) {
                continue;
            }
            crap_functions += 1;
            let file = &graph.files[symbol.file.0 as usize];
            let cov = inputs
                .coverage
                .function_coverage(&file.path, symbol.span)
                .unwrap_or(0.0);
            let score = crap_score(metrics.cyclomatic, cov);
            if score > inputs.crap_threshold {
                crap_over += 1;
                crapload += score;
                crap_excess += score - inputs.crap_threshold;
            }
        }
    }

    let internal_only = findings
        .iter()
        .filter(|f| f.category == "internal-only" && in_scope_finding(f))
        .count();

    let ratio = |num: f64, denom: f64| if denom > 0.0 { num / denom } else { 0.0 };
    let coverage_desc = if inputs.coverage.sources.is_empty() {
        "none".to_string()
    } else {
        inputs.coverage.sources.join(", ")
    };

    let mut categories = Vec::new();
    let mut push = |spec: &CategorySpec, raw: f64, extra: Extra| {
        let penalty = spec.weight * (raw / spec.saturation).min(1.0);
        categories.push(HealthCategory {
            category: spec.name.to_string(),
            ratio: (raw * 1000.0).round() / 1000.0,
            penalty: round1(penalty),
            count: extra.count,
            tokens_duplicated: extra.tokens_duplicated,
            crapload: extra.crapload,
            coverage: extra.coverage,
        });
    };

    push(
        &UNUSED_SYMBOLS,
        ratio(dead_symbols as f64, total_symbols as f64),
        Extra::count(dead_symbols),
    );
    push(
        &UNUSED_DEPENDENCIES,
        ratio(misdeclared as f64, declared as f64),
        Extra::count(misdeclared),
    );
    push(
        &UNUSED_FILES,
        ratio(orphan_files as f64, total_files as f64),
        Extra::count(orphan_files),
    );
    push(
        &TEST_ONLY,
        ratio(test_only_symbols as f64, total_symbols as f64),
        Extra::count(test_only_symbols),
    );
    push(
        &DUPLICATION,
        ratio(duplicated_tokens as f64, total_tokens as f64),
        Extra {
            tokens_duplicated: Some(duplicated_tokens),
            ..Extra::default()
        },
    );
    push(
        &CRAP,
        ratio(crap_excess, inputs.crap_threshold * crap_functions as f64),
        Extra {
            count: crap_measured.then_some(crap_over),
            crapload: crap_measured.then(|| round1(crapload)),
            coverage: Some(coverage_desc),
            ..Extra::default()
        },
    );
    push(
        &CYCLES,
        ratio(cycle_files as f64, total_files as f64),
        Extra::count(cycle_files),
    );
    push(
        &INTERNAL_ONLY,
        ratio(internal_only as f64, exported_symbols as f64),
        Extra::count(internal_only),
    );
    if has_test_roots {
        push(
            &UNTESTED,
            ratio(untested_symbols as f64, production_symbols as f64),
            Extra::count(untested_symbols),
        );
    }

    let score = (100.0 - categories.iter().map(|c| c.penalty).sum::<f64>()).max(0.0);
    (categories, round1(score))
}

#[derive(Default)]
struct Extra {
    count: Option<usize>,
    tokens_duplicated: Option<u64>,
    crapload: Option<f64>,
    coverage: Option<String>,
}

impl Extra {
    fn count(n: usize) -> Extra {
        Extra {
            count: Some(n),
            ..Extra::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ProjectPath, Span, VisibilityLevel};
    use crate::analysis::reachability;
    use crate::graph::{FileNode, SymbolMetrics, SymbolNode};
    use crate::vocab::{Confidence, Edge, FileClass, Provenance, SymbolKind};
    use smol_str::SmolStr;

    fn file(path: &str, package: u32) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: Some(SmolStr::new("mock")),
            class: Some(FileClass {
                role: FileRole::Production,
                origin: FileOrigin::Authored,
            }),
            package: PackageId(package),
            unit: None,
            test_spans: Vec::new(),
            string_call_sites: Vec::new(),
        }
    }

    fn symbol(file: u32, name: &str) -> SymbolNode {
        SymbolNode {
            file: FileId(file),
            name: SmolStr::new(name),
            kind: SymbolKind::Function,
            span: Span {
                start: (1, 1),
                end: (5, 1),
            },
            exported: true,
            visibility: VisibilityLevel(1),
            member_of: None,
            signature_span: None,
            implicitly_invoked: false,
            nested_scope: false,
            visibility_inherited: false,
        }
    }

    fn prod_root(file: u32) -> Edge {
        Edge {
            owner: crate::vocab::FileId(0),
            kind: EdgeKind::Root {
                kind: RootKind::Production,
                target: NodeRef::File(FileId(file)),
            },
            confidence: Confidence::Certain,
            source: Provenance::Adapter(SmolStr::new("mock")),
            span: None,
        }
    }

    fn inputs<'a>(
        coverage: &'a CoverageMap,
        cycles: &'a HashSet<FileId>,
        duplicated: &'a [(SymbolId, u32)],
    ) -> HealthInputs<'a> {
        HealthInputs {
            coverage,
            cycle_files: cycles,
            duplicated,
            crap_threshold: crate::analysis::crap::CRAP_THRESHOLD,
        }
    }

    #[test]
    fn a_clean_reachable_project_scores_100_grade_a() {
        let graph = crate::graph::ProjectGraph::for_test(
            vec![file("src/a.mock", 0)],
            vec![symbol(0, "used")],
            vec![],
            vec![
                prod_root(0),
                Edge {
                    owner: crate::vocab::FileId(0),
                    kind: EdgeKind::References {
                        from: NodeRef::File(FileId(0)),
                        to: SymbolId(0),
                        kind: crate::vocab::RefKind::Read,
                    },
                    confidence: Confidence::Certain,
                    source: Provenance::Adapter(SmolStr::new("mock")),
                    span: None,
                },
            ],
        );
        let reach = reachability::compute(&graph);
        let (cov, cyc, dup) = (CoverageMap::default(), HashSet::default(), vec![]);
        let h = compute(&graph, &reach, &[], &inputs(&cov, &cyc, &dup));
        assert_eq!(h.score, 100.0);
        assert_eq!(h.grade, "A");
        assert!(h.packages.is_empty(), "single package: no breakdown");
        // No test roots: the untested category is skipped, not charged.
        assert!(!h.categories.iter().any(|c| c.category == "untested"));
    }

    #[test]
    fn symbols_inside_test_regions_stay_out_of_the_ratios() {
        // A dead symbol whose span sits in a test region (FileFacts::test_spans) is inline
        // test infrastructure — neither numerator nor denominator, same as test files.
        let mut f = file("src/a.mock", 0);
        f.test_spans = vec![Span {
            start: (100, 1),
            end: (200, 999),
        }];
        let mut symbols = vec![symbol(0, "used"), symbol(0, "test_helper")];
        symbols[1].span = Span {
            start: (110, 1),
            end: (120, 1),
        };
        let graph = crate::graph::ProjectGraph::for_test(
            vec![f],
            symbols,
            vec![],
            vec![
                prod_root(0),
                Edge {
                    owner: crate::vocab::FileId(0),
                    kind: EdgeKind::References {
                        from: NodeRef::File(FileId(0)),
                        to: SymbolId(0),
                        kind: crate::vocab::RefKind::Read,
                    },
                    confidence: Confidence::Certain,
                    source: Provenance::Adapter(SmolStr::new("mock")),
                    span: None,
                },
            ],
        );
        let reach = reachability::compute(&graph);
        let (cov, cyc, dup) = (CoverageMap::default(), HashSet::default(), vec![]);
        let h = compute(&graph, &reach, &[], &inputs(&cov, &cyc, &dup));
        assert_eq!(
            h.score, 100.0,
            "the unreferenced in-region symbol must not charge unused-symbols"
        );
    }

    #[test]
    fn dead_symbols_charge_the_unused_symbols_penalty_linearly_then_saturate() {
        // 1 of 4 symbols dead: ratio 0.25 = the saturation point → the full 25-point weight.
        let mut symbols: Vec<SymbolNode> = (0..4).map(|i| symbol(0, &format!("s{i}"))).collect();
        symbols[3].name = SmolStr::new("dead");
        let mut edges = vec![prod_root(0)];
        for i in 0..3 {
            edges.push(Edge {
                owner: crate::vocab::FileId(0),
                kind: EdgeKind::References {
                    from: NodeRef::File(FileId(0)),
                    to: SymbolId(i),
                    kind: crate::vocab::RefKind::Read,
                },
                confidence: Confidence::Certain,
                source: Provenance::Adapter(SmolStr::new("mock")),
                span: None,
            });
        }
        let graph = crate::graph::ProjectGraph::for_test(
            vec![file("src/a.mock", 0)],
            symbols,
            vec![],
            edges,
        );
        let reach = reachability::compute(&graph);
        let (cov, cyc, dup) = (CoverageMap::default(), HashSet::default(), vec![]);
        let h = compute(&graph, &reach, &[], &inputs(&cov, &cyc, &dup));
        let unused = h
            .categories
            .iter()
            .find(|c| c.category == "unused-symbols")
            .unwrap();
        assert_eq!(unused.count, Some(1));
        assert_eq!(unused.penalty, 25.0, "0.25 ratio saturates the full weight");
        assert_eq!(h.score, 75.0);
        assert_eq!(h.grade, "C");
    }

    #[test]
    fn duplication_penalty_uses_token_ratio() {
        // Two symbols, 100 tokens each; one is a redundant clone → ratio 0.5, saturated (0.3)
        // → the full 20-point duplication weight.
        let graph = crate::graph::ProjectGraph::for_test(
            vec![file("src/a.mock", 0)],
            vec![symbol(0, "a"), symbol(0, "b")],
            vec![],
            vec![prod_root(0)],
        )
        .with_function_metrics(vec![
            (
                SymbolId(0),
                SymbolMetrics {
                    cyclomatic: 1,
                    loc: 5,
                    token_count: 100,
                    fingerprints: vec![1],
                },
            ),
            (
                SymbolId(1),
                SymbolMetrics {
                    cyclomatic: 1,
                    loc: 5,
                    token_count: 100,
                    fingerprints: vec![1],
                },
            ),
        ]);
        let reach = reachability::compute(&graph);
        let (cov, cyc) = (CoverageMap::default(), HashSet::default());
        let dup = vec![(SymbolId(1), 100u32)];
        let h = compute(&graph, &reach, &[], &inputs(&cov, &cyc, &dup));
        let d = h
            .categories
            .iter()
            .find(|c| c.category == "duplication")
            .unwrap();
        assert_eq!(d.tokens_duplicated, Some(100));
        assert_eq!(d.penalty, 20.0);
    }

    #[test]
    fn crap_category_reports_crapload_and_coverage_provenance() {
        // comp 8, report present but its lines unhit → cov 0 → CRAP 72, excess 42 over one
        // function → raw ratio 42/30 = 1.4, saturated → full 20 points.
        let graph = crate::graph::ProjectGraph::for_test(
            vec![file("src/a.mock", 0)],
            vec![symbol(0, "gnarly")],
            vec![],
            vec![prod_root(0)],
        )
        .with_function_metrics(vec![(
            SymbolId(0),
            SymbolMetrics {
                cyclomatic: 8,
                loc: 5,
                token_count: 80,
                fingerprints: vec![],
            },
        )]);
        let reach = reachability::compute(&graph);
        let mut sink = crate::coverage::CoverageSink::default();
        for line in 1..=5 {
            sink.add_line(
                crate::adapter::ProjectPath(SmolStr::new("src/a.mock")),
                line,
                0,
            );
        }
        let cov = sink.into_map();
        let (cyc, dup) = (HashSet::default(), vec![]);
        let h = compute(&graph, &reach, &[], &inputs(&cov, &cyc, &dup));
        let c = h.categories.iter().find(|c| c.category == "crap").unwrap();
        assert_eq!(c.count, Some(1));
        assert_eq!(c.crapload, Some(72.0));
        assert_eq!(c.penalty, 20.0);
    }

    #[test]
    fn no_ingested_coverage_leaves_the_crap_axis_unmeasured() {
        // Same gnarly function, but no report at all: the crap analysis is skipped, and the
        // score must not punish what wasn't measured — zero penalty, the row visible with the
        // absence explicit.
        let graph = crate::graph::ProjectGraph::for_test(
            vec![file("src/a.mock", 0)],
            vec![symbol(0, "gnarly")],
            vec![],
            vec![prod_root(0)],
        )
        .with_function_metrics(vec![(
            SymbolId(0),
            SymbolMetrics {
                cyclomatic: 8,
                loc: 5,
                token_count: 80,
                fingerprints: vec![],
            },
        )]);
        let reach = reachability::compute(&graph);
        let (cov, cyc, dup) = (CoverageMap::default(), HashSet::default(), vec![]);
        let h = compute(&graph, &reach, &[], &inputs(&cov, &cyc, &dup));
        let c = h.categories.iter().find(|c| c.category == "crap").unwrap();
        assert_eq!(c.count, None);
        assert_eq!(c.crapload, None);
        assert_eq!(c.coverage.as_deref(), Some("none"));
        assert_eq!(c.penalty, 0.0);
    }

    #[test]
    fn grade_boundaries_match_the_rfc() {
        assert_eq!(grade(90.0), "A");
        assert_eq!(grade(89.9), "B");
        assert_eq!(grade(80.0), "B");
        assert_eq!(grade(79.9), "C");
        assert_eq!(grade(65.0), "C");
        assert_eq!(grade(64.9), "D");
        assert_eq!(grade(50.0), "D");
        assert_eq!(grade(49.9), "F");
    }

    #[test]
    fn multi_package_projects_get_a_per_package_breakdown() {
        use crate::graph::PackageNode;
        let graph = crate::graph::ProjectGraph::for_test(
            vec![file("a/src/x.mock", 1), file("b/src/y.mock", 2)],
            vec![symbol(0, "alive"), symbol(1, "dead")],
            vec![],
            vec![
                prod_root(0),
                prod_root(1),
                Edge {
                    owner: crate::vocab::FileId(0),
                    kind: EdgeKind::References {
                        from: NodeRef::File(FileId(0)),
                        to: SymbolId(0),
                        kind: crate::vocab::RefKind::Read,
                    },
                    confidence: Confidence::Certain,
                    source: Provenance::Adapter(SmolStr::new("mock")),
                    span: None,
                },
            ],
        )
        .with_packages(vec![
            PackageNode {
                workspace_entry: None,
                targets: Vec::new(),
                executables: Vec::new(),
                manifest: None,
                name: None,
                private: false,
                declares_surface: false,
                surface: Vec::new(),
                resolves_dependency_usage: true,
            },
            PackageNode {
                workspace_entry: None,
                targets: Vec::new(),
                executables: Vec::new(),
                manifest: Some(ProjectPath(SmolStr::new("a/pkg.json"))),
                name: Some(SmolStr::new("pkg-a")),
                private: false,
                declares_surface: false,
                surface: Vec::new(),
                resolves_dependency_usage: true,
            },
            PackageNode {
                workspace_entry: None,
                targets: Vec::new(),
                executables: Vec::new(),
                manifest: Some(ProjectPath(SmolStr::new("b/pkg.json"))),
                name: Some(SmolStr::new("pkg-b")),
                private: false,
                declares_surface: false,
                surface: Vec::new(),
                resolves_dependency_usage: true,
            },
        ]);
        let reach = reachability::compute(&graph);
        let (cov, cyc, dup) = (CoverageMap::default(), HashSet::default(), vec![]);
        let h = compute(&graph, &reach, &[], &inputs(&cov, &cyc, &dup));
        assert_eq!(h.packages.len(), 2);
        let a = h.packages.iter().find(|p| p.package == "pkg-a").unwrap();
        let b = h.packages.iter().find(|p| p.package == "pkg-b").unwrap();
        assert_eq!(a.score, 100.0, "pkg-a is fully alive");
        assert!(b.score < 100.0, "pkg-b carries the dead symbol");
    }
}
