//! Dispatch: the engine deriving roots and exemptions from markers under the
//! claiming extension's rules — the one place a marker acquires meaning. An
//! adapter reports `#[test]` as a marker; its spec says a `test` marker roots
//! a Test entry; this module joins the two, for every language alike, so a
//! framework's attribute is a line of data and never a branch in an adapter.
//! A pure function of (evidence, rules): recomputed whenever a file's
//! evidence moves, stored on the graph beside the manifest anchors and never
//! in the evidence cache, so a rule change never has to re-extract anything.
//!
//! Everything derived here names the rule that derived it ([`RuleId`]). That
//! attribution is not a diagnostic: it reaches `kndo describe` and a fixture's
//! `because`, which is what makes ablation a gate — remove a rule, and the
//! claims standing on it fail by name.

use kndo_contract::evidence::{FileEvidence, Marker, MarkerTarget, Root, RootTarget};
use kndo_contract::manifest::UnitKind;
use kndo_contract::plugin::{DeclarationCx, DispatchRule, Effect, PluginSpec, RuleId};
use smol_str::SmolStr;
use std::collections::BTreeMap;

/// The rules one claiming plugin is dispatched with: its own, in its own
/// order, then every ACTIVE rule pack's, in composition order. Each keeps the
/// identity of the spec that declares it, so a pack's rule is `kndo:xctest#0`
/// whichever language it rides into.
#[derive(Debug, Default, Clone)]
pub struct Rules(Vec<(RuleId, DispatchRule)>);

impl Rules {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Every rule a spec declares, in the order the spec writes them.
    pub fn of(spec: &PluginSpec) -> Rules {
        let mut rules = Rules::default();
        rules.extend(spec);
        rules
    }

    pub fn extend(&mut self, spec: &PluginSpec) {
        self.declared_by(spec.coordinate(), spec.dispatch_rules());
    }

    fn declared_by(&mut self, coordinate: &str, rules: &[DispatchRule]) {
        self.0.extend(
            rules
                .iter()
                .enumerate()
                .map(|(i, r)| (RuleId::new(coordinate, i as u32), r.clone())),
        );
    }

    fn iter(&self) -> impl Iterator<Item = &(RuleId, DispatchRule)> {
        self.0.iter()
    }
}

/// A root a rule derived, beside the rule that derived it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DerivedRoot {
    pub root: Root,
    pub by: RuleId,
}

/// A declaration a rule lifts out of the unused judgment.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Exemption {
    pub decl: u32,
    pub by: RuleId,
}

/// A declaration a rule made a witness, with the base whose surface it
/// satisfies as the rule spells it — see
/// [`kndo_contract::plugin::Effect::Witness`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Witnessed {
    pub decl: u32,
    pub base: SmolStr,
    pub by: RuleId,
}

/// What the rules derived for one file.
#[derive(Debug, Default)]
pub struct Dispatched {
    /// Roots the rules derived, sorted and deduplicated like manifest anchors.
    pub roots: Vec<DerivedRoot>,
    /// Declarations the source exempts from the unused judgment, by index,
    /// sorted and deduplicated.
    pub exempt: Vec<Exemption>,
    /// What the run should say about this file: a blanket exemption is a fact
    /// worth a line in the report, never a silent hole in the findings.
    pub notes: Vec<String>,
    /// A generator owns this file — see
    /// [`kndo_contract::plugin::Effect::Generated`].
    pub generated: bool,
    /// Declarations a rule made witnesses, sorted by index.
    pub witnesses: Vec<Witnessed>,
}

