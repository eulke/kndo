//! `private-type-leak` — declared visibility *below* what usage requires (group
//! `defect`): the second half of the visibility-mismatch pair (`internal-only` is the other
//! direction). "A public/exported symbol whose signature references a type of *lower*
//! visibility — the API promises a type its consumers cannot name."
//!
//! Built exactly on the two facts and nothing else: a `TypeUse`-kinded `References`
//! edge attributed (`within`) to the exported declaration, whose site span lies inside
//! that declaration's `signature_span`. Body-internal type uses are not leaks (a private type
//! used *inside* a public function is ordinary encapsulation); only the signature is a
//! promise. v1 is deliberately callables-only — `signature_span` is `None` on type
//! declarations until fields exist as member declarations with their own visibility, because
//! firing on a whole struct body would falsely accuse exported-struct/unexported-field shapes
//! (degrade toward silence, never toward accusation).
//!
//! Severity: warning in library-mode packages (a lying public API), info in
//! app packages — the package's publish signal (`PackageNode::private`) is the
//! mode. Only **certain**-confidence evidence edges accuse (fallback bindings
//! routinely pick the wrong same-name type), the declaration's *effective* surface is computed
//! through its `member_of` chain (an exported-looking member of an unexported container is not
//! public API), and test-role code is exempt. Cross-language pairs are skipped —
//! visibility levels only mean anything *within one language's ladder* and
//! comparing them across languages would be numerology. "Lower visibility" is compared as
//! ladder rung *scopes* when the language declared a ladder (so same-scope rungs like Java
//! `protected`/`public` never accuse each other), raw indices otherwise.
//!
//! Exemption (shared with `internal-only`): a leaked type a plugin's
//! `annotate_symbols` marked externally consumed (`ProjectGraph::is_externally_consumed`) isn't
//! actually unnameable to consumers — FFI, serialization, a public SDK surface the graph itself
//! has no edge for — so the "lying public API" verdict doesn't hold.

use rustc_hash::FxHashMap as HashMap;

use smol_str::SmolStr;

use crate::adapter::Span;
use crate::analysis::{finding_id, FindingIdParts};
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::{
    Category, Confidence, EdgeKind, FileOrigin, FileRole, Group, NodeRef, RefKind, SubjectKind,
    SymbolId,
};

