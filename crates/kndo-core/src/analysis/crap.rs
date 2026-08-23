//! `crap` — Change Risk Anti-Patterns (RFC 0005 §10): per callable,
//! `CRAP(m) = comp(m)² × (1 − cov(m))³ + comp(m)`, with `comp` the adapter-extracted
//! cyclomatic complexity ([`crate::graph::SymbolMetrics`]) and `cov` the covered fraction of
//! the function's instrumented lines from ingested reports ([`crate::coverage::CoverageMap`],
//! ADR 0005 — coverage is a separate `run_all` input precisely because its freshness varies
//! independently of the graph's content hashes).
//!
//! No coverage data for a function ⇒ the RFC's "CRAPload with cov=0, flagged
//! `coverage: none`" — the score is computed pessimistically and the message says why, rather
//! than silently skipping the exact functions most likely to be the problem. Test code is
//! exempt (a test's own coverage is meaningless), as are generated/vendored files (not yours
//! to refactor).

use crate::analysis::finding_id;
use crate::coverage::CoverageMap;
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::{Confidence, FileOrigin, FileRole};

/// RFC 0005 §10's standard threshold ("findings for CRAP > 30"); configurability lands with
/// the config file.
pub(crate) const CRAP_THRESHOLD: f64 = 30.0;

pub fn crap_score(cyclomatic: u32, coverage: f64) -> f64 {
    let comp = cyclomatic as f64;
    comp * comp * (1.0 - coverage).powi(3) + comp
}

pub fn find_crap(graph: &ProjectGraph, coverage: &CoverageMap) -> Vec<Finding> {
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
        if score <= CRAP_THRESHOLD {
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
            id: finding_id("crap", facet, path, &qualified, ""),
            category: "crap".to_string(),
            group: "risk".to_string(),
            subject_kind: facet.to_string(),
            severity: Severity::Warning,
            confidence: Confidence::Certain,
            message: format!(
                "{path}#{qualified} has CRAP {score:.0} (complexity {comp}, {coverage_text}) — \
                 above the threshold of {CRAP_THRESHOLD:.0}",
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
    findings
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
        let findings = find_crap(&graph, &CoverageMap::default());
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
        let findings = find_crap(&graph, &CoverageMap::default());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "crap");
        assert_eq!(findings[0].group, "risk");
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
        let findings = find_crap(&graph, &sink.into_map());
        assert!(findings.is_empty(), "fully covered: CRAP = comp = 6");
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
        assert_eq!(find_crap(&graph, &CoverageMap::default()).len(), 1);
        let mut sink = CoverageSink::default();
        for line in 1..=20 {
            sink.add_line(
                ProjectPath(SmolStr::new("src/a.mock")),
                line,
                u64::from(line <= 10),
            );
        }
        let findings = find_crap(&graph, &sink.into_map());
        assert!(findings.is_empty());
    }

    #[test]
    fn simple_functions_never_fire_even_uncovered() {
        // comp 5 uncovered: 25 + 5 = 30 — exactly at the threshold, and the RFC says "> 30".
        let graph = graph_with(
            vec![file(
                "src/a.mock",
                FileRole::Production,
                FileOrigin::Authored,
            )],
            vec![symbol(FileId(0), "plain", 1, 10)],
            vec![(SymbolId(0), metrics(5))],
        );
        assert!(find_crap(&graph, &CoverageMap::default()).is_empty());
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
        assert!(find_crap(&graph, &CoverageMap::default()).is_empty());
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
        let a = find_crap(&make(1), &CoverageMap::default());
        let b = find_crap(&make(50), &CoverageMap::default());
        assert_eq!(
            a[0].id, b[0].id,
            "moving the function must not change the id"
        );
    }
}
