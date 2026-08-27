//! `untested` — static test-blind spots: "the exact inverse of `test-only`,
//! computed from the same coloring passes at zero extra cost." Fires on nodes colored
//! [`Reachability::Production`] that are *also* unreached by every test root, at every
//! confidence tier — not "low coverage" (needs a report) but "no test even imports this,
//! transitively."
//!
//! `test-only`'s color already tells you "reachable only from tests," but a node's color is a
//! *precedence* verdict (Production beats TestOnly) — a Production node may or may not also be
//! test-reachable, and the color alone can't distinguish those. That's exactly what
//! [`ReachabilityMap::reachable_from`] answers directly, independent of which color won.
//!
//! Same exemptions as `test-only` (test-role files, generated/vendored origin) plus one of its
//! own: "active only when the project has test roots at all — a repo without tests gets one
//! diagnostic, not a thousand findings" — every Production node would otherwise be
//! test-unreached by definition, which is true but useless. That gate is checked once, up
//! front, not per node.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::adapter::{Diagnostic, DiagnosticLevel};
use crate::analysis::reachability::{Reachability, ReachabilityMap};
use crate::analysis::rollup;
use crate::analysis::{finding_id, FindingIdParts};
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::{
    Category, Confidence, EdgeKind, FileId, FileOrigin, FileRole, Group, NodeRef, PackageId,
    RootKind, SubjectKind, SymbolId,
};

/// Findings plus, when the project has no test roots at all, an abstention
/// instead of a false positive per production node.
pub fn find_untested(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
) -> (Vec<Finding>, super::Verdict) {
    let has_test_roots = graph.edges.iter().any(|e| {
        matches!(
            e.kind,
            EdgeKind::Root {
                kind: RootKind::Test,
                ..
            }
        )
    });
    if !has_test_roots {
        return (
            Vec::new(),
            crate::analysis::Verdict::Abstained(Diagnostic {
                level: DiagnosticLevel::Info,
                path: None,
                message: "untested: no test roots detected — skipped (a project with no tests \
                          has no test-blind-spot baseline to compare against)"
                    .to_string(),
                span: None,
            }),
        );
    }

    let mut findings = find_untested_files(graph, reach);
    findings.extend(find_untested_symbols(graph, reach));
    (findings, crate::analysis::Verdict::Judged)
}

fn is_untested_node(
    class_role: FileRole,
    class_origin: FileOrigin,
    reach: &ReachabilityMap,
    node: NodeRef,
) -> bool {
    if class_role == FileRole::Test
        || matches!(class_origin, FileOrigin::Generated | FileOrigin::Vendored)
    {
        return false;
    }
    reach.get(node).0 == Reachability::Production && !reach.reachable_from(RootKind::Test, node)
}

