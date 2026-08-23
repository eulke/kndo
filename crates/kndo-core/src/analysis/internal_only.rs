//! `internal-only` — declared visibility wider than any real usage requires (group
//! `waste`): the "declared > required" half of the visibility-mismatch pair (`private-type-leak`
//! is the other direction). "The analysis computes the **tightest sufficient visibility** — the
//! lowest ladder level that still covers the origin of every incoming reference. Declared above
//! it ⇒ finding."
//!
//! Generalized over the adapter-declared visibility ladder: each incoming
//! reference's origin is classified into the narrowest [`VisibilityScope`] relating it to the
//! declaring file (same file → `File`, same `FileNode::unit` → `Unit`, same package →
//! `Package`, else `Public`); the **required** scope is the widest of those over the strong
//! (≥ `Probable`) references. The tightest sufficient rung is then the lowest ladder index
//! whose scope covers it — if that rung's scope is strictly narrower than the declared rung's,
//! the finding fires and the remediation names the lower rung's *label* (the language's own
//! word). Two rungs sharing a scope never accuse each other (Java
//! `protected`/`public` both map to `Public` by the conservative-mapping rule — no evidence
//! could distinguish them). A language with no ladder, an empty ladder (CSS/JSON), or a
//! declared level the ladder doesn't cover is skipped outright — degrade toward silence.
//!
//! Exemptions: a symbol that is itself a root target (library-mode public API, a test
//! file's exported fixtures, a tooling config's exports — the promotion, already
//! wired in `graph::assemble`'s phase 3a) is externally consumed by definition, regardless of
//! whether any in-graph reference reaches it — never a candidate. Same for a symbol a plugin's
//! `annotate_symbols` marked externally consumed (`ProjectGraph::
//! is_externally_consumed`) — FFI, serialization, a public SDK surface the graph itself has no
//! edge for. A symbol with *zero* incoming
//! references at all, or one that's [`Reachability::Unreachable`] outright (dead code can have a
//! same-file self-reference and nothing else — a same-file `console.log(helper())` in a file
//! nothing imports), is `unused`'s verdict, not this one: suggesting "narrow the visibility" on
//! code already flagged for deletion is redundant noise (same rollup-taxonomy reasoning
//! `unused.rs` itself documents for skipping symbols in an already-unreachable file).
//!
//! Confidence: `Certain` when the strong references alone define the verdict; if a
//! `Possible`-confidence reference (the wildcard/dynamic tier — unreliable either
//! way) originates *wider* than the strong-evidence requirement, the verdict still fires but
//! demoted to `Possible`, mirroring "confidence demotes through wildcard edges like every
//! reachability verdict."

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::adapter::VisibilityScope;
use crate::analysis::finding_id;
use crate::analysis::reachability::{Reachability, ReachabilityMap};
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::{Confidence, EdgeKind, FileId, FileOrigin, NodeRef, SymbolId};

fn origin_file(graph: &ProjectGraph, node: NodeRef) -> FileId {
    match node {
        NodeRef::File(f) => f,
        NodeRef::Symbol(s) => graph.symbols[s.0 as usize].file,
    }
}

/// The narrowest scope that relates `origin` to the declaring file `decl` — what a reference
/// from `origin` *requires* the declaration's visibility to at least be. Scopes nest
/// (File ⊂ Unit ⊂ Package ⊂ Public), so this is a straight first-match walk.
fn required_scope(graph: &ProjectGraph, decl: FileId, origin: FileId) -> VisibilityScope {
    if decl == origin {
        return VisibilityScope::File;
    }
    let decl_file = &graph.files[decl.0 as usize];
    let origin_file = &graph.files[origin.0 as usize];
    if let (Some(a), Some(b)) = (&decl_file.unit, &origin_file.unit) {
        if a == b {
            return VisibilityScope::Unit;
        }
    }
    if decl_file.package == origin_file.package {
        return VisibilityScope::Package;
    }
    VisibilityScope::Public
}

