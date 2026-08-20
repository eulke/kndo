//! `private-type-leak` — declared visibility *below* what usage requires (RFC 0005 §7, group
//! `defect`): the second half of the visibility-mismatch pair (`internal-only` is the other
//! direction). "A public/exported symbol whose signature references a type of *lower*
//! visibility — the API promises a type its consumers cannot name."
//!
//! Built exactly on RFC 0012 §5's two facts and nothing else: a `TypeUse`-kinded `References`
//! edge attributed (`within`, §4) to the exported declaration, whose site span lies inside
//! that declaration's `signature_span`. Body-internal type uses are not leaks (a private type
//! used *inside* a public function is ordinary encapsulation); only the signature is a
//! promise. v1 is deliberately callables-only — `signature_span` is `None` on type
//! declarations until fields exist as member declarations with their own visibility, because
//! firing on a whole struct body would falsely accuse exported-struct/unexported-field shapes
//! (RFC 0012 §2: degrade toward silence, never toward accusation).
//!
//! Severity per RFC 0005 §7: warning in library-mode packages (a lying public API), info in
//! app packages — the package's publish signal (`PackageNode::private`, RFC 0011 §5) is the
//! mode. Confidence: the evidence edge's own confidence. Cross-language pairs are skipped —
//! visibility levels only mean anything *within one language's ladder* (RFC 0012 §6) and
//! comparing them across languages would be numerology. "Lower visibility" is compared as
//! ladder rung *scopes* when the language declared a ladder (so same-scope rungs like Java
//! `protected`/`public` never accuse each other), raw indices otherwise.

use std::collections::HashMap;

use crate::adapter::Span;
use crate::analysis::finding_id;
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::{EdgeKind, FileOrigin, NodeRef, RefKind, SymbolId};

fn contains(outer: &Span, inner: &Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

pub fn find_private_type_leaks(graph: &ProjectGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    // One finding per (declaration, leaked type) pair, however many signature sites repeat it.
    let mut seen: HashMap<(SymbolId, SymbolId), ()> = HashMap::new();

    for edge in &graph.edges {
        let EdgeKind::References {
            from: NodeRef::Symbol(decl_id),
            to: type_id,
            kind: RefKind::TypeUse,
        } = edge.kind
        else {
            continue;
        };
        let Some(site) = edge.span else { continue };

        let decl = &graph.symbols[decl_id.0 as usize];
        let Some(sig) = decl.signature_span else {
            continue; // no declared signature — nothing to distinguish from the body
        };
        if !decl.exported || !contains(&sig, &site) {
            continue;
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
        if decl_file.language != leaked_file.language {
            continue; // visibility levels only compare within one language (module doc)
        }
        // "Lower visibility" via the language's ladder when it's declared (RFC 0012 §6):
        // comparing *scopes* — not raw indices — means two rungs sharing a scope (Java
        // `protected`/`public`, both `Public` by the conservative-mapping rule) never accuse
        // each other. Fall back to index comparison when no ladder covers the levels — the
        // pre-ladder behavior, still meaningful within one language.
        let ladder = decl_file
            .language
            .as_deref()
            .and_then(|lang| graph.ladder_for(lang));
        let leaks = match ladder.map(|l| {
            (
                l.get(leaked.visibility.0 as usize),
                l.get(decl.visibility.0 as usize),
            )
        }) {
            Some((Some(leaked_rung), Some(decl_rung))) => leaked_rung.scope < decl_rung.scope,
            _ => leaked.visibility < decl.visibility,
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
        // Library vs app mode (RFC 0011 §5): the publish signal of the *declaring* package.
        let is_library = graph
            .packages
            .get(decl_file.package.0 as usize)
            .map(|p| !p.private)
            .unwrap_or(false);
        findings.push(Finding {
            id: finding_id("private-type-leak", facet, path, &qualified, &leaked_name),
            category: "private-type-leak".to_string(),
            group: "defect".to_string(),
            subject_kind: facet.to_string(),
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
        }
    }

    fn type_use(from: SymbolId, to: SymbolId, site: Span, confidence: Confidence) -> Edge {
        Edge {
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
        assert_eq!(findings[0].group, "defect");
        assert!(findings[0].message.contains("secret"));
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
        // to `Public` scope by RFC 0012 §6's conservative rule, so index inequality alone
        // must not accuse.
        use crate::adapter::{VisibilityRung, VisibilityScope};
        let rung = |scope, label: &str| VisibilityRung {
            scope,
            label: SmolStr::new(label),
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
    fn confidence_is_the_evidence_edge_s_own() {
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
        assert_eq!(findings[0].confidence, Confidence::Probable);
    }
}
