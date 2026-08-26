//! `crap` — Change Risk Anti-Patterns: per callable,
//! `CRAP(m) = comp(m)² × (1 − cov(m))³ + comp(m)`, with `comp` the adapter-extracted
//! cyclomatic complexity ([`crate::graph::SymbolMetrics`]) and `cov` the covered fraction of
//! the function's instrumented lines from ingested reports ([`crate::coverage::CoverageMap`]
//! — coverage is a separate `run_all` input precisely because its freshness varies
//! independently of the graph's content hashes).
//!
//! With **no report ingested at all**, the analysis is skipped with one diagnostic: the
//! score's coverage factor would be a guess for every function at once, not a measurement,
//! and a category-wide guess is noise, not risk (the same posture `untested` takes for a
//! project with no test roots). With a report present, a function the report simply doesn't
//! instrument ⇒ "CRAPload with cov=0, flagged `coverage: none`" — pessimistic, and the
//! message says why, rather than silently skipping the exact functions most likely to be the
//! problem. Test code is exempt (a test's own coverage is meaningless), as are
//! generated/vendored files (not yours to refactor).

use crate::adapter::{Diagnostic, DiagnosticLevel};
use crate::analysis::{finding_id, FindingIdParts};
use crate::coverage::CoverageMap;
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::{Category, Confidence, FileOrigin, FileRole, Group, SubjectKind};

/// The standard threshold ("findings for CRAP > 30") — the built-in default;
/// `[analysis.crap] threshold` in `kndo.toml` overrides it per project
/// (`AnalysisTuning::crap_threshold`).
pub(crate) const CRAP_THRESHOLD: f64 = 30.0;

pub fn crap_score(cyclomatic: u32, coverage: f64) -> f64 {
    let comp = cyclomatic as f64;
    comp * comp * (1.0 - coverage).powi(3) + comp
}

