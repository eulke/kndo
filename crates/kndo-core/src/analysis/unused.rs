//! `unused` — unreachable files and symbols.
//!
//! Symbol-level findings only fire for symbols in files granularity has already ruled *in*
//! scope: a symbol whose owning file is itself `unreachable` is skipped, because the file-level
//! finding already covers it (taxonomy rollup rule — "the file finding replaces the per-symbol
//! findings it summarizes"; reporting both would be redundant, not more informative). Reference
//! evidence is symbol-attributed where the adapter supplies `within` (a dead
//! function's calls keep nothing alive, so transitive death lands here as ordinary `unused`
//! findings) and file-attributed otherwise; either way a symbol is `unreachable` only when
//! *no reachable code anywhere* references it.
//!
//! Directory rollup (taxonomy rule 3: "a directory whose every file carries the same verdict
//! rolls up once more — the widest uniform node gets one finding, not fifty") is implemented
//! for files: when *every* file anywhere under a directory (recursively, including any nested
//! subdirectories) is unused, that whole directory gets one `unused:directory` finding instead
//! of one per file, and the individual file findings it summarizes are dropped. A single
//! non-unused file anywhere in the subtree blocks the rollup for every one of its ancestors —
//! this includes files merely out-of-scope (unclaimed, generated, vendored), not just
//! genuinely-reachable ones: a directory containing so much as a vendored README isn't safe to
//! claim as "delete this whole folder." A nested package manifest structurally can never be
//! `unused` (manifests are unclaimed, never eligible — see below), so rollup can never
//! silently cross a package boundary either.

use rustc_hash::FxHashMap as HashMap;

use crate::analysis::finding_id;
use crate::analysis::reachability::{Reachability, ReachabilityMap};
use crate::analysis::rollup::{self, DirGroup};
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::{Confidence, FileId, FileOrigin, NodeRef, PackageId, SymbolId};

