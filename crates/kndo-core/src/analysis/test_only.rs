//! `test-only` — non-productive code (RFC 0005 §3, §4): "you built it, tests enshrined it,
//! production never came." Fires on nodes colored [`Reachability::TestOnly`] — reachable, just
//! never from a production or tooling root — **excluding test-role files themselves**: a test
//! being test-only is trivially true (that's what a test is), not a finding. "Declared test
//! utilities (`testkit`/`fixtures` conventions, configurable)" is RFC 0005 §3's other stated
//! exemption; there's no config system yet to make it configurable, so it's not attempted here
//! rather than hand-picking a convention no config can override.
//!
//! Unlike `unused` ("dead is always certain"), `test-only` findings inherit whatever confidence
//! their evidence carries (RFC 0005 §1: a node reachable only through a `probable` edge from a
//! test root is test-only-*probable*) — the symbol/file loops below report the reachability
//! map's own confidence verbatim, and directory rollup takes the *weakest* confidence among a
//! group's files (a group's claim can never be stronger than its least-certain member).
//!
//! Directory rollup reuses [`crate::analysis::rollup`] — same mechanism `unused` uses, not a
//! reimplementation: what's "eligible" differs (`TestOnly` color vs. `Unreachable`), how
//! eligible files fold into directories does not.

use rustc_hash::FxHashMap as HashMap;

use crate::analysis::finding_id;
use crate::analysis::reachability::{Reachability, ReachabilityMap};
use crate::analysis::rollup::{self, DirGroup};
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::{Confidence, FileId, FileOrigin, FileRole, NodeRef, PackageId, SymbolId};

pub fn find_test_only_files(graph: &ProjectGraph, reach: &ReachabilityMap) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut test_only: HashMap<&str, (FileId, PackageId, Confidence)> = HashMap::default();
    for (index, file) in graph.files.iter().enumerate() {
        let Some(class) = file.class else {
            continue; // unclaimed — out of scope, not a verdict
        };
        if matches!(class.origin, FileOrigin::Generated | FileOrigin::Vendored) {
            continue;
        }
        if class.role == FileRole::Test {
            continue; // the exemption RFC 0005 §3 states explicitly
        }

        let file_id = FileId(index as u32);
        let (color, confidence) = reach.get(NodeRef::File(file_id));
        if color != Reachability::TestOnly {
            continue;
        }
        test_only.insert(file.path.0.as_str(), (file_id, file.package, confidence));
    }

    let eligible: HashMap<&str, (FileId, PackageId)> = test_only
        .iter()
        .map(|(&path, &(id, package, _))| (path, (id, package)))
        .collect();
    let rolled_up = rollup::directory_rollups(graph, &eligible);
    for dir in &rolled_up.dirs {
        let confidence = dir
            .files
            .iter()
            .filter_map(|f| test_only.get(f).map(|&(_, _, c)| c))
            .min()
            .unwrap_or(Confidence::Possible);
        findings.push(directory_finding(graph, dir, confidence));
    }
    for (&path, &(_, package, confidence)) in &test_only {
        if rolled_up.covered.contains(path) {
            continue;
        }
        findings.push(Finding {
            advisory: false,
            id: finding_id("test-only", "file", path, "", ""),
            category: "test-only".to_string(),
            group: "waste".to_string(),
            subject_kind: "file".to_string(),
            severity: Severity::Info, // RFC 0005 §3: info by default
            confidence,
            message: format!("{path} is reachable only from tests: production never calls it"),
            location: Location {
                path: Some(crate::adapter::ProjectPath(smol_str::SmolStr::new(path))),
                range: None,
                symbol: None,
                package: graph.package_name(package).map(str::to_string),
            },
            related: Vec::new(),
            delta: None,
            delta_origin: None,
        });
    }
    findings
}

fn directory_finding(graph: &ProjectGraph, dir: &DirGroup<'_>, confidence: Confidence) -> Finding {
    let display = if dir.path.is_empty() { "." } else { dir.path };
    Finding {
        advisory: false,
        id: finding_id("test-only", "directory", dir.path, "", ""),
        category: "test-only".to_string(),
        group: "waste".to_string(),
        subject_kind: "directory".to_string(),
        severity: Severity::Info,
        confidence,
        message: format!(
            "{display} is reachable only from tests: {} files, production never calls them",
            dir.files.len()
        ),
        location: Location {
            path: Some(crate::adapter::ProjectPath(smol_str::SmolStr::new(
                dir.path,
            ))),
            range: None,
            symbol: None,
            package: graph.package_name(dir.package).map(str::to_string),
        },
        related: Vec::new(),
        delta: None,
        delta_origin: None,
    }
}