pub fn find_crap(
    graph: &ProjectGraph,
    coverage: &CoverageMap,
    threshold: f64,
) -> (Vec<Finding>, Option<Diagnostic>) {
    if coverage.is_empty() {
        return (
            Vec::new(),
            Some(Diagnostic {
                level: DiagnosticLevel::Info,
                path: None,
                message: "crap: no coverage ingested — skipped (the score is complexity × \
                          untestedness; without a report the coverage factor would be a guess, \
                          not a measurement — drop a report at a well-known path (lcov.info, \
                          coverage.xml, jacoco.xml, coverage.out, …) or point \
                          [plugins.<id>] report at one to enable it)"
                    .to_string(),
                span: None,
            }),
        );
    }

    let mut findings = Vec::new();
    for (symbol_id, metrics) in &graph.function_metrics {
        let symbol = &graph.symbols[symbol_id.0 as usize];
        let file = &graph.files[symbol.file.0 as usize];
        let Some(class) = file.class else {
            continue;
        };
        if class.role == FileRole::Test
            || matches!(class.origin, FileOrigin::Generated | FileOrigin::Vendored)
        {
            continue;
        }
        // Sub-file test regions (FileFacts::test_spans): a `#[test]` fn or a `#[cfg(test)]`
        // module member inside a production file is test code — same exemption as test
        // files, at span granularity.
        if crate::graph::span_in_test_region(&file.test_spans, symbol.span) {
            continue;
        }

        let cov = coverage.function_coverage(&file.path, symbol.span);
        let score = crap_score(metrics.cyclomatic, cov.unwrap_or(0.0));
        if score <= threshold {
            continue;
        }

        let path = file.path.0.as_str();
        let facet = symbol.kind.facet();
        let qualified = symbol.qualified_name();
        let coverage_text = match cov {
            Some(c) => format!("coverage {:.0}%", c * 100.0),
            None => "coverage: none".to_string(),
        };
        findings.push(Finding {
            advisory: false,
            id: finding_id(FindingIdParts {
                category: &Category::CRAP,
                subject_kind: &SubjectKind::new(facet),
                path,
                symbol_path: &qualified,
                discriminator: "",
            }),
            category: Category::CRAP,
            group: Group::Risk,
            subject_kind: SubjectKind::new(facet),
            severity: Severity::Warning,
            confidence: Confidence::Certain,
            message: format!(
                "{path}#{qualified} has CRAP {score:.0} (complexity {comp}, {coverage_text}) — \
                 above the threshold of {threshold:.0}",
                comp = metrics.cyclomatic,
            ),
            location: Location {
                path: Some(file.path.clone()),
                range: Some(symbol.span),
                symbol: Some(qualified),
                package: graph.package_name(file.package).map(str::to_string),
            },
            related: Vec::new(),
            delta: None,
            delta_origin: None,
        });
    }
    (findings, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ProjectPath, Span, VisibilityLevel};
    use crate::coverage::CoverageSink;
    use crate::graph::{FileNode, SymbolMetrics, SymbolNode};
    use crate::vocab::{FileClass, FileId, SymbolId, SymbolKind};
    use smol_str::SmolStr;

    fn file(path: &str, role: FileRole, origin: FileOrigin) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: Some(SmolStr::new("mock")),
            class: Some(FileClass { role, origin }),
            package: crate::vocab::PackageId(0),
            unit: None,
            unit_parent: None,
            test_spans: Vec::new(),
            string_call_sites: Vec::new(),
        }
    }

    fn symbol(file: FileId, name: &str, start: u32, end: u32) -> SymbolNode {
        SymbolNode {
            file,
            name: SmolStr::new(name),
            kind: SymbolKind::Function,
            span: Span {
                start: (start, 1),
                end: (end, 1),
            },
            exported: true,
            visibility: VisibilityLevel(1),
            member_of: None,
            signature_span: None,
            implicitly_invoked: false,
            nested_scope: false,
            visibility_inherited: false,
            visible_in_unit: None,
            implements: None,
            markers: Vec::new(),
        }
    }

    fn metrics(cyclomatic: u32) -> SymbolMetrics {
        SymbolMetrics {
            cyclomatic,
            loc: 10,
            token_count: 60,
            fingerprints: Vec::new(),
        }
    }

    fn graph_with(
        files: Vec<FileNode>,
        symbols: Vec<SymbolNode>,
        function_metrics: Vec<(SymbolId, SymbolMetrics)>,
    ) -> ProjectGraph {
        ProjectGraph::for_test(files, symbols, vec![], vec![])
            .with_function_metrics(function_metrics)
    }

    /// A non-empty map that instruments a file none of the fixtures use: the analysis runs
    /// (a report exists) while every fixture function stays report-unknown — the pessimistic
    /// cov=0 path, distinct from the no-report skip.
    fn unrelated_coverage() -> CoverageMap {
        let mut sink = CoverageSink::default();
        sink.add_line(ProjectPath(SmolStr::new("elsewhere.mock")), 1, 1);
        sink.into_map()
    }

    #[test]
    fn formula_matches_the_rfc() {
        // comp 6, cov 0: 36 × 1 + 6 = 42. comp 6, cov 1: 0 + 6 = 6.
        assert!((crap_score(6, 0.0) - 42.0).abs() < 1e-9);
        assert!((crap_score(6, 1.0) - 6.0).abs() < 1e-9);
        // comp 10, cov 0.5: 100 × 0.125 + 10 = 22.5.
        assert!((crap_score(10, 0.5) - 22.5).abs() < 1e-9);
    }

    #[test]
    fn symbols_inside_test_regions_are_exempt() {
        // A gnarly `#[test]` fn in a production file (FileFacts::test_spans) gets the same
        // exemption as a test file's functions; the same complexity outside the region fires.
        let mut f = file("src/a.mock", FileRole::Production, FileOrigin::Authored);
        f.test_spans = vec![crate::adapter::Span {
            start: (100, 1),
            end: (200, 999),
        }];
        let graph = graph_with(
            vec![f],
            vec![
                symbol(FileId(0), "gnarly_test", 110, 140),
                symbol(FileId(0), "gnarly_prod", 10, 40),
            ],
            vec![(SymbolId(0), metrics(6)), (SymbolId(1), metrics(6))],
        );
        let findings = find_crap(&graph, &unrelated_coverage(), CRAP_THRESHOLD).0;
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("gnarly_prod"));
    }

    #[test]
    fn uncovered_complex_function_fires_flagged_coverage_none() {
        let graph = graph_with(
            vec![file(
                "src/a.mock",
                FileRole::Production,
                FileOrigin::Authored,
            )],
            vec![symbol(FileId(0), "gnarly", 1, 30)],
            vec![(SymbolId(0), metrics(6))],
        );
        let findings = find_crap(&graph, &unrelated_coverage(), CRAP_THRESHOLD).0;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "crap");
        assert_eq!(findings[0].group, crate::vocab::Group::Risk);
        assert_eq!(findings[0].severity, Severity::Warning);
        assert!(findings[0].message.contains("CRAP 42"));
        assert!(findings[0].message.contains("coverage: none"));
    }

    #[test]
    fn coverage_pulls_the_score_below_the_threshold() {
        let graph = graph_with(
            vec![file(
                "src/a.mock",
                FileRole::Production,
                FileOrigin::Authored,
            )],
            vec![symbol(FileId(0), "gnarly", 1, 30)],
            vec![(SymbolId(0), metrics(6))],
        );
        let mut sink = CoverageSink::default();
        for line in 1..=30 {
            sink.add_line(ProjectPath(SmolStr::new("src/a.mock")), line, 1);
        }
        let findings = find_crap(&graph, &sink.into_map(), CRAP_THRESHOLD).0;
        assert!(findings.is_empty(), "fully covered: CRAP = comp = 6");
        // The threshold is a knob, not a constant: the same fully-covered function
        // (CRAP = 6) is a finding under a stricter configured threshold, and the
        // message names the effective value.
        let mut sink = CoverageSink::default();
        for line in 1..=30 {
            sink.add_line(ProjectPath(SmolStr::new("src/a.mock")), line, 1);
        }
        let strict = find_crap(&graph, &sink.into_map(), 5.0).0;
        assert_eq!(strict.len(), 1);
        assert!(
            strict[0].message.contains("threshold of 5"),
            "{}",
            strict[0].message
        );
    }

    #[test]
    fn partial_coverage_scores_between_the_extremes() {
        // comp 8, half covered: 64 × 0.125 + 8 = 16 — under threshold. Uncovered it would be
        // 72 — over. The report is what changes the verdict.
        let graph = graph_with(
            vec![file(
                "src/a.mock",
                FileRole::Production,
                FileOrigin::Authored,
            )],
            vec![symbol(FileId(0), "halfway", 1, 20)],
            vec![(SymbolId(0), metrics(8))],
        );
        assert_eq!(
            find_crap(&graph, &unrelated_coverage(), CRAP_THRESHOLD)
                .0
                .len(),
            1
        );
        let mut sink = CoverageSink::default();
        for line in 1..=20 {
            sink.add_line(
                ProjectPath(SmolStr::new("src/a.mock")),
                line,
                u64::from(line <= 10),
            );
        }
        let findings = find_crap(&graph, &sink.into_map(), CRAP_THRESHOLD).0;
        assert!(findings.is_empty());
    }

    #[test]
    fn simple_functions_never_fire_even_uncovered() {
        // comp 5 uncovered: 25 + 5 = 30 — exactly at the threshold, and the rule is "> 30".
        let graph = graph_with(
            vec![file(
                "src/a.mock",
                FileRole::Production,
                FileOrigin::Authored,
            )],
            vec![symbol(FileId(0), "plain", 1, 10)],
            vec![(SymbolId(0), metrics(5))],
        );
        assert!(find_crap(&graph, &unrelated_coverage(), CRAP_THRESHOLD)
            .0
            .is_empty());
    }

    #[test]
    fn test_and_generated_code_are_exempt() {
        let graph = graph_with(
            vec![
                file("tests/a.mock", FileRole::Test, FileOrigin::Authored),
                file("gen/b.mock", FileRole::Production, FileOrigin::Generated),
            ],
            vec![
                symbol(FileId(0), "testHelper", 1, 30),
                symbol(FileId(1), "generated", 1, 30),
            ],
            vec![(SymbolId(0), metrics(9)), (SymbolId(1), metrics(9))],
        );
        assert!(find_crap(&graph, &unrelated_coverage(), CRAP_THRESHOLD)
            .0
            .is_empty());
    }

    #[test]
    fn finding_id_is_line_free_and_stable() {
        let make = |start: u32| {
            graph_with(
                vec![file(
                    "src/a.mock",
                    FileRole::Production,
                    FileOrigin::Authored,
                )],
                vec![symbol(FileId(0), "gnarly", start, start + 29)],
                vec![(SymbolId(0), metrics(6))],
            )
        };
        let a = find_crap(&make(1), &unrelated_coverage(), CRAP_THRESHOLD).0;
        let b = find_crap(&make(50), &unrelated_coverage(), CRAP_THRESHOLD).0;
        assert_eq!(
            a[0].id, b[0].id,
            "moving the function must not change the id"
        );
    }

    #[test]
    fn no_ingested_coverage_skips_with_one_diagnostic() {
        let graph = graph_with(
            vec![file(
                "src/a.mock",
                FileRole::Production,
                FileOrigin::Authored,
            )],
            vec![symbol(FileId(0), "gnarly", 1, 30)],
            vec![(SymbolId(0), metrics(20))],
        );
        let (findings, diagnostic) = find_crap(&graph, &CoverageMap::default(), CRAP_THRESHOLD);
        assert!(
            findings.is_empty(),
            "no report ⇒ no findings, however complex the code"
        );
        let d = diagnostic.expect("the skip must say so");
        assert!(
            d.message.starts_with("crap: no coverage ingested"),
            "{}",
            d.message
        );
    }
}
