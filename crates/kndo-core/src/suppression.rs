//! Suppression binding & marking (contracts/core-traits.md §2.1, RFC 0005 §12). Comment syntax
//! is language-defined, so adapters only *extract* `kndo:allow`/`kndo:allow-file` pragmas
//! (`FileFacts::suppressions`, carried onto `ProjectGraph::suppressions` by assembly); binding,
//! validation and marking are core logic here, identical across languages.
//!
//! **No-flicker guarantee:** [`apply`] runs strictly after `analysis::run_all` has already
//! computed the complete finding set as if no pragmas existed (contracts §2.1) — it only *marks*
//! matched findings (filtered from the report and `--fail-on`, still counted in
//! [`SuppressedSummary::inline`]), never influences what analyses themselves see.
//!
//! Deliberately not implemented yet, honestly absent rather than faked:
//! - `stale` findings for a pragma that binds to nothing, names an unknown category, or matches
//!   nothing — RFC 0005 §13 schedules the `stale` rule for M6; a pragma that doesn't bind here
//!   today simply suppresses nothing, silently.
//! - `category` validation against a registry (contracts §2.1 says the core validates category
//!   names) — no registry of valid category strings exists as a checkable list yet, so any
//!   category name is accepted as written.
//! - Config-based suppression (`kndo.toml` glob/category disables) — no config parser exists
//!   yet (RFC 0005 §12's second mechanism); [`SuppressedSummary::config`] stays honestly `0`.

use std::collections::HashMap;

use crate::adapter::{RawSuppression, SuppressionScope};
use crate::engine::{Finding, SuppressedSummary};
use crate::graph::ProjectGraph;
use crate::vocab::FileId;

enum Scope {
    File,
    /// Line-inclusive range of the bound declaration ("the symbol and everything it declares,"
    /// contracts §2.1) — any finding whose location falls inside covers both the anchor symbol
    /// itself and anything nested in it (e.g. a class's members), without needing to walk a
    /// symbol hierarchy: nested declarations are separate `SymbolNode`s with narrower spans.
    Declaration {
        start_line: u32,
        end_line: u32,
    },
}

struct Binding<'a> {
    file: FileId,
    category: &'a str,
    subject: Option<&'a str>,
    scope: Scope,
}

/// Binds every extracted pragma to what it covers, then filters findings that match a binding
/// out of `findings` — counting, never deleting the underlying fact (they're just not in the
/// returned set, same contract as [`crate::engine::Engine`]'s baseline filtering).
pub(crate) fn apply(
    graph: &ProjectGraph,
    findings: Vec<Finding>,
) -> (Vec<Finding>, SuppressedSummary) {
    if graph.suppressions.is_empty() {
        return (findings, SuppressedSummary::default());
    }

    let bindings: Vec<Binding> = graph
        .suppressions
        .iter()
        .filter_map(|(file, raw)| bind_one(graph, *file, raw))
        .collect();
    if bindings.is_empty() {
        return (findings, SuppressedSummary::default());
    }

    let file_by_path: HashMap<&str, FileId> = graph
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| (f.path.0.as_str(), FileId(i as u32)))
        .collect();

    let mut inline = 0usize;
    let kept = findings
        .into_iter()
        .filter(|f| {
            let Some(file) = f
                .location
                .path
                .as_ref()
                .and_then(|p| file_by_path.get(p.0.as_str()))
            else {
                return true; // no (resolvable) location to bind against — never suppressible
            };
            let suppressed = bindings.iter().any(|b| matches(b, *file, f));
            if suppressed {
                inline += 1;
            }
            !suppressed
        })
        .collect();

    (kept, SuppressedSummary { inline, config: 0 })
}