pub fn find_unused_files(graph: &ProjectGraph, reach: &ReachabilityMap) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut unused: HashMap<&str, (FileId, PackageId)> = HashMap::default();
    for (index, file) in graph.files.iter().enumerate() {
        // Unclaimed: no adapter recognized this file, so no adapter has an opinion on whether
        // it can be a root or a target — out of scope, not a verdict.
        let Some(class) = file.class else {
            continue;
        };
        // Generated/vendored origins are exempt by default.
        if matches!(class.origin, FileOrigin::Generated | FileOrigin::Vendored) {
            continue;
        }

        let file_id = FileId(index as u32);
        let (color, confidence) = reach.get(NodeRef::File(file_id));
        // Rule 4: unreachable at every root kind and every confidence tier —
        // dead is always certain, so this is the only case `unused` ever fires for.
        if color != Reachability::Unreachable {
            continue;
        }
        debug_assert_eq!(confidence, Confidence::Certain);
        unused.insert(file.path.0.as_str(), (file_id, file.package));
    }

    let rolled_up = rollup::directory_rollups(graph, &unused);
    for dir in &rolled_up.dirs {
        findings.push(directory_finding(graph, dir));
    }
    for (&path, &(_, package)) in &unused {
        if rolled_up.covered.contains(path) {
            continue;
        }
        findings.push(Finding {
            advisory: false,
            id: finding_id("unused", "file", path, "", ""),
            category: "unused".to_string(),
            group: "waste".to_string(),
            subject_kind: "file".to_string(),
            severity: Severity::Warning,
            confidence: Confidence::Certain,
            message: format!("{path} is unreachable: no root or import reaches it"),
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

fn directory_finding(graph: &ProjectGraph, dir: &DirGroup<'_>) -> Finding {
    let display = if dir.path.is_empty() { "." } else { dir.path };
    Finding {
        advisory: false,
        id: finding_id("unused", "directory", dir.path, "", ""),
        category: "unused".to_string(),
        group: "waste".to_string(),
        subject_kind: "directory".to_string(),
        severity: Severity::Warning,
        confidence: Confidence::Certain,
        message: format!(
            "{display} is unreachable: {} files, none referenced — safe to delete the whole directory",
            dir.files.len()
        ),
        location: Location {
            path: Some(crate::adapter::ProjectPath(smol_str::SmolStr::new(dir.path))),
            range: None,
            symbol: None,
            package: graph.package_name(dir.package).map(str::to_string),
        },
        related: Vec::new(),
        delta: None,
        delta_origin: None,
    }
}

/// The per-symbol scope gate: symbols in unclaimed/generated/vendored files are out of
/// jurisdiction; symbols in unreachable files roll up to the file finding; constructors are
/// never accused directly — instantiation references the *type*, so a constructor's
/// unreachability is structurally unknowable and its liveness follows the class (whose own
/// finding/rollup covers real death).
fn symbol_in_scope(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    symbol: &crate::graph::SymbolNode,
) -> bool {
    let file = &graph.files[symbol.file.0 as usize];
    let Some(class) = file.class else {
        return false;
    };
    !matches!(class.origin, FileOrigin::Generated | FileOrigin::Vendored)
        && reach.get(NodeRef::File(symbol.file)).0 != Reachability::Unreachable
        && symbol.kind != crate::vocab::SymbolKind::Constructor
}

pub fn find_unused_symbols(graph: &ProjectGraph, reach: &ReachabilityMap) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (index, symbol) in graph.symbols.iter().enumerate() {
        let file = &graph.files[symbol.file.0 as usize];
        if !symbol_in_scope(graph, reach, symbol) {
            continue;
        }

        let symbol_id = SymbolId(index as u32);
        let (color, confidence) = reach.get(NodeRef::Symbol(symbol_id));
        if color != Reachability::Unreachable {
            continue;
        }
        debug_assert_eq!(confidence, Confidence::Certain);

        let path = file.path.0.as_str();
        let facet = symbol.kind.facet();
        let qualified = symbol.qualified_name();
        findings.push(Finding {
            advisory: false,
            id: finding_id("unused", facet, path, &qualified, ""),
            category: "unused".to_string(),
            group: "waste".to_string(),
            subject_kind: facet.to_string(),
            severity: Severity::Warning,
            confidence: Confidence::Certain,
            message: format!("{path}#{qualified} is unreachable: nothing references this {facet}"),
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
    use crate::adapter::ProjectPath;
    use crate::analysis::reachability;
    use crate::graph::FileNode;
    use crate::vocab::{Edge, EdgeKind, FileClass, FileRole, NodeRef, Provenance, RootKind};
    use smol_str::SmolStr;

    fn file(path: &str, class: Option<FileClass>) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: class.map(|_| SmolStr::new("mock")),
            class,
            package: crate::vocab::PackageId(0),
            unit: None,
            test_spans: Vec::new(),
            string_call_sites: Vec::new(),
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
    fn orphan_file_is_reported_unused() {
        let files = vec![file("orphan.ts", Some(FileClass::default()))];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = reachability::compute(&graph);
        let findings = find_unused_files(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "unused");
        assert_eq!(findings[0].subject_kind, "file");
        assert_eq!(findings[0].group, "waste");
        assert!(findings[0].message.contains("orphan.ts"));
    }

    #[test]
    fn root_file_is_not_reported() {
        let files = vec![file("main.ts", Some(FileClass::default()))];
        let edges = vec![edge(
            EdgeKind::Root {
                kind: RootKind::Production,
                target: NodeRef::File(FileId(0)),
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        assert!(find_unused_files(&graph, &reach).is_empty());
    }

    #[test]
    fn imported_file_is_not_reported() {
        let files = vec![
            file("main.ts", Some(FileClass::default())),
            file("lib.ts", Some(FileClass::default())),
        ];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
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
        assert!(find_unused_files(&graph, &reach).is_empty());
    }

    #[test]
    fn unclaimed_file_is_out_of_scope() {
        let files = vec![file("README.md", None)];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = reachability::compute(&graph);
        assert!(find_unused_files(&graph, &reach).is_empty());
    }

    #[test]
    fn generated_orphan_is_exempt() {
        let class = FileClass {
            role: FileRole::Production,
            origin: FileOrigin::Generated,
        };
        let files = vec![file("dist/bundle.js", Some(class))];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = reachability::compute(&graph);
        assert!(find_unused_files(&graph, &reach).is_empty());
    }

    #[test]
    fn vendored_orphan_is_exempt() {
        let class = FileClass {
            role: FileRole::Production,
            origin: FileOrigin::Vendored,
        };
        let files = vec![file("vendor/lib.js", Some(class))];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = reachability::compute(&graph);
        assert!(find_unused_files(&graph, &reach).is_empty());
    }

    #[test]
    fn finding_id_is_stable_across_runs() {
        let files = vec![file("orphan.ts", Some(FileClass::default()))];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = reachability::compute(&graph);
        let a = find_unused_files(&graph, &reach);
        let b = find_unused_files(&graph, &reach);
        assert_eq!(a[0].id, b[0].id);
    }

    // ---------------------------------------------------------------- directory rollup

    /// A live file at `src/main.ts` with a Production root — present in every rollup test below
    /// so `src/legacy` has some live sibling content both at the project root *and* inside
    /// `src` itself. Without it, `src/legacy` being "fully dead" also makes `src` (nothing
    /// else lives there in these tiny fixtures) and the project root fully dead, and the
    /// widest-rollup rule correctly (if unhelpfully, for testing one directory in isolation)
    /// climbs past the directory under test.
    fn with_live_root(mut files: Vec<FileNode>) -> (Vec<FileNode>, Vec<Edge>) {
        files.insert(0, file("src/main.ts", Some(FileClass::default())));
        let edges = vec![edge(
            EdgeKind::Root {
                kind: RootKind::Production,
                target: NodeRef::File(FileId(0)),
            },
            Confidence::Certain,
        )];
        (files, edges)
    }

    #[test]
    fn a_fully_dead_directory_rolls_up_to_one_finding() {
        let (files, edges) = with_live_root(vec![
            file("src/legacy/a.ts", Some(FileClass::default())),
            file("src/legacy/b.ts", Some(FileClass::default())),
            file("src/legacy/sub/c.ts", Some(FileClass::default())),
        ]);
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        let findings = find_unused_files(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].subject_kind, "directory");
        assert_eq!(findings[0].location.path.as_ref().unwrap().0, "src/legacy");
        assert!(findings[0].message.contains("3 files"));
    }

    #[test]
    fn rollup_picks_the_widest_qualifying_directory_not_every_level() {
        // Both src/legacy and src/legacy/sub are, on their own, "every file inside is dead" —
        // taxonomy rule 3 wants the widest one, one finding, not one per level.
        let (files, edges) = with_live_root(vec![
            file("src/legacy/a.ts", Some(FileClass::default())),
            file("src/legacy/sub/b.ts", Some(FileClass::default())),
            file("src/legacy/sub/c.ts", Some(FileClass::default())),
        ]);
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        let findings = find_unused_files(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].location.path.as_ref().unwrap().0, "src/legacy");
    }

    #[test]
    fn one_live_file_blocks_rollup_for_every_ancestor() {
        let files = vec![
            file("src/legacy/a.ts", Some(FileClass::default())),
            file("src/legacy/b.ts", Some(FileClass::default())),
        ];
        let edges = vec![edge(
            EdgeKind::Root {
                kind: RootKind::Production,
                target: NodeRef::File(FileId(1)), // b.ts is reachable
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        let findings = find_unused_files(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].subject_kind, "file");
        assert!(findings[0].message.contains("a.ts"));
    }

    #[test]
    fn an_exempt_file_also_blocks_rollup_even_though_it_reports_nothing_itself() {
        // vendor.js is Vendored (exempt, reports no finding of its own) but its mere presence
        // means "delete this whole folder" would also delete something never actually judged.
        let files = vec![
            file("src/legacy/a.ts", Some(FileClass::default())),
            file("src/legacy/b.ts", Some(FileClass::default())),
            file(
                "src/legacy/vendor.js",
                Some(FileClass {
                    role: FileRole::Production,
                    origin: FileOrigin::Vendored,
                }),
            ),
        ];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = reachability::compute(&graph);
        let findings = find_unused_files(&graph, &reach);
        assert_eq!(findings.len(), 2, "no rollup — vendor.js blocks it");
        assert!(findings.iter().all(|f| f.subject_kind == "file"));
    }

    #[test]
    fn a_single_dead_file_never_rolls_up_to_its_own_directory() {
        // A one-file "directory" finding is a worse restatement of the plain file finding.
        let files = vec![file("orphan.ts", Some(FileClass::default()))];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = reachability::compute(&graph);
        let findings = find_unused_files(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].subject_kind, "file");
    }

    #[test]
    fn directory_finding_id_is_stable_across_runs() {
        let (files, edges) = with_live_root(vec![
            file("src/legacy/a.ts", Some(FileClass::default())),
            file("src/legacy/b.ts", Some(FileClass::default())),
        ]);
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        let a = find_unused_files(&graph, &reach);
        let b = find_unused_files(&graph, &reach);
        assert_eq!(a[0].id, b[0].id);
    }

    // ---------------------------------------------------------------- symbols

    use crate::adapter::VisibilityLevel;
    use crate::graph::SymbolNode;
    use crate::vocab::SymbolKind;

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
        }
    }

    #[test]
    fn dead_symbol_in_a_reachable_file_is_reported() {
        let files = vec![file("main.ts", Some(FileClass::default()))];
        let symbols = vec![symbol(FileId(0), "used"), symbol(FileId(0), "dead")];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::File(FileId(0)),
                    to: crate::vocab::SymbolId(0),
                    kind: crate::vocab::RefKind::Read,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = reachability::compute(&graph);
        let findings = find_unused_symbols(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "unused");
        assert_eq!(findings[0].subject_kind, "function");
        assert!(findings[0].message.contains("main.ts#dead"));
    }

    #[test]
    fn referenced_symbol_is_not_reported() {
        let files = vec![file("main.ts", Some(FileClass::default()))];
        let symbols = vec![symbol(FileId(0), "used")];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::File(FileId(0)),
                    to: crate::vocab::SymbolId(0),
                    kind: crate::vocab::RefKind::Read,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = reachability::compute(&graph);
        assert!(find_unused_symbols(&graph, &reach).is_empty());
    }

    #[test]
    fn symbol_in_an_already_unused_file_is_not_double_reported() {
        // The file itself has no root and no importer — file-level `unused` already fires;
        // the rollup rule says the symbol finding must not duplicate it.
        let files = vec![file("orphan.ts", Some(FileClass::default()))];
        let symbols = vec![symbol(FileId(0), "dead")];
        let graph = ProjectGraph::for_test(files, symbols, vec![], vec![]);
        let reach = reachability::compute(&graph);
        assert!(find_unused_symbols(&graph, &reach).is_empty());
        assert_eq!(find_unused_files(&graph, &reach).len(), 1);
    }

    #[test]
    fn symbol_finding_id_is_stable_across_runs() {
        let files = vec![file("main.ts", Some(FileClass::default()))];
        let symbols = vec![symbol(FileId(0), "used"), symbol(FileId(0), "dead")];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::File(FileId(0)),
                    to: crate::vocab::SymbolId(0),
                    kind: crate::vocab::RefKind::Read,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = reachability::compute(&graph);
        let a = find_unused_symbols(&graph, &reach);
        let b = find_unused_symbols(&graph, &reach);
        assert_eq!(a[0].id, b[0].id);
    }
}