pub fn find_internal_only(graph: &ProjectGraph, reach: &ReachabilityMap) -> Vec<Finding> {
    let root_targets: HashSet<NodeRef> = graph
        .edges
        .iter()
        .filter_map(|e| match e.kind {
            EdgeKind::Root { target, .. } => Some(target),
            _ => None,
        })
        .collect();

    // A reference attributed to a macro symbol executes at the macro's *expansion sites*,
    // not where the template is written — origins the graph cannot enumerate (a
    // `macro_rules!` body calling a `pub(crate)` fn expands wherever the macro is invoked).
    // Such a use requires the widest scope: degrade toward silence, never toward accusation
    //.
    let from_macro = |from: NodeRef| match from {
        NodeRef::Symbol(s) => graph.symbols[s.0 as usize].kind == crate::vocab::SymbolKind::Macro,
        NodeRef::File(_) => false,
    };
    let mut refs_by_target: HashMap<SymbolId, Vec<(FileId, Confidence, bool)>> = HashMap::default();
    for edge in &graph.edges {
        if let EdgeKind::References { from, to, .. } = edge.kind {
            refs_by_target.entry(to).or_default().push((
                origin_file(graph, from),
                edge.confidence,
                from_macro(from),
            ));
        }
    }

    let mut findings = Vec::new();
    for (index, symbol) in graph.symbols.iter().enumerate() {
        let file = &graph.files[symbol.file.0 as usize];
        let Some(class) = file.class else {
            continue; // unclaimed — out of scope, not a verdict
        };
        if matches!(class.origin, FileOrigin::Generated | FileOrigin::Vendored) {
            continue;
        }
        let Some(ladder) = file
            .language
            .as_deref()
            .and_then(|lang| graph.ladder_for(lang))
        else {
            continue; // no ladder declared — visibility semantics unknown, stay silent
        };
        let declared_index = symbol.visibility.0 as usize;
        let Some(declared) = ladder.get(declared_index) else {
            continue; // level the ladder doesn't cover (empty ladder included) — stay silent
        };
        if declared_index == 0 {
            continue; // already the tightest rung there is — nothing to narrow
        }

        if symbol.kind == crate::vocab::SymbolKind::Constructor {
            continue; // a constructor's only in-graph reference is the synthetic
                      // container→constructor liveness edge (graph.rs) — same-file by
                      // construction, so any verdict here would accuse kndo's own modeling,
                      // not the code; real call sites reference the type, which is measured
        }
        if symbol.kind == crate::vocab::SymbolKind::Macro {
            continue; // an expansion symbol's invocations resolve textually (SymbolKind::Macro
                      // contract), not through the module ladder the graph measures — the
                      // observed use scope is structurally underestimated, so any narrowing
                      // advice would be a guess
        }

        let symbol_id = SymbolId(index as u32);
        if root_targets.contains(&NodeRef::Symbol(symbol_id)) {
            continue; // roots are externally consumed by definition (the exemption)
        }
        if graph.is_externally_consumed(symbol_id) {
            continue; // a plugin marked it externally consumed (the annotate_symbols
                      // exemption) — a narrower visibility is real advice for the
                      // language, but not for whatever the plugin says reaches this from outside
                      // the graph (serialization, FFI, a public SDK surface)
        }
        if reach.get(NodeRef::Symbol(symbol_id)).0 == Reachability::Unreachable {
            continue; // dead code — `unused`'s verdict, not this one
        }

        let Some(refs) = refs_by_target.get(&symbol_id) else {
            continue; // zero incoming references at all — `unused`'s verdict, not this one
        };
        // Strong (≥ Probable) references define what the declaration *must* cover; a weak
        // (Possible) reference from wider than that doesn't widen the requirement — it
        // demotes the verdict's confidence instead.
        let origin_scope = |f: FileId, via_macro: bool| {
            if via_macro {
                VisibilityScope::Public
            } else {
                required_scope(graph, symbol.file, f)
            }
        };
        let required = refs
            .iter()
            .filter(|&&(_, c, _)| c >= Confidence::Probable)
            .map(|&(f, _, m)| origin_scope(f, m))
            .max()
            .unwrap_or(VisibilityScope::File);
        let weak_wider = refs
            .iter()
            .filter(|&&(_, c, _)| c < Confidence::Probable)
            .any(|&(f, _, m)| origin_scope(f, m) > required);

        // The tightest sufficient rung: lowest index whose scope covers every strong origin.
        let Some((tightest_index, tightest)) = ladder
            .iter()
            .enumerate()
            .find(|(_, rung)| rung.scope >= required)
        else {
            continue; // no rung covers the usage — nothing narrower to suggest
        };
        if tightest_index >= declared_index || tightest.scope >= declared.scope {
            continue; // declared is already tightest, or only same-scope rungs below it
        }

        let confidence = if weak_wider {
            Confidence::Possible
        } else {
            Confidence::Certain
        };
        let usage = match required {
            VisibilityScope::File => "its own file",
            VisibilityScope::Unit => "its own unit",
            VisibilityScope::Package => "its own package",
            VisibilityScope::Public => "the project", // unreachable: Public rungs cover it
        };
        let path = file.path.0.as_str();
        let facet = symbol.kind.facet();
        let qualified = symbol.qualified_name();
        findings.push(Finding {
            advisory: false,
            id: finding_id("internal-only", facet, path, &qualified, ""),
            category: "internal-only".to_string(),
            group: "waste".to_string(),
            subject_kind: facet.to_string(),
            severity: Severity::Info, // info by default
            confidence,
            message: format!(
                "{path}#{qualified} is declared {} but only used within {usage} — {} would suffice for this {facet}",
                declared.label, tightest.label
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
    use crate::graph::{FileNode, ProjectGraph, SymbolNode};
    use crate::vocab::{
        Edge, FileClass, FileOrigin, FileRole, Provenance, RefKind, RootKind, SymbolKind,
    };
    use smol_str::SmolStr;

    fn file(path: &str) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: Some(SmolStr::new("mock")),
            class: Some(FileClass {
                role: FileRole::Production,
                origin: FileOrigin::Authored,
            }),
            package: crate::vocab::PackageId(0),
            unit: None,
            test_spans: Vec::new(),
            string_call_sites: Vec::new(),
        }
    }

    fn symbol(file: FileId, name: &str, visibility: u8) -> SymbolNode {
        SymbolNode {
            file,
            name: SmolStr::new(name),
            kind: SymbolKind::Function,
            span: Default::default(),
            exported: visibility > 0,
            visibility: VisibilityLevel(visibility),
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
    fn exported_symbol_used_only_in_its_own_file_is_internal_only() {
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
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
                    to: SymbolId(0),
                    kind: RefKind::Call,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        let findings = find_internal_only(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "internal-only");
        assert_eq!(findings[0].group, "waste");
        assert_eq!(findings[0].confidence, Confidence::Certain);
        assert!(findings[0].message.contains("src/a.ts#helper"));
    }

    #[test]
    fn exported_symbol_referenced_cross_file_is_not_internal_only() {
        let files = vec![file("src/a.ts"), file("src/b.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
        let edges = vec![edge(
            EdgeKind::References {
                from: NodeRef::File(FileId(1)),
                to: SymbolId(0),
                kind: RefKind::Call,
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn dead_code_with_a_same_file_self_reference_is_exempt_not_double_flagged() {
        // No root anywhere — the whole graph is unreachable. `helper` still has an incoming
        // same-file reference (e.g. `console.log(helper())` in a file nothing imports), which
        // without the reachability gate would read as internal-only — redundant with `unused`
        // already covering the same dead code.
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
        let edges = vec![edge(
            EdgeKind::References {
                from: NodeRef::File(FileId(0)),
                to: SymbolId(0),
                kind: RefKind::Call,
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn unexported_symbol_is_already_tightest_and_never_a_candidate() {
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 0)];
        let edges = vec![edge(
            EdgeKind::References {
                from: NodeRef::File(FileId(0)),
                to: SymbolId(0),
                kind: RefKind::Call,
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn zero_references_is_unused_s_verdict_not_this_one() {
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
        let graph = ProjectGraph::for_test(files, symbols, vec![], vec![]);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn a_root_symbol_is_exempt_even_if_only_self_file_referenced() {
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "publicApi", 1)];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::Symbol(SymbolId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::File(FileId(0)),
                    to: SymbolId(0),
                    kind: RefKind::Call,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn a_root_symbol_with_no_references_at_all_is_still_exempt() {
        // Library-mode promotion: nothing in the graph calls it, but it's the package's public
        // API surface — externally consumed by definition, never a candidate for either verdict.
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "publicApi", 1)];
        let edges = vec![edge(
            EdgeKind::Root {
                kind: RootKind::Production,
                target: NodeRef::Symbol(SymbolId(0)),
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn only_possible_confidence_cross_file_reference_demotes_not_exempts() {
        let files = vec![file("src/a.ts"), file("src/b.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(1)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::File(FileId(1)),
                    to: SymbolId(0),
                    kind: RefKind::Call,
                },
                Confidence::Possible,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        let findings = find_internal_only(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].confidence, Confidence::Possible);
    }

    #[test]
    fn plugin_annotated_externally_consumed_symbol_is_exempt() {
        // The file (not the symbol itself) is the root, and the only reference is a same-file,
        // Certain-confidence call — real enough evidence to fire "should be private" on its own
        // (asserted first, below). A plugin's `annotate_symbols` marking the
        // symbol externally consumed must suppress it exactly like a root target would.
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
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
                    to: SymbolId(0),
                    kind: RefKind::Call,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files.clone(), symbols.clone(), vec![], edges.clone());
        let reach = crate::analysis::reachability::compute(&graph);
        assert_eq!(find_internal_only(&graph, &reach).len(), 1);

        let annotated = ProjectGraph::for_test(files, symbols, vec![], edges)
            .with_externally_consumed(vec![SymbolId(0)]);
        let reach = crate::analysis::reachability::compute(&annotated);
        assert!(find_internal_only(&annotated, &reach).is_empty());
    }

    #[test]
    fn reference_from_within_another_symbol_resolves_to_that_symbol_s_file() {
        // The referencing edge's `from` is a Symbol, not a File — origin_file must resolve it
        // through the symbol's own `file` field, same cross-file logic either way.
        let files = vec![file("src/a.ts"), file("src/b.ts")];
        let symbols = vec![
            symbol(FileId(0), "helper", 1),
            symbol(FileId(1), "caller", 0),
        ];
        let edges = vec![edge(
            EdgeKind::References {
                from: NodeRef::Symbol(SymbolId(1)),
                to: SymbolId(0),
                kind: RefKind::Call,
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn generated_origin_is_exempt() {
        let files = vec![FileNode {
            path: ProjectPath(SmolStr::new("dist/a.ts")),
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
        }];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
        let edges = vec![edge(
            EdgeKind::References {
                from: NodeRef::File(FileId(0)),
                to: SymbolId(0),
                kind: RefKind::Call,
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn finding_id_is_stable_across_runs() {
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
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
                    to: SymbolId(0),
                    kind: RefKind::Call,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        let a = find_internal_only(&graph, &reach);
        let b = find_internal_only(&graph, &reach);
        assert_eq!(a[0].id, b[0].id);
    }

    // ------------------------------------------ ladder generalization

    fn file_in_unit(path: &str, unit: &str) -> FileNode {
        let mut f = file(path);
        f.unit = Some(SmolStr::new(unit));
        f
    }

    fn root_and_ref(root_file: FileId, from: FileId, to: SymbolId) -> Vec<Edge> {
        vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(root_file),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::File(from),
                    to,
                    kind: RefKind::Call,
                },
                Confidence::Certain,
            ),
        ]
    }

    #[test]
    fn exported_symbol_used_only_by_same_unit_siblings_is_internal_only() {
        // The Go under-reporting the ladder prevents: without it, cross-file evidence would
        // justify any exported level; with the ladder, a same-unit-only use narrows to the
        // Unit rung ("could be unexported").
        let files = vec![
            file_in_unit("pkg/a.go2", "pkg#p"),
            file_in_unit("pkg/b.go2", "pkg#p"),
        ];
        let symbols = vec![symbol(FileId(0), "Helper", 1)];
        let edges = root_and_ref(FileId(1), FileId(1), SymbolId(0));
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        let findings = find_internal_only(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].confidence, Confidence::Certain);
        assert!(
            findings[0].message.contains("private would suffice"),
            "remediation must name the lower rung's label: {}",
            findings[0].message
        );
    }

    #[test]
    fn cross_package_use_needs_the_widest_rung_and_is_not_flagged() {
        // Mock ladder is [Unit, Public]: a strong reference from another package requires
        // Package scope, and the lowest covering rung is Public — exactly the declared level.
        let mut f0 = file("a/x.ts");
        f0.package = crate::vocab::PackageId(0);
        let mut f1 = file("b/y.ts");
        f1.package = crate::vocab::PackageId(1);
        let symbols = vec![symbol(FileId(0), "helper", 1)];
        let edges = root_and_ref(FileId(1), FileId(1), SymbolId(0));
        let graph =
            ProjectGraph::for_test(vec![f0, f1], symbols, vec![], edges).with_packages(vec![
                crate::graph::PackageNode {
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
                crate::graph::PackageNode {
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
            ]);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn a_language_with_no_declared_ladder_is_skipped() {
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
        let edges = root_and_ref(FileId(0), FileId(0), SymbolId(0));
        let graph =
            ProjectGraph::for_test(files, symbols, vec![], edges).with_visibility_ladders(vec![]);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn a_declared_level_beyond_the_ladder_is_skipped_not_accused() {
        // Conservative degradation: an adapter emitting a level its ladder doesn't name is a
        // contract wobble — stay silent rather than guess a scope.
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 7)];
        let edges = root_and_ref(FileId(0), FileId(0), SymbolId(0));
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    fn rung(scope: crate::adapter::VisibilityScope, label: &str) -> crate::adapter::VisibilityRung {
        crate::adapter::VisibilityRung {
            scope,
            label: SmolStr::new(label),
            surface_transitive: matches!(scope, crate::adapter::VisibilityScope::Public),
        }
    }

    #[test]
    fn multi_rung_ladder_names_the_tightest_sufficient_label() {
        // JS-shaped ladder [File, Package, Public]: declared at the middle rung, used
        // same-file only — the remediation names rung 0's label, not just "not exported".
        use crate::adapter::VisibilityScope::*;
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
        let edges = root_and_ref(FileId(0), FileId(0), SymbolId(0));
        let graph =
            ProjectGraph::for_test(files, symbols, vec![], edges).with_visibility_ladders(vec![(
                SmolStr::new("mock"),
                vec![
                    rung(File, "module-local"),
                    rung(Package, "exported"),
                    rung(Public, "package surface"),
                ],
            )]);
        let reach = crate::analysis::reachability::compute(&graph);
        let findings = find_internal_only(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0]
                .message
                .contains("declared exported but only used within its own file"),
            "{}",
            findings[0].message
        );
        assert!(
            findings[0].message.contains("module-local would suffice"),
            "{}",
            findings[0].message
        );
    }

    #[test]
    fn a_macro_subject_is_never_accused() {
        // Macro invocations resolve textually, not through the module ladder — the graph
        // structurally underestimates a macro's use scope, so no narrowing advice is safe.
        let files = vec![file("src/a.ts")];
        let mut m = symbol(FileId(0), "shout", 2);
        m.kind = SymbolKind::Macro;
        let edges = root_and_ref(FileId(0), FileId(0), SymbolId(0));
        let graph = ProjectGraph::for_test(files, vec![m], vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn a_use_from_inside_a_macro_counts_as_expansion_site_wide() {
        // `helper` is exported and its only strong reference comes from a same-file macro's
        // template — but that template executes wherever the macro expands, so the use does
        // not justify narrowing (a `log_error! → set_errored` shape). The identical
        // graph with a function as the referrer must still accuse (asserted second).
        let files = vec![file("src/a.ts")];
        let referrer_kinds = [SymbolKind::Macro, SymbolKind::Function];
        let verdicts: Vec<usize> = referrer_kinds
            .map(|kind| {
                let mut referrer = symbol(FileId(0), "emit", 0);
                referrer.kind = kind;
                let symbols = vec![symbol(FileId(0), "helper", 1), referrer];
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
                            to: SymbolId(1),
                            kind: RefKind::Call,
                        },
                        Confidence::Certain,
                    ),
                    edge(
                        EdgeKind::References {
                            from: NodeRef::Symbol(SymbolId(1)),
                            to: SymbolId(0),
                            kind: RefKind::Call,
                        },
                        Confidence::Certain,
                    ),
                ];
                let graph = ProjectGraph::for_test(files.clone(), symbols, vec![], edges);
                let reach = crate::analysis::reachability::compute(&graph);
                find_internal_only(&graph, &reach).len()
            })
            .to_vec();
        assert_eq!(verdicts, vec![0, 1]);
    }

    #[test]
    fn same_scope_rungs_never_accuse_each_other() {
        // Java-shaped tail [.., Public "protected", Public "public"]: a symbol declared at
        // the top rung whose uses require Public must NOT be told to become "protected" —
        // no static evidence can distinguish two rungs sharing a scope.
        use crate::adapter::VisibilityScope::*;
        let mut f0 = file("A.java2");
        f0.package = crate::vocab::PackageId(0);
        let mut f1 = file("B.java2");
        f1.package = crate::vocab::PackageId(1);
        let symbols = vec![symbol(FileId(0), "helper", 3)];
        let edges = root_and_ref(FileId(1), FileId(1), SymbolId(0));
        let graph = ProjectGraph::for_test(vec![f0, f1], symbols, vec![], edges)
            .with_packages(vec![
                crate::graph::PackageNode {
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
                crate::graph::PackageNode {
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
            ])
            .with_visibility_ladders(vec![(
                SmolStr::new("mock"),
                vec![
                    rung(File, "private"),
                    rung(Unit, "package-private"),
                    rung(Public, "protected"),
                    rung(Public, "public"),
                ],
            )]);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }
}
