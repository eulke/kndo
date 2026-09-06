//! Dispatch: the engine deriving roots and exemptions from markers under the
//! claiming extension's rules — the one place a marker acquires meaning. An
//! adapter reports `#[test]` as a marker; its spec says a `test` marker roots
//! a Test entry; this module joins the two, for every language alike, so a
//! framework's attribute is a line of data and never a branch in an adapter.
//! A pure function of (evidence, rules): recomputed whenever a file's
//! evidence moves, stored on the graph beside the manifest anchors and never
//! in the evidence cache, so a rule change never has to re-extract anything.

use kndo_contract::evidence::{FileEvidence, Marker, MarkerTarget, Root, RootTarget};
use kndo_contract::extension::{DispatchRule, Effect};

/// What the rules derived for one file.
#[derive(Debug, Default)]
pub struct Dispatched {
    /// Roots the rules derived, sorted and deduplicated like manifest anchors.
    pub roots: Vec<Root>,
    /// Declarations the source exempts from the unused judgment, by index,
    /// sorted and deduplicated.
    pub exempt: Vec<u32>,
    /// What the run should say about this file: a blanket exemption is a fact
    /// worth a line in the report, never a silent hole in the findings.
    pub notes: Vec<String>,
}

pub fn apply(evidence: &FileEvidence, rules: &[DispatchRule]) -> Dispatched {
    let mut out = Dispatched::default();
    if rules.is_empty() || evidence.markers.is_empty() {
        return out;
    }
    for marker in &evidence.markers {
        for rule in rules {
            if !rule.when.matches(marker) {
                continue;
            }
            match rule.then {
                Effect::Root(kind) => {
                    let target = match &marker.on {
                        MarkerTarget::File => RootTarget::WholeFile,
                        MarkerTarget::Declaration(id) => RootTarget::Declaration(*id),
                        _ => continue,
                    };
                    out.roots.push(Root {
                        target,
                        kind,
                        confidence: rule.confidence,
                    });
                }
                Effect::Exempt => match &marker.on {
                    MarkerTarget::File => {
                        let n = evidence.declarations.len();
                        if n == 0 {
                            continue;
                        }
                        out.exempt.extend(0..n as u32);
                        out.notes.push(format!(
                            "`{}` at file level exempts every declaration here ({n}) from \
                             the unused judgment",
                            spell(marker)
                        ));
                    }
                    // Lexically scoped, as lint attributes are: the marked
                    // declaration and everything declared within its extent.
                    MarkerTarget::Declaration(id) => {
                        let outer = evidence.declarations[id.index()].span;
                        for (j, d) in evidence.declarations.iter().enumerate() {
                            let inside = d.span.start >= outer.start && d.span.end <= outer.end;
                            if j == id.index() || inside {
                                out.exempt.push(j as u32);
                            }
                        }
                    }
                    _ => {}
                },
            }
        }
    }
    let target_key = |r: &Root| match &r.target {
        RootTarget::Declaration(id) => id.index() as u32,
        _ => u32::MAX,
    };
    out.roots
        .sort_by_key(|r| (target_key(r), r.kind as u8, std::cmp::Reverse(r.confidence)));
    out.roots.dedup_by(|a, b| {
        target_key(a) == target_key(b) && a.kind == b.kind && a.confidence == b.confidence
    });
    out.exempt.sort_unstable();
    out.exempt.dedup();
    out
}