fn find_untested_files(graph: &ProjectGraph, reach: &ReachabilityMap) -> Vec<Finding> {
    let mut findings = Vec::new();
    let values_only = files_declaring_only_values(graph);
    let mut untested: HashMap<&str, (FileId, PackageId, Confidence)> = HashMap::default();
    for (index, file) in graph.files.iter().enumerate() {
        let Some(class) = file.class else {
            continue; // unclaimed — out of scope, not a verdict
        };
        let file_id = FileId(index as u32);
        if !is_untested_node(class.role, class.origin, reach, NodeRef::File(file_id)) {
            continue;
        }
        // A file that declares only values is not an untested file — it is a file the question
        // does not apply to. Derived from what the file DECLARES, so it needs no per-language
        // opt-out and stays right per file: a `.scss` carrying a `@function` (Sass has
        // unit-testing tooling) is still in scope, which a "SCSS is not testable" flag would
        // have silenced.
        if values_only.contains(&file_id) {
            continue;
        }
        let confidence = reach.get(NodeRef::File(file_id)).1;
        untested.insert(file.path.0.as_str(), (file_id, file.package, confidence));
    }

    let eligible: HashMap<&str, (FileId, PackageId)> = untested
        .iter()
        .map(|(&path, &(id, package, _))| (path, (id, package)))
        .collect();
    let rolled_up = rollup::directory_rollups(graph, &eligible);
    for dir in &rolled_up.dirs {
        let confidence = dir
            .files
            .iter()
            .filter_map(|f| untested.get(f).map(|&(_, _, c)| c))
            .min()
            .unwrap_or(Confidence::Possible);
        findings.push(rollup::directory_finding(
            graph,
            dir,
            rollup::DirVerdict {
                category: Category::UNTESTED,
                group: Group::Risk,
                severity: Severity::Info,
                confidence,
            },
            format!(
                "{} is production-reachable but no test reaches it: {} files",
                dir.display(),
                dir.files.len()
            ),
        ));
    }
    for (&path, &(_, package, confidence)) in &untested {
        if rolled_up.covered.contains(path) {
            continue;
        }
        findings.push(Finding {
            advisory: false,
            id: finding_id(FindingIdParts {
                category: &Category::UNTESTED,
                subject_kind: &SubjectKind::FILE,
                path,
                symbol_path: "",
                discriminator: "",
            }),
            category: Category::UNTESTED,
            group: Group::Risk,
            subject_kind: SubjectKind::FILE,
            severity: Severity::Info, // info by default
            confidence,
            message: format!("{path} is production-reachable but no test reaches it"),
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

/// Is this kind something a person writes a test FOR?
///
/// `untested` asks "does a test exercise this", and for a value there is nothing to exercise:
/// `Scheme.https`, `Genre.HORROR`, `MAX_VARCHAR_LENGTH`, `--bs-gray-600`. Nobody opens a pull
/// request titled "add a test for a header-name constant", and 457 of these — 17% of every
/// `untested` finding across the field corpus, 56% of one project's — were exactly that.
///
/// `TypeAlias` was already excluded here, with this same reasoning stated in place ("no runtime
/// footprint — `type Output = Stats` can never be 'covered'"). This generalizes that one
/// carve-out into the rule it always was.
///
/// A DENYLIST, not an allowlist, and deliberately: `Other(name)` is adapter-defined vocabulary
/// — Kotlin's `object` arrives that way, and an object is a type someone tests — so guessing
/// about kinds that do not exist yet would silence them. Types (`Class`/`Interface`/`Struct`/
/// `Enum`) stay too: a person does write a test for a type.
///
/// This is only correct because a computed property is no longer a `Field`. While a stored
/// constant and a getter-with-a-body shared one kind, excluding `Field` would have taken real
/// logic with it.
fn is_a_unit_of_testing(kind: &crate::vocab::SymbolKind) -> bool {
    use crate::vocab::SymbolKind as K;
    !matches!(
        kind,
        K::TypeAlias
            | K::EnumMember
            | K::Const
            | K::Static
            | K::Variable
            | K::Field
            | K::CssRule
            | K::CssVariable
    )
}

/// Symbols the per-symbol pass never flags: values, which are not units of testing
/// ([`is_a_unit_of_testing`]), whole-file rollups, and anything not untested itself.
fn symbol_skipped(
    symbol: &crate::graph::SymbolNode,
    class: crate::vocab::FileClass,
    reach: &ReachabilityMap,
    symbol_id: SymbolId,
) -> bool {
    !is_a_unit_of_testing(&symbol.kind)
        || is_untested_node(class.role, class.origin, reach, NodeRef::File(symbol.file))
        || !is_untested_node(class.role, class.origin, reach, NodeRef::Symbol(symbol_id))
}

/// Files that declare symbols and NOT ONE of them is a unit of testing — a declarative
/// stylesheet, a JSON document, a Markdown page.
///
/// "Declares symbols" is required, not incidental: a file the adapter extracted nothing from
/// says nothing about what it contains, and concluding "nothing to test" from an absence of
/// evidence would silence it for a reason nobody could see. Only a file that demonstrably
/// declares values and only values is exempt.
///
/// One pass over the symbol table rather than a scan per file — `find_untested_files` asks this
/// for every file, and the two together would be quadratic.
fn files_declaring_only_values(graph: &ProjectGraph) -> HashSet<FileId> {
    let mut declares_anything: HashSet<FileId> = HashSet::default();
    let mut declares_a_unit: HashSet<FileId> = HashSet::default();
    for symbol in &graph.symbols {
        declares_anything.insert(symbol.file);
        if is_a_unit_of_testing(&symbol.kind) {
            declares_a_unit.insert(symbol.file);
        }
    }
    declares_anything
        .into_iter()
        .filter(|f| !declares_a_unit.contains(f))
        .collect()
}

fn find_untested_symbols(graph: &ProjectGraph, reach: &ReachabilityMap) -> Vec<Finding> {
    let mut findings = Vec::new();
    let test_roots = crate::analysis::test_root_symbols(graph);
    for (index, symbol) in graph.symbols.iter().enumerate() {
        if test_roots.contains(&SymbolId(index as u32)) {
            continue; // inline test infrastructure (see analysis::test_root_symbols)
        }
        let file = &graph.files[symbol.file.0 as usize];
        let Some(class) = file.class else { continue };
        let symbol_id = SymbolId(index as u32);
        if symbol_skipped(symbol, class, reach, symbol_id) {
            continue;
        }

        let path = file.path.0.as_str();
        let facet = symbol.kind.facet();
        let qualified = symbol.qualified_name();
        let confidence = reach.get(NodeRef::Symbol(symbol_id)).1;
        findings.push(Finding {
            advisory: false,
            id: finding_id(FindingIdParts {
                category: &Category::UNTESTED,
                subject_kind: &SubjectKind::new(facet),
                path,
                symbol_path: &qualified,
                discriminator: "",
            }),
            category: Category::UNTESTED,
            group: Group::Risk,
            subject_kind: SubjectKind::new(facet),
            severity: Severity::Info,
            confidence,
            message: format!(
                "{path}#{qualified} is production-reachable but no test reaches this {facet}"
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
    use crate::vocab::{Edge, FileClass, Provenance, RefKind, SymbolKind};
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
            unit_parent: None,
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
            nested_scope: false,
            visibility_inherited: false,
            visible_in_unit: None,
            implements: None,
            markers: Vec::new(),
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
    fn no_test_roots_at_all_yields_one_diagnostic_and_no_findings() {
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
        let (findings, verdict) = find_untested(&graph, &reach);
        assert!(findings.is_empty());
        let crate::analysis::Verdict::Abstained(d) = verdict else {
            panic!("no test roots ⇒ untested must abstain, not report a clean verdict");
        };
        assert_eq!(d.level, DiagnosticLevel::Info);
    }

    fn kinded(file: FileId, name: &str, kind: SymbolKind) -> SymbolNode {
        SymbolNode {
            kind,
            ..symbol(file, name)
        }
    }

    /// The per-SYMBOL shape (modelled on
    /// `symbol_untested_but_file_has_some_tested_symbols_is_reported_individually`): the
    /// declaring file IS test-reachable, so no whole-file rollup swallows its symbols, while
    /// each symbol is referenced only from a production-only file the test side never reaches.
    fn untested_project(symbols: Vec<SymbolNode>) -> ProjectGraph {
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("src/caller.mock", FileRole::Production),
            file("src/main.mock", FileRole::Production),
        ];
        let mut edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(1)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(2)),
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
                EdgeKind::ImportsFile {
                    from: FileId(0),
                    to: FileId(2),
                },
                Confidence::Certain,
            ),
        ];
        for i in 0..symbols.len() {
            edges.push(edge(
                EdgeKind::References {
                    from: NodeRef::File(FileId(1)),
                    to: SymbolId(i as u32),
                    kind: RefKind::Read,
                },
                Confidence::Certain,
            ));
        }
        ProjectGraph::for_test(files, symbols, vec![], edges)
    }

    /// Symbol-level findings only — `src/caller.mock` draws its own file-level verdict here,
    /// which is a separate, real one and not what these tests are about.
    fn untested_symbols(graph: &ProjectGraph) -> Vec<String> {
        let reach = reachability::compute(graph);
        let mut names: Vec<String> = find_untested(graph, &reach)
            .0
            .iter()
            .filter(|f| f.subject_kind != "file" && f.subject_kind != "directory")
            .filter_map(|f| f.location.symbol.clone())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_value_is_not_a_unit_of_testing() {
        // 457 findings across the field corpus — 17% of every `untested`, 56% of one project's
        // — were `Scheme.https`, `Genre.HORROR`, `MAX_VARCHAR_LENGTH`. Nobody writes a test for
        // a constant. `TypeAlias` was already excluded here for exactly this reason; the rest
        // of the values join it.
        let graph = untested_project(vec![
            kinded(FileId(2), "compute", SymbolKind::Function),
            kinded(FileId(2), "Widget", SymbolKind::Class),
            kinded(FileId(2), "MAX_LEN", SymbolKind::Const),
            kinded(FileId(2), "https", SymbolKind::Field),
            kinded(FileId(2), "HORROR", SymbolKind::EnumMember),
            kinded(FileId(2), "accent", SymbolKind::CssVariable),
        ]);
        assert_eq!(
            untested_symbols(&graph),
            vec!["Widget", "compute"],
            "a callable and a type are units of testing; a value is not"
        );
    }

    #[test]
    fn an_adapter_defined_kind_is_still_a_unit_of_testing() {
        // `Other` is adapter vocabulary — Kotlin's `object` arrives that way and IS a type
        // someone tests. The rule is a denylist precisely so kinds that don't exist yet are
        // not silenced by a guess.
        let graph = untested_project(vec![kinded(
            FileId(2),
            "Registry",
            SymbolKind::Other(SmolStr::new("object")),
        )]);
        assert_eq!(untested_symbols(&graph), vec!["Registry"]);
    }

    #[test]
    fn a_file_declaring_only_values_is_not_an_untested_file() {
        // The `.scss`/`.json`/`.md` case, derived rather than declared: the verdict would be
        // true and useless. A stylesheet carrying a Sass `@function` — a real testing unit —
        // stays in scope, which a per-language "not testable" flag would have silenced.
        // `src/caller.mock` declares nothing and stays eligible — an adapter that extracted
        // nothing says nothing about what the file contains, so absence of evidence must not
        // silence it. `src/main.mock` declares values and only values, and drops out.
        let values = untested_project(vec![
            kinded(FileId(2), "accent", SymbolKind::CssVariable),
            kinded(FileId(2), "card", SymbolKind::CssRule),
        ]);
        let reach = reachability::compute(&values);
        let value_findings = find_untested(&values, &reach).0;
        let paths: Vec<&str> = value_findings
            .iter()
            .filter_map(|f| f.location.path.as_ref().map(|p| p.0.as_str()))
            .collect();
        assert_eq!(paths, vec!["src/caller.mock"], "main.mock is out of scope");
        assert!(untested_symbols(&values).is_empty());

        // The same file carrying a Sass-style `@function` is a testing unit again.
        let with_function = untested_project(vec![
            kinded(FileId(2), "accent", SymbolKind::CssVariable),
            kinded(FileId(2), "double", SymbolKind::Function),
        ]);
        assert_eq!(untested_symbols(&with_function), vec!["double"]);
    }

    #[test]
    fn tooling_only_files_are_not_untested() {
        // The root-kind cap (graph phase 2.58) turns a tooling bin's Production roots into
        // Tooling ones — this analysis fires only on production-reachable nodes, so a
        // tooling-role entry point (xtask) never draws "production-reachable but no test
        // reaches it": the statement would be false. A Test root elsewhere keeps the
        // no-test-roots honesty gate from trivially skipping the whole analysis.
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("xtask/main.mock", FileRole::Tooling),
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
                    kind: RootKind::Tooling,
                    target: NodeRef::File(FileId(1)),
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        let (findings, verdict) = find_untested(&graph, &reach);
        assert!(matches!(verdict, crate::analysis::Verdict::Judged));
        assert!(
            findings.is_empty(),
            "a ToolingOnly file is not a test blind spot: {findings:?}"
        );
    }

    #[test]
    fn production_file_reached_by_no_test_is_untested() {
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("src/live.mock", FileRole::Production), // its own production root, and test-imported
            file("src/blind.mock", FileRole::Production), // a disjoint production-only root
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
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(2)),
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
        let (findings, verdict) = find_untested(&graph, &reach);
        assert!(matches!(verdict, crate::analysis::Verdict::Judged));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "untested");
        assert_eq!(findings[0].group, crate::vocab::Group::Risk);
        assert!(findings[0].message.contains("src/blind.mock"));
    }

    #[test]
    fn production_file_also_reached_by_a_test_is_not_untested() {
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("src/covered.mock", FileRole::Production),
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
                    to: FileId(1),
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        let (findings, _) = find_untested(&graph, &reach);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_only_file_is_not_untested_its_not_production_at_all() {
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
        let (findings, _) = find_untested(&graph, &reach);
        assert!(findings.is_empty());
    }

    #[test]
    fn unreachable_file_is_not_untested() {
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("src/orphan.mock", FileRole::Production),
        ];
        let edges = vec![edge(
            EdgeKind::Root {
                kind: RootKind::Test,
                target: NodeRef::File(FileId(0)),
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        let (findings, _) = find_untested(&graph, &reach);
        assert!(findings.is_empty());
    }

    #[test]
    fn generated_and_vendored_are_exempt() {
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("src/live.mock", FileRole::Production),
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
                unit_parent: None,
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
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(1)),
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
        let (findings, _) = find_untested(&graph, &reach);
        assert!(findings.is_empty());
    }

    #[test]
    fn a_fully_untested_directory_rolls_up_to_one_finding() {
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("src/live.mock", FileRole::Production), // test-reached — blocks src/ from rolling up
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
                    to: FileId(1),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(2)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(3)),
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        let (findings, _) = find_untested(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].subject_kind, "directory");
        assert_eq!(findings[0].location.path.as_ref().unwrap().0, "src/legacy");
    }

    #[test]
    fn symbol_untested_but_file_has_some_tested_symbols_is_reported_individually() {
        // main.mock (file 2) is production-reachable AND test-reachable overall (the test root
        // imports it directly), so it's not itself an untested file. But `blindSpot`, declared
        // in main.mock, is referenced only from caller.mock (file 1) — a production-only file
        // the test side never reaches — so the symbol itself never gets a test-reachable path,
        // even though its containing file does. The file-level finding can't cover that.
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("src/caller.mock", FileRole::Production),
            file("src/main.mock", FileRole::Production),
        ];
        let symbols = vec![symbol(FileId(2), "blindSpot")];
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
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(2)),
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
                EdgeKind::ImportsFile {
                    from: FileId(0),
                    to: FileId(2),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::File(FileId(1)),
                    to: SymbolId(0),
                    kind: RefKind::Read,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = reachability::compute(&graph);
        let (findings, _) = find_untested(&graph, &reach);
        // caller.mock is untested too (a real, separate verdict — the test side never reaches
        // it at all) — the point under test is that main.mock's *file*-level finding doesn't
        // fire (it's test-reachable) while its symbol-level one does (the reference site isn't).
        // (A symbol's own finding also carries its declaring file's path, so filter by subject
        // kind, not path, to tell the two apart.)
        assert!(!findings
            .iter()
            .any(|f| f.subject_kind == "file"
                && f.location.path.as_ref().unwrap().0 == "src/main.mock"));
        let symbol_finding = findings
            .iter()
            .find(|f| f.subject_kind == "function")
            .expect("blindSpot should be reported individually");
        assert!(symbol_finding.message.contains("main.mock#blindSpot"));
    }

    #[test]
    fn a_test_invoking_the_binary_clears_the_bins_untested_findings() {
        // The e2e shape (the invoked-program rule): the test never imports the
        // bin — it executes it. The InvokesFile edge reaches the bin's Production root and
        // its call tree, so neither the file nor `main`/`helper` are test-blind spots.
        let files = vec![
            file("tests/e2e.test.mock", FileRole::Test),
            file("src/main.mock", FileRole::Production),
        ];
        let symbols = vec![symbol(FileId(1), "main"), symbol(FileId(1), "helper")];
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
                    target: NodeRef::Symbol(SymbolId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::InvokesFile {
                    from: NodeRef::File(FileId(0)),
                    to: FileId(1),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::Symbol(SymbolId(0)),
                    to: SymbolId(1),
                    kind: RefKind::Call,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = reachability::compute(&graph);
        let (findings, _) = find_untested(&graph, &reach);
        assert!(
            findings.is_empty(),
            "unexpected findings: {:?}",
            findings.iter().map(|f| &f.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_machinery_hook_on_a_test_reached_type_is_not_untested() {
        // `LoadError.fmt` shape: the impl method is Production-rooted (dispatch rule) but no
        // test ever writes `.fmt(` — the machinery-dispatch rule lets it inherit the
        // owner's test-reachability instead of false-positiving as a blind spot.
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("src/error.mock", FileRole::Production),
        ];
        let symbols = vec![
            symbol(FileId(1), "LoadError"),
            SymbolNode {
                member_of: Some(SmolStr::new("LoadError")),
                implicitly_invoked: true,
                nested_scope: false,
                visibility_inherited: false,
                visible_in_unit: None,
                implements: None,
                markers: Vec::new(),
                ..symbol(FileId(1), "fmt")
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
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::Symbol(SymbolId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::Symbol(SymbolId(1)),
                },
                Confidence::Probable,
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
        let (findings, _) = find_untested(&graph, &reach);
        assert!(
            !findings.iter().any(|f| f.message.contains("fmt")),
            "unexpected: {:?}",
            findings.iter().map(|f| &f.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn finding_id_is_stable_across_runs() {
        let files = vec![
            file("tests/spec.test.mock", FileRole::Test),
            file("src/live.mock", FileRole::Production),
            file("src/blind.mock", FileRole::Production),
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
                    from: FileId(1),
                    to: FileId(2),
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        let a = find_untested(&graph, &reach).0;
        let b = find_untested(&graph, &reach).0;
        assert_eq!(a[0].id, b[0].id);
    }
}