/// A `Declaration`-scope pragma "attaches to the declaration it precedes or shares a line with"
/// (contracts §2.1): the pragma's own line matches the declaration's start line (a trailing
/// same-line comment), or the declaration starts on the line right after the pragma ends (a
/// comment directly above it). `None` when nothing in the file qualifies — the pragma binds to
/// nothing (a future `stale` finding, not emitted yet — see module docs).
fn bind_one<'a>(
    graph: &'a ProjectGraph,
    file: FileId,
    raw: &'a RawSuppression,
) -> Option<Binding<'a>> {
    let scope = match raw.scope {
        SuppressionScope::File => Scope::File,
        SuppressionScope::Declaration => {
            let anchor = graph.symbols.iter().find(|s| {
                s.file == file
                    && (s.span.start.0 == raw.span.start.0 || s.span.start.0 == raw.span.end.0 + 1)
            })?;
            Scope::Declaration {
                start_line: anchor.span.start.0,
                end_line: anchor.span.end.0,
            }
        }
    };
    Some(Binding {
        file,
        category: raw.category.as_str(),
        subject: raw.subject.as_deref(),
        scope,
    })
}

fn matches(binding: &Binding, file: FileId, finding: &Finding) -> bool {
    if binding.file != file || binding.category != finding.category {
        return false;
    }
    if let Some(subject) = binding.subject {
        if subject != finding.subject_kind {
            return false;
        }
    }
    match &binding.scope {
        Scope::File => true,
        Scope::Declaration {
            start_line,
            end_line,
        } => finding
            .location
            .range
            .is_some_and(|r| r.start.0 >= *start_line && r.start.0 <= *end_line),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ProjectPath, Span, VisibilityLevel};
    use crate::engine::{Location, Severity};
    use crate::graph::{FileNode, SymbolNode};
    use crate::vocab::{Confidence, PackageId, SymbolKind};
    use smol_str::SmolStr;

    fn graph_with(
        symbols: Vec<SymbolNode>,
        suppressions: Vec<(FileId, RawSuppression)>,
    ) -> ProjectGraph {
        ProjectGraph::for_test(
            vec![FileNode {
                path: ProjectPath(SmolStr::new("a.ts")),
                content_hash: [0u8; 32],
                language: Some(SmolStr::new("mock")),
                class: None,
                package: PackageId(0),
                unit: None,
            }],
            symbols,
            vec![],
            vec![],
        )
        .with_suppressions(suppressions)
    }

    fn symbol(name: &str, start_line: u32, end_line: u32) -> SymbolNode {
        SymbolNode {
            file: FileId(0),
            name: SmolStr::new(name),
            kind: SymbolKind::Function,
            span: Span {
                start: (start_line, 1),
                end: (end_line, 1),
            },
            exported: false,
            visibility: VisibilityLevel(0),
            member_of: None,
            signature_span: None,
        }
    }

    fn suppression(
        category: &str,
        subject: Option<&str>,
        scope: SuppressionScope,
        start_line: u32,
        end_line: u32,
    ) -> RawSuppression {
        RawSuppression {
            span: Span {
                start: (start_line, 1),
                end: (end_line, 1),
            },
            category: SmolStr::new(category),
            subject: subject.map(SmolStr::new),
            reason: None,
            scope,
        }
    }

    fn finding_at(category: &str, subject_kind: &str, line: u32) -> Finding {
        Finding {
            id: format!("kndo-{category}-{line}"),
            category: category.to_string(),
            group: "waste".to_string(),
            subject_kind: subject_kind.to_string(),
            severity: Severity::Warning,
            confidence: Confidence::Certain,
            message: "example".to_string(),
            location: Location {
                path: Some(ProjectPath(SmolStr::new("a.ts"))),
                range: Some(Span {
                    start: (line, 1),
                    end: (line, 10),
                }),
                symbol: None,
                package: None,
            },
            delta: None,
            delta_origin: None,
        }
    }

    #[test]
    fn pragma_directly_above_a_declaration_suppresses_its_finding() {
        let graph = graph_with(
            vec![symbol("foo", 3, 5)],
            vec![(
                FileId(0),
                suppression("unused", None, SuppressionScope::Declaration, 2, 2),
            )],
        );
        let (kept, summary) = apply(&graph, vec![finding_at("unused", "function", 3)]);
        assert!(kept.is_empty());
        assert_eq!(summary.inline, 1);
        assert_eq!(summary.config, 0);
    }

    #[test]
    fn trailing_same_line_pragma_suppresses_its_finding() {
        let graph = graph_with(
            vec![symbol("foo", 3, 3)],
            vec![(
                FileId(0),
                suppression("unused", None, SuppressionScope::Declaration, 3, 3),
            )],
        );
        let (kept, summary) = apply(&graph, vec![finding_at("unused", "function", 3)]);
        assert!(kept.is_empty());
        assert_eq!(summary.inline, 1);
    }

    #[test]
    fn declaration_scope_covers_a_nested_member_within_the_same_span() {
        // class Baz { ... } spans lines 3-10; a member declared inside it (line 6) is a
        // separate SymbolNode but its finding still falls inside the anchor's line range.
        let graph = graph_with(
            vec![symbol("Baz", 3, 10)],
            vec![(
                FileId(0),
                suppression("unused", None, SuppressionScope::Declaration, 2, 2),
            )],
        );
        let (kept, summary) = apply(&graph, vec![finding_at("unused", "method", 6)]);
        assert!(kept.is_empty());
        assert_eq!(summary.inline, 1);
    }

    #[test]
    fn subject_facet_only_matches_the_named_subject_kind() {
        let graph = graph_with(
            vec![symbol("foo", 3, 3)],
            vec![(
                FileId(0),
                suppression(
                    "unused",
                    Some("enum-member"),
                    SuppressionScope::Declaration,
                    3,
                    3,
                ),
            )],
        );
        let (kept, summary) = apply(&graph, vec![finding_at("unused", "function", 3)]);
        assert_eq!(
            kept.len(),
            1,
            "subject facet must not match a different subject_kind"
        );
        assert_eq!(summary.inline, 0);
    }

    #[test]
    fn file_scope_suppresses_regardless_of_location() {
        let graph = graph_with(
            vec![],
            vec![(
                FileId(0),
                suppression("version-skew", None, SuppressionScope::File, 1, 1),
            )],
        );
        let (kept, summary) = apply(&graph, vec![finding_at("version-skew", "dependency", 40)]);
        assert!(kept.is_empty());
        assert_eq!(summary.inline, 1);
    }

    #[test]
    fn a_pragma_that_binds_to_nothing_suppresses_nothing() {
        // No declaration precedes or shares line 2 — the pragma is orphaned (a future `stale`
        // finding, not emitted yet).
        let graph = graph_with(
            vec![symbol("foo", 10, 10)],
            vec![(
                FileId(0),
                suppression("unused", None, SuppressionScope::Declaration, 2, 2),
            )],
        );
        let (kept, summary) = apply(&graph, vec![finding_at("unused", "function", 10)]);
        assert_eq!(kept.len(), 1);
        assert_eq!(summary.inline, 0);
    }

    #[test]
    fn a_different_category_is_not_suppressed() {
        let graph = graph_with(
            vec![symbol("foo", 3, 3)],
            vec![(
                FileId(0),
                suppression("unused", None, SuppressionScope::Declaration, 3, 3),
            )],
        );
        let (kept, summary) = apply(&graph, vec![finding_at("test-only", "function", 3)]);
        assert_eq!(kept.len(), 1);
        assert_eq!(summary.inline, 0);
    }

    #[test]
    fn no_suppressions_is_a_cheap_no_op() {
        let graph = graph_with(vec![], vec![]);
        let (kept, summary) = apply(&graph, vec![finding_at("unused", "function", 3)]);
        assert_eq!(kept.len(), 1);
        assert_eq!(summary.inline, 0);
        assert_eq!(summary.config, 0);
    }

    #[test]
    fn a_finding_with_no_location_path_is_never_suppressible() {
        let graph = graph_with(
            vec![],
            vec![(
                FileId(0),
                suppression("version-skew", None, SuppressionScope::File, 1, 1),
            )],
        );
        let mut f = finding_at("version-skew", "dependency", 1);
        f.location.path = None;
        let (kept, summary) = apply(&graph, vec![f]);
        assert_eq!(kept.len(), 1);
        assert_eq!(summary.inline, 0);
    }
}