/// How a file stands to the unit that compiles it — the one thing that decides
/// how far a [`MarkerTarget::Unit`] marker reaches. The engine's to state, and
/// only the engine's: extraction reads one file and has never seen the manifest
/// that says which file the build enters a unit through. One type rather than a
/// flag beside a list, because "does this file speak for its unit" and "what
/// its unit said" are halves of one fact and must not be able to disagree.
pub enum UnitVoice<'m> {
    /// The build enters a unit here: what this file claims for the unit IS the
    /// unit's, and reaches every file the unit compiles.
    Entry,
    /// Another file the unit compiles, carrying what its entry claimed. A unit
    /// claim written HERE is this file's own and reaches no further.
    Member(&'m [Marker]),
    /// No unit compiles this file: nothing claims over it, and its own unit
    /// claim is its own.
    Unstated,
}

impl<'m> UnitVoice<'m> {
    fn carried(&self) -> &'m [Marker] {
        match self {
            UnitVoice::Member(markers) => markers,
            _ => &[],
        }
    }

    /// The word a blanket this file wrote is reported with: a unit's statement
    /// on the file the build enters it through, and the file's own anywhere
    /// else.
    fn level_of_own(&self) -> &'static str {
        match self {
            UnitVoice::Entry => "unit",
            _ => "file",
        }
    }
}

/// What one file's rules derive from its own markers and from what its unit
/// said — see [`UnitVoice`]. A file that IS its unit's entry carries none: its own
/// markers already hold them, and one marker read twice credits two rules to
/// one declaration.
pub fn apply(
    evidence: &FileEvidence,
    voice: &UnitVoice<'_>,
    supertypes: &BTreeMap<SmolStr, Vec<SmolStr>>,
    rules: &Rules,
) -> Dispatched {
    let mut out = Dispatched::default();
    let carried = voice.carried();
    if rules.is_empty() || (evidence.markers.is_empty() && carried.is_empty()) {
        return out;
    }
    let cx = DeclarationCx {
        evidence,
        compiled_into: None,
        supertypes,
    };
    let markers = evidence
        .markers
        .iter()
        .map(|m| (m, voice.level_of_own()))
        // What the entry claimed for the unit arrives here already honored.
        .chain(carried.iter().map(|m| (m, "unit")));
    for (marker, level) in markers {
        for (id, rule) in rules.iter() {
            if !rule.when.matches(&cx, marker) {
                continue;
            }
            match rule.then {
                Effect::Root(kind) => {
                    let target = match &marker.on {
                        MarkerTarget::File | MarkerTarget::Unit => RootTarget::WholeFile,
                        MarkerTarget::Declaration(id) => RootTarget::Declaration(*id),
                        _ => continue,
                    };
                    out.roots.push(DerivedRoot {
                        root: Root {
                            target,
                            kind,
                            confidence: rule.confidence,
                        },
                        by: id.clone(),
                    });
                }
                Effect::Generated => {
                    if !matches!(marker.on, MarkerTarget::File | MarkerTarget::Unit) {
                        continue;
                    }
                    if !out.generated {
                        out.generated = true;
                        out.notes.push(format!(
                            "`{}` marks this file a generator's output: what it declares is \
                             not judged, what it imports and names still is",
                            spell(marker)
                        ));
                    }
                }
                Effect::Witness => {
                    if let MarkerTarget::Declaration(decl) = &marker.on {
                        out.witnesses.push(Witnessed {
                            decl: decl.index() as u32,
                            base: spell_path(marker),
                            by: id.clone(),
                        });
                    }
                }
                Effect::Exempt => match &marker.on {
                    MarkerTarget::File | MarkerTarget::Unit => {
                        let n = evidence.declarations.len();
                        if n == 0 {
                            continue;
                        }
                        out.exempt.extend((0..n as u32).map(|decl| Exemption {
                            decl,
                            by: id.clone(),
                        }));
                        let level = match marker.on {
                            MarkerTarget::Unit => level,
                            _ => "file",
                        };
                        out.notes.push(format!(
                            "`{}` at {level} level exempts every declaration here ({n}) from \
                             the unused judgment",
                            spell(marker)
                        ));
                    }
                    // Lexically scoped, as lint attributes are: the marked
                    // declaration and everything declared within its extent.
                    MarkerTarget::Declaration(decl) => {
                        let outer = evidence.declarations[decl.index()].span;
                        for (j, d) in evidence.declarations.iter().enumerate() {
                            let inside = d.span.start >= outer.start && d.span.end <= outer.end;
                            if j == decl.index() || inside {
                                out.exempt.push(Exemption {
                                    decl: j as u32,
                                    by: id.clone(),
                                });
                            }
                        }
                    }
                    _ => {}
                },
            }
        }
    }
    sort_roots(&mut out.roots);
    sort_exemptions(&mut out.exempt);
    sort_witnesses(&mut out.witnesses);
    out
}