pub fn find_test_only_symbols(graph: &ProjectGraph, reach: &ReachabilityMap) -> Vec<Finding> {
    let mut findings = Vec::new();
    let test_roots = crate::analysis::test_root_symbols(graph);
    for (index, symbol) in graph.symbols.iter().enumerate() {
        let file = &graph.files[symbol.file.0 as usize];
        let Some(class) = file.class else { continue };
        if matches!(class.origin, FileOrigin::Generated | FileOrigin::Vendored) {
            continue;
        }
        if class.role == FileRole::Test {
            continue; // the file itself is exempt — so are its own symbols
        }
        if test_roots.contains(&SymbolId(index as u32)) {
            continue; // inline test infrastructure IS a test, not test-only production code
        }
        if reach.get(NodeRef::File(symbol.file)).0 == Reachability::TestOnly {
            continue; // rollup: the file-level finding already covers every symbol in it
        }

        let symbol_id = SymbolId(index as u32);
        let (color, confidence) = reach.get(NodeRef::Symbol(symbol_id));
        if color != Reachability::TestOnly {
            continue;
        }

        let path = file.path.0.as_str();
        let facet = symbol.kind.facet();
        let qualified = symbol.qualified_name();
        findings.push(Finding {
            advisory: false,
            id: finding_id("test-only", facet, path, &qualified, ""),
            category: "test-only".to_string(),
            group: "waste".to_string(),
            subject_kind: facet.to_string(),
            severity: Severity::Info,
            confidence,
            message: format!(
                "{path}#{qualified} is reachable only from tests: nothing in production calls this {facet}"
            ),
            location: Location {
                path: Some(file.path.clone()),
                range: Some(symbol.span),
                symbol: Some(qualified.clone()),
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
    use crate::adapter::{ProjectPath, VisibilityLevel};
    use crate::analysis::reachability;
    use crate::graph::{FileNode, SymbolNode};
    use crate::vocab::{Edge, EdgeKind, FileClass, Provenance, RefKind, RootKind, SymbolKind};
    use smol_str::SmolStr;

    fn file(path: &str, role: FileRole) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: Some(SmolStr::new("mock")),
            class: Some(FileClass {
                role,
                origin: FileOrigin::Authored,
            }),
            package: crate::vocab::PackageId(0),
            unit: None,
            test_spans: Vec::new(),
            string_call_sites: Vec::new(),
        }
    }

    fn symbol(file: FileId, name: &str) -> SymbolNode {
        SymbolNode {
            file,
            name: SmolStr::new(name),
            kind: SymbolKind::Function,
            span: Default::default(),
            exported: true,
            visibility: VisibilityLevel(1),
            member_of: None,
            signature_span: None,
            implicitly_invoked: false,
        }
    }

    fn edge(kind: EdgeKind, confidence: Confidence) -> Edge {
        Edge {
            owner: crate::vocab::FileId(0),
            kind,
            confidence,
            source: Provenance::Adapter(SmolStr::new("mock")),
            span: None,
        }
    }

    #[test]
    fn file_reached_only_by_a_test_root_is_test_only() {
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("src/helper.mock", FileRole::Production),
        ];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Test,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::ImportsFile {
                    from: FileId(0),
                    to: FileId(1),
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        let findings = find_test_only_files(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "test-only");
        assert_eq!(findings[0].group, "waste");
        assert_eq!(findings[0].severity, Severity::Info);
        assert_eq!(findings[0].subject_kind, "file");
        assert!(findings[0].message.contains("src/helper.mock"));
    }

    #[test]
    fn the_test_root_file_itself_is_never_reported() {
        let files = vec![file("tests/spec.test.mock", FileRole::Test)];
        let edges = vec![edge(
            EdgeKind::Root {
                kind: RootKind::Test,
                target: NodeRef::File(FileId(0)),
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        assert!(find_test_only_files(&graph, &reach).is_empty());
    }

    #[test]
    fn production_reachable_file_is_not_test_only() {
        let files = vec![file("src/lib.mock", FileRole::Production)];
        let edges = vec![edge(
            EdgeKind::Root {
                kind: RootKind::Production,
                target: NodeRef::File(FileId(0)),
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        assert!(find_test_only_files(&graph, &reach).is_empty());
    }

    #[test]
    fn unreachable_file_is_not_test_only() {
        // Unreachable, not test-only — a different verdict entirely (`unused`'s job).
        let files = vec![file("src/orphan.mock", FileRole::Production)];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = reachability::compute(&graph);
        assert!(find_test_only_files(&graph, &reach).is_empty());
    }

    #[test]
    fn confidence_reflects_the_weakest_reaching_edge() {
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("src/helper.mock", FileRole::Production),
        ];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Test,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::ImportsFile {
                    from: FileId(0),
                    to: FileId(1),
                },
                Confidence::Probable,
            ),
        ];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        let findings = find_test_only_files(&graph, &reach);
        assert_eq!(findings[0].confidence, Confidence::Probable);
    }

    #[test]
    fn generated_and_vendored_are_exempt() {
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            FileNode {
                path: ProjectPath(SmolStr::new("dist/bundle.mock")),
                content_hash: [0; 32],
                language: Some(SmolStr::new("mock")),
                class: Some(FileClass {
                    role: FileRole::Production,
                    origin: FileOrigin::Generated,
                }),
                package: crate::vocab::PackageId(0),
                unit: None,
                test_spans: Vec::new(),
                string_call_sites: Vec::new(),
            },
        ];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Test,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::ImportsFile {
                    from: FileId(0),
                    to: FileId(1),
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        assert!(find_test_only_files(&graph, &reach).is_empty());
    }

    #[test]
    fn finding_id_is_stable_across_runs() {
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("src/helper.mock", FileRole::Production),
        ];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Test,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::ImportsFile {
                    from: FileId(0),
                    to: FileId(1),
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        let a = find_test_only_files(&graph, &reach);
        let b = find_test_only_files(&graph, &reach);
        assert_eq!(a[0].id, b[0].id);
    }

    // ---------------------------------------------------------------- directory rollup

    #[test]
    fn a_fully_test_only_directory_rolls_up_to_one_finding() {
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("src/live.mock", FileRole::Production), // blocks src/ itself from rolling up
            file("src/legacy/a.mock", FileRole::Production),
            file("src/legacy/b.mock", FileRole::Production),
        ];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Test,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(1)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::ImportsFile {
                    from: FileId(0),
                    to: FileId(2),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::ImportsFile {
                    from: FileId(0),
                    to: FileId(3),
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        let findings = find_test_only_files(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].subject_kind, "directory");
        assert_eq!(findings[0].location.path.as_ref().unwrap().0, "src/legacy");
    }

    // ---------------------------------------------------------------- symbols

    #[test]
    fn symbol_reached_only_from_a_test_is_test_only() {
        // main.mock is production-reachable overall, but `helper` is called only from the
        // test — the RFC 0005 §3 "enshrined by tests" case at symbol granularity.
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("src/main.mock", FileRole::Production),
        ];
        let symbols = vec![symbol(FileId(1), "helper")];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(1)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::Root {
                    kind: RootKind::Test,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::File(FileId(0)),
                    to: SymbolId(0),
                    kind: RefKind::Read,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = reachability::compute(&graph);
        let findings = find_test_only_symbols(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "test-only");
        assert_eq!(findings[0].subject_kind, "function");
        assert!(findings[0].message.contains("main.mock#helper"));
    }

    #[test]
    fn symbol_in_an_already_test_only_file_is_not_double_reported() {
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("src/helper.mock", FileRole::Production),
        ];
        let symbols = vec![symbol(FileId(1), "fn")];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Test,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::ImportsFile {
                    from: FileId(0),
                    to: FileId(1),
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = reachability::compute(&graph);
        assert!(find_test_only_symbols(&graph, &reach).is_empty());
        assert_eq!(find_test_only_files(&graph, &reach).len(), 1);
    }

    #[test]
    fn symbols_declared_in_a_test_file_are_exempt_too() {
        let files = vec![file("tests/spec.test.mock", FileRole::Test)];
        let symbols = vec![symbol(FileId(0), "fixtureHelper")];
        let edges = vec![edge(
            EdgeKind::Root {
                kind: RootKind::Test,
                target: NodeRef::File(FileId(0)),
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = reachability::compute(&graph);
        assert!(find_test_only_symbols(&graph, &reach).is_empty());
    }
}