fn contains(outer: &Span, inner: &Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// The declaration's *effective* surface through its `member_of` chain: a
/// capitalized Go method on an unexported receiver, or a public method of a private nested
/// Java class, is not public API — a member is only as visible as every container above it.
/// `exported` is the conjunction along the chain; `visibility` the lowest rung. `None` when a
/// container name doesn't resolve to a same-file declaration (a generic impl target like
/// `impl<M> Trait for &M` — no surface we can vouch for; degrade toward silence).
struct EffectiveSurface {
    exported: bool,
    visibility: crate::adapter::VisibilityLevel,
    /// The anchor belonging to whichever declaration in the container chain supplied
    /// `visibility` — a level without its anchor names no region at all.
    anchor: Option<SmolStr>,
}

fn effective_surface(
    graph: &ProjectGraph,
    by_file_name: &HashMap<(u32, &str), Vec<SymbolId>>,
    decl_id: SymbolId,
) -> Option<EffectiveSurface> {
    let decl = &graph.symbols[decl_id.0 as usize];
    let mut exported = decl.exported;
    let mut visibility = decl.visibility;
    let mut anchor = decl.visible_in_unit.clone();
    let mut current = decl;
    for _ in 0..8 {
        let Some(container_name) = current.member_of.as_deref() else {
            return Some(EffectiveSurface {
                exported,
                visibility,
                anchor,
            });
        };
        let candidates = by_file_name.get(&(current.file.0, container_name))?;
        // Prefer the container whose span encloses the member; fall back to the first.
        let container_id = candidates
            .iter()
            .find(|c| {
                let s = &graph.symbols[c.0 as usize];
                contains(&s.span, &current.span)
            })
            .or_else(|| candidates.first())?;
        let container = &graph.symbols[container_id.0 as usize];
        exported &= container.exported;
        if container.visibility < visibility {
            visibility = container.visibility;
            anchor = container.visible_in_unit.clone();
        }
        current = container;
    }
    None // pathological nesting depth — vouch for nothing
}

pub fn find_private_type_leaks(graph: &ProjectGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    // One finding per (declaration, leaked type) pair, however many signature sites repeat it.
    let mut seen: HashMap<(SymbolId, SymbolId), ()> = HashMap::default();
    let mut by_file_name: HashMap<(u32, &str), Vec<SymbolId>> = HashMap::default();
    for (i, s) in graph.symbols.iter().enumerate() {
        by_file_name
            .entry((s.file.0, s.name.as_str()))
            .or_default()
            .push(SymbolId(i as u32));
    }
    // `scope_contains_site` reads units positionally, the same shape assembly hands it.
    let file_units: Vec<Option<SmolStr>> = graph.files.iter().map(|f| f.unit.clone()).collect();
    let unit_parents = crate::graph::unit_parent_index(&graph.files);

    for edge in &graph.edges {
        let EdgeKind::References {
            from: NodeRef::Symbol(decl_id),
            to: type_id,
            kind: RefKind::TypeUse,
        } = edge.kind
        else {
            continue;
        };
        // A defect-group accusation needs certain evidence: a duck-typed or qualified-table
        // fallback binding (Probable/Possible) routinely picks the wrong same-name type
        // across files — e.g. binding stdlib `Error` mentions to
        // arbitrary nested `Error` enums. Degrade toward silence.
        if edge.confidence != Confidence::Certain {
            continue;
        }
        let Some(site) = edge.span else { continue };

        let decl = &graph.symbols[decl_id.0 as usize];
        let Some(sig) = decl.signature_span else {
            continue; // no declared signature — nothing to distinguish from the body
        };
        if !decl.exported || !contains(&sig, &site) {
            continue;
        }
        // The effective surface through the container chain — and only from production code:
        // a test file's exports are not an API promise.
        let Some(surface) = effective_surface(graph, &by_file_name, decl_id) else {
            continue;
        };
        if !surface.exported {
            continue;
        }

        if graph.is_externally_consumed(type_id) {
            continue; // a plugin marked the leaked type externally consumed (the
                      // annotate_symbols exemption) — its consumers really can
                      // name it, just not through an edge the graph itself models (FFI,
                      // serialization, a public SDK surface the plugin knows about)
        }

        let leaked = &graph.symbols[type_id.0 as usize];

        let decl_file = &graph.files[decl.file.0 as usize];
        let leaked_file = &graph.files[leaked.file.0 as usize];
        let Some(class) = decl_file.class else {
            continue;
        };
        if matches!(class.origin, FileOrigin::Generated | FileOrigin::Vendored) {
            continue;
        }
        if class.role == FileRole::Test
            || crate::graph::span_in_test_region(&decl_file.test_spans, decl.span)
        {
            continue; // a test file's exports are not an API promise
        }
        if decl_file.language != leaked_file.language {
            continue; // visibility levels only compare within one language (module doc)
        }
        let ladder = decl_file
            .language
            .as_deref()
            .and_then(|lang| graph.ladder_for(lang));

        // This verdict's whole premise is its own message: "consumers can see the function yet
        // cannot name that type". `exported` alone does not establish that there ARE consumers
        // — it is true for `pub(crate)`, top-level `pub(super)`, Java package-private and Swift
        // `internal`, none of which cross the package boundary. `surface_transitive` is the
        // ladder rung that does establish it, and every other consumer of the ladder in the
        // codebase gates on it too (`graph::assemble`'s library-mode promotion, `graph::surface`'s
        // member closure) — an intra-package item has no consumers outside the package to lie
        // to, so it must not be accused of leaking to them.
        //
        // `VisibilityScope::Module` (a unit and its subtree, `internal/detection-gaps.md` §7)
        // gives the containment test below a scope finer than the four-bucket ladder: an item
        // visible to a *sibling module* that names a type private to its own module is judged
        // against the region it actually names, so that shape is a genuine leak by the letter
        // of the language's rules.

        // "Lower visibility" via the language's ladder when it's declared:
        // comparing *scopes* — not raw indices — means two rungs sharing a scope (Java
        // `protected`/`public`, both `Public` by the conservative-mapping rule) never accuse
        // each other. Fall back to index comparison when no ladder covers the levels — sound
        // only because the language check above already confines the comparison to one language.
        //
        // Equal scopes are NOT automatically safe: the buckets are relative to the symbol that
        // owns them, so two `File`-scoped symbols in different files, or two `Unit`-scoped ones
        // in different units, name disjoint regions. The type's region has to actually contain
        // the declaration's, which is what `scope_contains_site` decides — asking whether the
        // declaring file sits inside the region the leaked type is visible to.
        // One question, asked once: does the region the TYPE is visible in reach everywhere the
        // item promises itself to? A scope alone cannot answer it — two `File` scopes in
        // different files, or two module subtrees anchored at different depths, are disjoint
        // regions an enum comparison reads as equal — so the comparison is region against
        // region (`graph::region_covers`).
        let leaks = match ladder.map(|l| {
            (
                l.get(leaked.visibility.0 as usize),
                l.get(surface.visibility.0 as usize),
            )
        }) {
            Some((Some(leaked_rung), Some(decl_rung))) => !crate::graph::region_covers(
                &crate::graph::VisibilityRegion {
                    scope: leaked_rung.scope,
                    anchor: leaked.visible_in_unit.as_ref(),
                    file: leaked.file.0 as usize,
                },
                &crate::graph::VisibilityRegion {
                    scope: decl_rung.scope,
                    anchor: surface.anchor.as_ref(),
                    file: decl.file.0 as usize,
                },
                &file_units,
                &unit_parents,
                &graph.files,
            ),
            _ => leaked.visibility < surface.visibility,
        };
        if !leaks {
            continue; // the type is at least as visible as the promise — no leak
        }
        if seen.insert((decl_id, type_id), ()).is_some() {
            continue;
        }

        let path = decl_file.path.0.as_str();
        let facet = decl.kind.facet();
        let qualified = decl.qualified_name();
        let leaked_name = leaked.qualified_name();
        // Library vs app mode: the publish signal of the *declaring* package.
        let is_library = graph
            .packages
            .get(decl_file.package.0 as usize)
            .map(|p| !p.private)
            .unwrap_or(false);
        findings.push(Finding {
            advisory: false,
            id: finding_id(FindingIdParts {
                category: &Category::PRIVATE_TYPE_LEAK,
                subject_kind: &SubjectKind::new(facet),
                path,
                symbol_path: &qualified,
                discriminator: &leaked_name,
            }),
            category: Category::PRIVATE_TYPE_LEAK,
            group: Group::Defect,
            subject_kind: SubjectKind::new(facet),
            severity: if is_library {
                Severity::Warning
            } else {
                Severity::Info
            },
            confidence: edge.confidence,
            message: format!(
                "{path}#{qualified} is exported but its signature references {leaked_name}, \
                 which is not — consumers can see the {facet} yet cannot name that type \
                 (export {leaked_name}, or narrow {qualified})"
            ),
            location: Location {
                path: Some(decl_file.path.clone()),
                range: Some(site),
                symbol: Some(qualified),
                package: graph.package_name(decl_file.package).map(str::to_string),
            },
            related: Vec::new(),
            rolled_up: None,
            sources: Vec::new(),
            delta: None,
            delta_origin: None,
        });
    }
    findings.sort_by(|a, b| a.id.cmp(&b.id));
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ProjectPath, VisibilityLevel};
    use crate::graph::{FileNode, SymbolNode};
    use crate::vocab::{Confidence, Edge, FileClass, FileId, FileRole, Provenance, SymbolKind};
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
            unit_parent: None,
            test_spans: Vec::new(),
            string_call_sites: Vec::new(),
            string_attr_args: Vec::new(),
        }
    }

    fn span(sl: u32, sc: u32, el: u32, ec: u32) -> Span {
        Span {
            start: (sl, sc),
            end: (el, ec),
        }
    }

    fn callable(file: FileId, name: &str, visibility: u8, sig: Span) -> SymbolNode {
        SymbolNode {
            file,
            name: SmolStr::new(name),
            kind: SymbolKind::Function,
            span: span(sig.start.0, 1, sig.end.0 + 10, 1),
            exported: visibility > 0,
            visibility: VisibilityLevel(visibility),
            member_of: None,
            signature_span: Some(sig),
            implicitly_invoked: false,
            nested_scope: false,
            visibility_inherited: false,
            visible_in_unit: None,
            implements: None,
            markers: Vec::new(),
        }
    }

    fn ty(file: FileId, name: &str, visibility: u8) -> SymbolNode {
        SymbolNode {
            file,
            name: SmolStr::new(name),
            kind: SymbolKind::Struct,
            span: Span::default(),
            exported: visibility > 0,
            visibility: VisibilityLevel(visibility),
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

    fn type_use(from: SymbolId, to: SymbolId, site: Span, confidence: Confidence) -> Edge {
        Edge {
            owner: crate::vocab::FileId(0),
            kind: EdgeKind::References {
                from: NodeRef::Symbol(from),
                to,
                kind: RefKind::TypeUse,
            },
            confidence,
            source: Provenance::Adapter(SmolStr::new("mock")),
            span: Some(site),
        }
    }

    fn graph_with(symbols: Vec<SymbolNode>, edges: Vec<Edge>) -> ProjectGraph {
        ProjectGraph::for_test(vec![file("a.mock")], symbols, vec![], edges)
    }

    #[test]
    fn exported_callable_with_unexported_type_in_signature_is_a_leak() {
        let symbols = vec![
            callable(FileId(0), "F", 1, span(1, 1, 1, 40)),
            ty(FileId(0), "secret", 0),
        ];
        let edges = vec![type_use(
            SymbolId(0),
            SymbolId(1),
            span(1, 10, 1, 16),
            Confidence::Certain,
        )];
        let findings = find_private_type_leaks(&graph_with(symbols, edges));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "private-type-leak");
        assert_eq!(findings[0].group, crate::vocab::Group::Defect);
        assert!(findings[0].message.contains("secret"));
    }

    #[test]
    fn plugin_annotated_externally_consumed_type_is_exempt() {
        // Same leak shape as above (which fires without the annotation) — a plugin's
        // `annotate_symbols` marking the *leaked type* externally consumed means
        // its consumers really can name it (FFI, serialization, a public SDK surface the graph
        // has no edge for), so the "lying public API" verdict doesn't hold.
        let symbols = vec![
            callable(FileId(0), "F", 1, span(1, 1, 1, 40)),
            ty(FileId(0), "secret", 0),
        ];
        let edges = vec![type_use(
            SymbolId(0),
            SymbolId(1),
            span(1, 10, 1, 16),
            Confidence::Certain,
        )];
        let graph = graph_with(symbols, edges).with_externally_consumed(vec![SymbolId(1)]);
        assert!(find_private_type_leaks(&graph).is_empty());
    }

    #[test]
    fn a_body_type_use_is_not_a_leak() {
        // Site outside the signature span: a private type used inside the body is ordinary
        // encapsulation, not a promise.
        let symbols = vec![
            callable(FileId(0), "F", 1, span(1, 1, 1, 40)),
            ty(FileId(0), "secret", 0),
        ];
        let edges = vec![type_use(
            SymbolId(0),
            SymbolId(1),
            span(3, 5, 3, 11),
            Confidence::Certain,
        )];
        assert!(find_private_type_leaks(&graph_with(symbols, edges)).is_empty());
    }

    #[test]
    fn an_equally_visible_type_is_not_a_leak() {
        let symbols = vec![
            callable(FileId(0), "F", 1, span(1, 1, 1, 40)),
            ty(FileId(0), "Public", 1),
        ];
        let edges = vec![type_use(
            SymbolId(0),
            SymbolId(1),
            span(1, 10, 1, 16),
            Confidence::Certain,
        )];
        assert!(find_private_type_leaks(&graph_with(symbols, edges)).is_empty());
    }

    #[test]
    fn an_unexported_callable_never_leaks() {
        let symbols = vec![
            callable(FileId(0), "f", 0, span(1, 1, 1, 40)),
            ty(FileId(0), "secret", 0),
        ];
        let edges = vec![type_use(
            SymbolId(0),
            SymbolId(1),
            span(1, 10, 1, 16),
            Confidence::Certain,
        )];
        assert!(find_private_type_leaks(&graph_with(symbols, edges)).is_empty());
    }

    #[test]
    fn a_read_reference_in_the_signature_is_not_a_leak() {
        let symbols = vec![
            callable(FileId(0), "F", 1, span(1, 1, 1, 40)),
            ty(FileId(0), "secret", 0),
        ];
        let edges = vec![Edge {
            owner: crate::vocab::FileId(0),
            kind: EdgeKind::References {
                from: NodeRef::Symbol(SymbolId(0)),
                to: SymbolId(1),
                kind: RefKind::Read,
            },
            confidence: Confidence::Certain,
            source: Provenance::Adapter(SmolStr::new("mock")),
            span: Some(span(1, 10, 1, 16)),
        }];
        assert!(find_private_type_leaks(&graph_with(symbols, edges)).is_empty());
    }

    #[test]
    fn severity_follows_the_package_publish_signal() {
        // for_test builds one implicit non-private package → library mode → Warning.
        let symbols = vec![
            callable(FileId(0), "F", 1, span(1, 1, 1, 40)),
            ty(FileId(0), "secret", 0),
        ];
        let edges = vec![type_use(
            SymbolId(0),
            SymbolId(1),
            span(1, 10, 1, 16),
            Confidence::Certain,
        )];
        let findings = find_private_type_leaks(&graph_with(symbols, edges));
        assert_eq!(findings[0].severity, Severity::Warning);
    }

    #[test]
    fn repeated_signature_sites_yield_one_finding_per_pair() {
        let symbols = vec![
            callable(FileId(0), "F", 1, span(1, 1, 1, 60)),
            ty(FileId(0), "secret", 0),
        ];
        let edges = vec![
            type_use(
                SymbolId(0),
                SymbolId(1),
                span(1, 10, 1, 16),
                Confidence::Certain,
            ),
            type_use(
                SymbolId(0),
                SymbolId(1),
                span(1, 30, 1, 36),
                Confidence::Certain,
            ),
        ];
        assert_eq!(
            find_private_type_leaks(&graph_with(symbols, edges)).len(),
            1
        );
    }

    #[test]
    fn same_scope_rungs_do_not_leak_even_with_different_indices() {
        // Java-shaped tail: `protected` (rung 2) in a `public` (rung 3) signature — both map
        // to `Public` scope by the conservative rule, so index inequality alone
        // must not accuse.
        use crate::adapter::{VisibilityRung, VisibilityScope};
        let rung = |scope, label: &str| VisibilityRung {
            scope,
            label: SmolStr::new(label),
            surface_transitive: matches!(scope, crate::adapter::VisibilityScope::Public),
        };
        let symbols = vec![
            callable(FileId(0), "F", 3, span(1, 1, 1, 40)),
            ty(FileId(0), "Prot", 2),
        ];
        let edges = vec![type_use(
            SymbolId(0),
            SymbolId(1),
            span(1, 10, 1, 16),
            Confidence::Certain,
        )];
        let graph = graph_with(symbols, edges).with_visibility_ladders(vec![(
            SmolStr::new("mock"),
            vec![
                rung(VisibilityScope::File, "private"),
                rung(VisibilityScope::Unit, "package-private"),
                rung(VisibilityScope::Public, "protected"),
                rung(VisibilityScope::Public, "public"),
            ],
        )]);
        assert!(find_private_type_leaks(&graph).is_empty());
    }

    #[test]
    fn a_package_visible_item_naming_a_file_private_type_is_a_leak() {
        // Regions decide this shape on its merits: with a ladder where "private" really means
        // the FILE, a package-visible item naming one is a genuine leak — every caller it
        // promises itself to is outside that file. A same-bucket comparison alone cannot tell
        // ripgrep's `flags::parse::lookup` (whose whole module subtree CAN name the private
        // `Flag`) apart from tokio's `task::state::unset_waker` (whose sibling caller cannot
        // name `UpdateResult`); comparing the actual regions can.
        use crate::adapter::{VisibilityRung, VisibilityScope};
        let ladder = vec![
            VisibilityRung {
                scope: VisibilityScope::File,
                label: SmolStr::new("private"),
                surface_transitive: false,
            },
            VisibilityRung {
                scope: VisibilityScope::Package,
                label: SmolStr::new("pub(crate)"),
                surface_transitive: false,
            },
            VisibilityRung {
                scope: VisibilityScope::Public,
                label: SmolStr::new("pub"),
                surface_transitive: true,
            },
        ];
        let edges = || {
            vec![type_use(
                SymbolId(0),
                SymbolId(1),
                span(1, 10, 1, 16),
                Confidence::Certain,
            )]
        };

        let contained = graph_with(
            vec![
                callable(FileId(0), "helper", 1, span(1, 1, 1, 40)),
                ty(FileId(0), "Parameters", 0),
            ],
            edges(),
        )
        .with_visibility_ladders(vec![(SmolStr::new("mock"), ladder.clone())]);
        assert_eq!(
            find_private_type_leaks(&contained).len(),
            1,
            "the package can reach `helper` and cannot name `Parameters`"
        );

        // The published half of the same ladder still accuses: `pub` IS a promise.
        let published = graph_with(
            vec![
                callable(FileId(0), "run", 2, span(1, 1, 1, 40)),
                ty(FileId(0), "FnVisitor", 0),
            ],
            edges(),
        )
        .with_visibility_ladders(vec![(SmolStr::new("mock"), ladder)]);
        assert_eq!(
            find_private_type_leaks(&published).len(),
            1,
            "ripgrep's WalkParallel::run shape stays a finding"
        );
    }

    #[test]
    fn a_module_subtree_tells_the_two_pub_super_shapes_apart() {
        // The whole point of the rung (`internal/detection-gaps.md` §7) — the two field cases
        // below:
        //
        //   tokio    `task::state::unset_waker` is pub(super) — visible in the `task` subtree —
        //            and returns `UpdateResult`, private to `state.rs`. Its caller in
        //            `task::harness` can reach the method and cannot name the type. LEAK.
        //   ripgrep  `flags::parse::lookup` is pub(super) — visible in the `flags` subtree —
        //            and returns `Flag`, private to `flags/mod.rs`. Private in Rust is the
        //            module AND its descendants, so every caller that can reach `lookup` can
        //            name `Flag` too. NOT a leak.
        //
        // Same rungs, same shape, opposite verdicts — decided by where each region is anchored.
        use crate::adapter::{VisibilityRung, VisibilityScope};
        let ladder = vec![
            VisibilityRung {
                scope: VisibilityScope::Module,
                label: SmolStr::new("private"),
                surface_transitive: false,
            },
            VisibilityRung {
                scope: VisibilityScope::Module,
                label: SmolStr::new("pub(super)"),
                surface_transitive: false,
            },
        ];
        let edges = || {
            vec![type_use(
                SymbolId(0),
                SymbolId(1),
                span(1, 10, 1, 16),
                Confidence::Certain,
            )]
        };
        // The item is `pub(super)`, anchored one module up; the type is `private`, anchored on
        // its own module. Only the anchors differ between the two graphs.
        let graph_for = |type_anchor: &str, item_anchor: &str| {
            let mut symbols = vec![
                callable(FileId(0), "subject", 1, span(1, 1, 1, 40)),
                ty(FileId(0), "Leaked", 0),
            ];
            symbols[0].visible_in_unit = Some(SmolStr::new(item_anchor));
            symbols[1].visible_in_unit = Some(SmolStr::new(type_anchor));
            let mut graph = graph_with(symbols, edges())
                .with_visibility_ladders(vec![(SmolStr::new("mock"), ladder.clone())]);
            graph.files[0].unit = Some(SmolStr::new("a/b/c"));
            graph.files[0].unit_parent = Some(SmolStr::new("a/b"));
            graph
        };
        assert_eq!(
            find_private_type_leaks(&graph_for("a/b/c", "a/b")).len(),
            1,
            "tokio: the parent subtree reaches wider than the type's own module"
        );
        assert!(
            find_private_type_leaks(&graph_for("a/b", "a/b")).is_empty(),
            "ripgrep: the type's module already covers everyone the item promises"
        );
    }

    #[test]
    fn equal_scopes_anchored_in_different_files_still_leak() {
        // The scope buckets are relative to the symbol that owns them, so `File == File` does
        // not mean "same region": a public callable in a.mock naming a file-private type
        // declared in b.mock promises a type its consumers genuinely cannot name. Comparing
        // the ordinals alone called this safe.
        use crate::adapter::{VisibilityRung, VisibilityScope};
        let ladder = vec![
            VisibilityRung {
                scope: VisibilityScope::File,
                label: SmolStr::new("private"),
                surface_transitive: false,
            },
            VisibilityRung {
                scope: VisibilityScope::File,
                label: SmolStr::new("file-public"),
                surface_transitive: true,
            },
        ];
        let graph = ProjectGraph::for_test(
            vec![file("a.mock"), file("b.mock")],
            vec![
                callable(FileId(0), "F", 1, span(1, 1, 1, 40)),
                ty(FileId(1), "Hidden", 0),
            ],
            vec![],
            vec![type_use(
                SymbolId(0),
                SymbolId(1),
                span(1, 10, 1, 16),
                Confidence::Certain,
            )],
        )
        .with_visibility_ladders(vec![(SmolStr::new("mock"), ladder)]);
        assert_eq!(find_private_type_leaks(&graph).len(), 1);
    }

    #[test]
    fn a_non_certain_evidence_edge_never_accuses() {
        // Duck-typed/qualified-table fallback bindings (Probable/Possible)
        // routinely pick the wrong same-name type — a defect-group accusation needs certain
        // evidence (degrade toward silence).
        let symbols = vec![
            callable(FileId(0), "F", 1, span(1, 1, 1, 40)),
            ty(FileId(0), "secret", 0),
        ];
        let edges = vec![type_use(
            SymbolId(0),
            SymbolId(1),
            span(1, 10, 1, 16),
            Confidence::Probable,
        )];
        let findings = find_private_type_leaks(&graph_with(symbols, edges));
        assert!(findings.is_empty());
    }

    #[test]
    fn a_member_of_an_unexported_container_is_not_public_surface() {
        // gin's `timeCodec.Decode` shape: an exported-looking method on an
        // unexported receiver is not public API — no leak to accuse.
        let mut method = callable(FileId(0), "Decode", 1, span(3, 1, 3, 40));
        method.member_of = Some(smol_str::SmolStr::new("timeCodec"));
        method.span = span(3, 1, 5, 2);
        let mut receiver = ty(FileId(0), "timeCodec", 0);
        receiver.span = span(1, 1, 10, 2);
        let symbols = vec![method, receiver];
        let edges = vec![type_use(
            SymbolId(0),
            SymbolId(1),
            span(3, 10, 3, 16),
            Confidence::Certain,
        )];
        let findings = find_private_type_leaks(&graph_with(symbols, edges));
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn an_unresolvable_container_name_vouches_for_nothing() {
        // The `impl<M> Matcher for &M` shape: member_of names a generic parameter that
        // is no declaration — skip rather than accuse.
        let mut method = callable(FileId(0), "captures", 1, span(3, 1, 3, 40));
        method.member_of = Some(smol_str::SmolStr::new("M"));
        let symbols = vec![method, ty(FileId(0), "secret", 0)];
        let edges = vec![type_use(
            SymbolId(0),
            SymbolId(1),
            span(3, 10, 3, 16),
            Confidence::Certain,
        )];
        let findings = find_private_type_leaks(&graph_with(symbols, edges));
        assert!(findings.is_empty(), "{findings:?}");
    }
}