/// Every supertype edge the project declares, by NAME — what an
/// `ExternalWitness` rule walks. Names on both sides: the base such a rule
/// names is the one the project does not declare, so it is only ever a
/// target.
pub fn supertype_edges<'a>(
    files: impl Iterator<Item = &'a FileEvidence>,
) -> BTreeMap<SmolStr, Vec<SmolStr>> {
    let mut out: BTreeMap<SmolStr, Vec<SmolStr>> = BTreeMap::new();
    for ev in files {
        for r in &ev.relations {
            let from = ev.declarations[r.from.index()].name.clone();
            out.entry(from).or_default().push(r.to.name.clone());
        }
    }
    for supers in out.values_mut() {
        supers.sort();
        supers.dedup();
    }
    out
}

/// What the DECLARATION-shaped rules derive: roots from a name a runner or a
/// runtime calls, witnesses from a surface an owner promised. Separate from
/// [`apply`] because the qualifier these rules read is the file's ROLE, which
/// the project states and only the assembled graph knows: what a unit's kind
/// and a declared file role say lands as a root on the file.
pub fn declaration_effects(
    evidence: &FileEvidence,
    compiled_into: Option<UnitKind>,
    supertypes: &BTreeMap<SmolStr, Vec<SmolStr>>,
    rules: &Rules,
) -> (Vec<DerivedRoot>, Vec<Witnessed>) {
    let mut roots = Vec::new();
    let mut witnesses = Vec::new();
    if rules.is_empty() {
        return (roots, witnesses);
    }
    let cx = DeclarationCx {
        evidence,
        compiled_into,
        supertypes,
    };
    for (decl, _) in evidence.declarations_with_ids() {
        for (id, rule) in rules.iter() {
            if !rule.when.matches_declaration(&cx, decl) {
                continue;
            }
            match rule.then {
                Effect::Root(kind) => roots.push(DerivedRoot {
                    root: Root {
                        target: RootTarget::Declaration(decl),
                        kind,
                        confidence: rule.confidence,
                    },
                    by: id.clone(),
                }),
                Effect::Witness => witnesses.push(Witnessed {
                    decl: decl.index() as u32,
                    base: witness_base(&rule.when),
                    by: id.clone(),
                }),
                Effect::Exempt | Effect::Generated => {}
            }
        }
    }
    sort_roots(&mut roots);
    sort_witnesses(&mut witnesses);
    (roots, witnesses)
}

/// The name a reader recognizes behind a witness: the base a rule named, or
/// the rule's own subject where it named none.
fn witness_base(trigger: &kndo_contract::plugin::Trigger) -> SmolStr {
    use kndo_contract::plugin::Trigger;
    match trigger {
        Trigger::ExternalWitness { base, .. } | Trigger::Relation { to: base, .. } => base.clone(),
        // The rule named the base on the OWNER; that is the name a reader
        // recognizes, not the member's own.
        Trigger::MemberOf { owner, name } => match owner.as_ref() {
            Trigger::MemberOf { .. } => name.clone(),
            other => witness_base(other),
        },
        Trigger::Marker { path, .. } | Trigger::Name { pattern: path, .. } => path.clone(),
    }
}

/// One witness per declaration, and the same one however the rules were
/// ordered: by index, then by the base's spelling, then by the rule — the
/// first survives.
pub fn sort_witnesses(witnesses: &mut Vec<Witnessed>) {
    witnesses.sort_by(|a, b| (a.decl, &a.base, &a.by).cmp(&(b.decl, &b.base, &b.by)));
    witnesses.dedup_by_key(|w| w.decl);
}