/// The marker as a reader would recognize it, language-neutral: `path` or
/// `path(arg, arg)`.
fn spell(marker: &Marker) -> String {
    if marker.args.is_empty() {
        marker.path.to_string()
    } else {
        format!("{}({})", marker.path, marker.args.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_contract::evidence::{
        EvidenceSink, EvidenceStream, EvidenceStreams, Reach, RootKind, SymbolKind,
    };
    use kndo_contract::extension::Trigger;
    use kndo_contract::vocab::{Confidence, Span};
    use smol_str::SmolStr;

    fn rules() -> Vec<DispatchRule> {
        vec![
            DispatchRule {
                when: Trigger::marker("test"),
                then: Effect::Root(RootKind::Test),
                confidence: Confidence::Certain,
            },
            DispatchRule {
                when: Trigger::marker("*::test"),
                then: Effect::Root(RootKind::Test),
                confidence: Confidence::Certain,
            },
            DispatchRule {
                when: Trigger::marker_with("allow", "dead_code"),
                then: Effect::Exempt,
                confidence: Confidence::Certain,
            },
        ]
    }

    fn sink() -> EvidenceSink {
        EvidenceSink::new(1000, EvidenceStreams::of(&[EvidenceStream::Markers]))
    }

    fn args(list: &[&str]) -> Vec<SmolStr> {
        list.iter().map(SmolStr::new).collect()
    }

    #[test]
    fn markers_derive_roots_by_rule_order_and_deduplicate() {
        let mut s = sink();
        let unit = s.declaration("unit", SymbolKind::Function, Span::new(0, 10), Reach::File);
        let plain = s.declaration(
            "plain",
            SymbolKind::Function,
            Span::new(20, 30),
            Reach::File,
        );
        s.marker(
            MarkerTarget::Declaration(unit),
            "test",
            vec![],
            Span::new(0, 7),
        );
        // Two attributes saying the same thing yield one root.
        s.marker(
            MarkerTarget::Declaration(unit),
            "tokio::test",
            vec![],
            Span::new(0, 7),
        );
        s.marker(
            MarkerTarget::Declaration(plain),
            "inline",
            vec![],
            Span::new(20, 28),
        );
        s.marker(MarkerTarget::File, "test", vec![], Span::new(0, 0));
        let d = apply(&s.finish(), &rules());
        assert_eq!(d.roots.len(), 2, "{:?}", d.roots);
        assert!(matches!(
            &d.roots[0],
            Root { target: RootTarget::Declaration(id), kind: RootKind::Test, .. } if *id == unit
        ));
        assert!(matches!(
            &d.roots[1],
            Root {
                target: RootTarget::WholeFile,
                kind: RootKind::Test,
                ..
            }
        ));
        assert!(d.exempt.is_empty() && d.notes.is_empty());
    }

    #[test]
    fn exemptions_are_lexically_scoped_and_a_blanket_is_said_aloud() {
        let mut s = sink();
        let outer = s.declaration("outer", SymbolKind::Module, Span::new(0, 100), Reach::File);
        let inner = s.declaration(
            "inner",
            SymbolKind::Function,
            Span::new(10, 40),
            Reach::File,
        );
        let beside = s.declaration(
            "beside",
            SymbolKind::Function,
            Span::new(120, 140),
            Reach::File,
        );
        s.marker(
            MarkerTarget::Declaration(outer),
            "allow",
            args(&["dead_code"]),
            Span::new(0, 5),
        );
        let ev = s.finish();
        let d = apply(&ev, &rules());
        assert_eq!(d.exempt, [outer.index() as u32, inner.index() as u32]);
        assert!(!d.exempt.contains(&(beside.index() as u32)));
        assert!(d.notes.is_empty());

        let mut s = sink();
        s.declaration("a", SymbolKind::Function, Span::new(0, 10), Reach::File);
        s.declaration("b", SymbolKind::Function, Span::new(20, 30), Reach::File);
        s.marker(
            MarkerTarget::File,
            "allow",
            args(&["dead_code", "unused_imports"]),
            Span::new(0, 5),
        );
        let d = apply(&s.finish(), &rules());
        assert_eq!(d.exempt, [0, 1]);
        assert_eq!(
            d.notes,
            [
                "`allow(dead_code, unused_imports)` at file level exempts every declaration here (2) from the unused judgment"
            ]
        );
    }

    #[test]
    fn no_rules_or_no_markers_derive_nothing() {
        let mut s = sink();
        let f = s.declaration("f", SymbolKind::Function, Span::new(0, 10), Reach::File);
        s.marker(
            MarkerTarget::Declaration(f),
            "test",
            vec![],
            Span::new(0, 7),
        );
        let ev = s.finish();
        let d = apply(&ev, &[]);
        assert!(d.roots.is_empty() && d.exempt.is_empty());
        let bare = sink().finish();
        let d = apply(&bare, &rules());
        assert!(d.roots.is_empty() && d.exempt.is_empty() && d.notes.is_empty());
    }
}