/// One exemption per declaration: the earliest rule that lifted it out.
fn sort_exemptions(exempt: &mut Vec<Exemption>) {
    exempt.sort_by_key(|e| e.decl);
    exempt.dedup_by_key(|e| e.decl);
}

/// The one order derived roots are held in — by target, then color, then
/// strongest confidence first — so a graph is byte-identical however its
/// roots were derived. The rule is NOT part of the key: two rules deriving
/// the same root are one root, credited to the earlier rule.
pub fn sort_roots(roots: &mut Vec<DerivedRoot>) {
    let target_key = |r: &DerivedRoot| match &r.root.target {
        RootTarget::Declaration(id) => id.index() as u32,
        _ => u32::MAX,
    };
    roots.sort_by_key(|r| {
        (
            target_key(r),
            r.root.kind as u8,
            std::cmp::Reverse(r.root.confidence),
        )
    });
    roots.dedup_by(|a, b| {
        target_key(a) == target_key(b)
            && a.root.kind == b.root.kind
            && a.root.confidence == b.root.confidence
    });
}

/// The marker's path alone — what a witness derived from one is spelled by.
fn spell_path(marker: &Marker) -> SmolStr {
    marker.path.clone()
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
    use kndo_contract::plugin::Trigger;
    use kndo_contract::vocab::{Confidence, Span};
    use smol_str::SmolStr;

    fn rules() -> Rules {
        let mut out = Rules::default();
        out.declared_by(
            "kndo:mock",
            &[
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
            ],
        );
        out
    }

    fn sink() -> EvidenceSink {
        EvidenceSink::new(1000, EvidenceStreams::of(&[EvidenceStream::Markers]))
    }

    fn args(list: &[&str]) -> Vec<SmolStr> {
        list.iter().map(SmolStr::new).collect()
    }

    fn exempted(d: &Dispatched) -> Vec<u32> {
        d.exempt.iter().map(|e| e.decl).collect()
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
        let d = apply(
            &s.finish(),
            &UnitVoice::Unstated,
            &BTreeMap::new(),
            &rules(),
        );
        assert_eq!(d.roots.len(), 2, "{:?}", d.roots);
        // Two attributes, one root — credited to the EARLIER rule, so the
        // derivation is a function of the rule list and not of the source's
        // attribute order.
        assert!(matches!(
            &d.roots[0],
            DerivedRoot { root: Root { target: RootTarget::Declaration(id), kind: RootKind::Test, .. }, by }
                if *id == unit && by.to_string() == "kndo:mock#0"
        ));
        assert!(matches!(
            &d.roots[1],
            DerivedRoot {
                root: Root {
                    target: RootTarget::WholeFile,
                    kind: RootKind::Test,
                    ..
                },
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
        let d = apply(&ev, &UnitVoice::Unstated, &BTreeMap::new(), &rules());
        assert_eq!(exempted(&d), [outer.index() as u32, inner.index() as u32]);
        assert!(!exempted(&d).contains(&(beside.index() as u32)));
        assert!(d.exempt.iter().all(|e| e.by.to_string() == "kndo:mock#2"));
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
        let d = apply(
            &s.finish(),
            &UnitVoice::Unstated,
            &BTreeMap::new(),
            &rules(),
        );
        assert_eq!(exempted(&d), [0, 1]);
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
        let d = apply(
            &ev,
            &UnitVoice::Unstated,
            &BTreeMap::new(),
            &Rules::default(),
        );
        assert!(d.roots.is_empty() && d.exempt.is_empty());
        let bare = sink().finish();
        let d = apply(&bare, &UnitVoice::Unstated, &BTreeMap::new(), &rules());
        assert!(d.roots.is_empty() && d.exempt.is_empty() && d.notes.is_empty());
    }
}
