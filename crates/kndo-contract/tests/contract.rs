use kndo_contract::evidence::{
    DiagnosticLevel, EvidenceSink, EvidenceStream, EvidenceStreams, FunctionMetrics, MarkerTarget,
    Reach, RefKind, RootKind, RootTarget, SymbolKind,
};
use kndo_contract::subject::{FindingId, Subject, SymbolSelector};
use kndo_contract::vocab::{Category, Confidence, ProjectPath, Span, SubjectKind};

#[test]
fn sink_attaches_metrics_and_membership_by_id() {
    let mut sink = EvidenceSink::new(100, EvidenceStreams::of(&[EvidenceStream::Metrics]));
    let owner = sink.declaration(
        "Widget",
        SymbolKind::Type,
        Span::new(0, 40),
        Reach::Exported,
    );
    let method = sink.declaration("draw", SymbolKind::Method, Span::new(10, 30), Reach::File);
    sink.member_of(method, owner);
    sink.metrics(
        method,
        FunctionMetrics {
            cyclomatic: 3,
            loc: 5,
            token_count: 40,
            fingerprints: vec![1, 2],
        },
    );
    sink.reference("helper", RefKind::Call, Span::new(12, 18));
    sink.root(
        RootTarget::Declaration(owner),
        RootKind::Production,
        kndo_contract::vocab::Confidence::Certain,
    );

    let ev = sink.finish();
    assert_eq!(ev.declarations[method.index()].owner, Some(owner));
    assert_eq!(ev.metrics.len(), 1);
    assert_eq!(ev.metrics[0].0, method);
    assert!(ev.diagnostics.is_empty());
}

#[test]
fn sink_degrades_on_a_bad_span_instead_of_failing() {
    let mut sink = EvidenceSink::new(10, EvidenceStreams::none());
    let d = sink.declaration("x", SymbolKind::Function, Span::new(4, 99), Reach::File);
    sink.diagnostic(DiagnosticLevel::Info, "note", None);
    let ev = sink.finish();
    assert_eq!(
        ev.declarations[d.index()].span,
        Span::new(4, 10),
        "clamped to file length"
    );
    assert!(
        ev.diagnostics
            .iter()
            .any(|d| d.level == DiagnosticLevel::Warn && d.message.contains("clamped")),
        "the clamp is reported, not silent"
    );
}

#[test]
fn undeclared_stream_writes_are_dropped_and_reported() {
    // The pairing rule at the sink: this adapter never declared Comments, so the
    // write is dropped with a Warn — the declaration stays truthful and analyses can
    // trust `declared` to abstain instead of guessing.
    let mut sink = EvidenceSink::new(50, EvidenceStreams::none());
    sink.comment(Span::new(0, 10), Span::new(2, 8));
    let ev = sink.finish();
    assert!(ev.comments.is_empty(), "undeclared write dropped");
    assert!(
        ev.diagnostics
            .iter()
            .any(|d| d.level == DiagnosticLevel::Warn && d.message.contains("undeclared stream")),
        "and reported"
    );
    assert!(!ev.declared.contains(EvidenceStream::Comments));
}

#[test]
fn declared_stream_writes_flow_through() {
    let mut sink = EvidenceSink::new(50, EvidenceStreams::of(&[EvidenceStream::Comments]));
    sink.comment(Span::new(0, 10), Span::new(2, 8));
    let ev = sink.finish();
    assert_eq!(ev.comments.len(), 1);
    assert!(ev.diagnostics.is_empty());
    assert!(ev.declared.contains(EvidenceStream::Comments));
}

#[test]
fn subject_derives_kind_and_identity_ignores_spans() {
    let subject = |span| Subject::Symbol {
        path: ProjectPath::new("src/lib.rs"),
        selector: SymbolSelector::member("Widget", "draw"),
        span,
    };
    let a = subject(Span::new(0, 10));
    let b = subject(Span::new(50, 60));
    assert_eq!(a.kind(), SubjectKind::Symbol);

    let id_a = FindingId::derive(&Category::UNUSED, &a, "");
    let id_b = FindingId::derive(&Category::UNUSED, &b, "");
    assert_eq!(id_a, id_b, "moving code must not change identity");

    let other_cat = FindingId::derive(&Category::UNTESTED, &a, "");
    let other_disc = FindingId::derive(&Category::UNUSED, &a, "nested#1");
    assert_ne!(id_a, other_cat);
    assert_ne!(id_a, other_disc);
    assert!(id_a.as_str().starts_with("kndo-"));
}

#[test]
fn plugin_categories_are_namespaced() {
    let c = Category::extension("github.com/acme/kndo-x", "no-foo");
    assert!(c.is_extension());
    assert_eq!(c.as_str(), "ext:github.com/acme/kndo-x/no-foo");
    assert!(!Category::UNUSED.is_extension());
}

#[test]
fn contract_fingerprint_is_stable_within_a_build() {
    assert_eq!(
        kndo_contract::contract_fingerprint_hex(),
        kndo_contract::contract_fingerprint_hex()
    );
}

#[test]
fn declaring_extensions_is_claiming_them() {
    use kndo_contract::extension::ExtensionSpec;
    let spec = ExtensionSpec::builder("demo", 1)
        .suffixes(&["ts", "tsx"])
        .claims(&["**/special.conf"])
        .build();
    // One declaration: the extension list is queryable AND the claim globs derive
    // from it, in order, with non-extension claims preserved alongside.
    assert_eq!(spec.suffixes(), ["ts", "tsx"]);
    assert_eq!(spec.claims(), ["**/*.ts", "**/*.tsx", "**/special.conf"]);
}

#[test]
fn as_str_spellings_are_the_serde_spellings() {
    use kndo_contract::finding::Severity;
    use kndo_contract::vocab::Confidence;
    // `as_str` exists so no frontend keeps its own severity/confidence table;
    // this is the tie that keeps it honest against what the envelope writes.
    for s in [Severity::Error, Severity::Warning, Severity::Info] {
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, format!("\"{}\"", s.as_str()));
    }
    for c in [
        Confidence::Possible,
        Confidence::Probable,
        Confidence::Certain,
    ] {
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, format!("\"{}\"", c.as_str()));
    }
}

#[test]
fn markers_ride_their_declared_stream() {
    // Declared: a marker on a declaration or the file lands as written.
    let mut sink = EvidenceSink::new(80, EvidenceStreams::of(&[EvidenceStream::Markers]));
    let f = sink.declaration("f", SymbolKind::Function, Span::new(10, 30), Reach::File);
    sink.marker(
        MarkerTarget::Declaration(f),
        "tokio::test",
        vec!["flavor = \"multi_thread\"".into()],
        Span::new(0, 9),
    );
    sink.marker(
        MarkerTarget::File,
        "allow",
        vec!["dead_code".into()],
        Span::new(40, 60),
    );
    let ev = sink.finish();
    assert_eq!(ev.markers.len(), 2);
    assert_eq!(ev.markers[0].path, "tokio::test");
    assert_eq!(ev.markers[0].args, ["flavor = \"multi_thread\""]);
    assert_eq!(ev.markers[0].on, MarkerTarget::Declaration(f));
    assert_eq!(ev.markers[1].on, MarkerTarget::File);
    assert!(ev.diagnostics.is_empty());

    // Undeclared: the write drops with a diagnostic, the pairing rule's teeth.
    let mut sink = EvidenceSink::new(80, EvidenceStreams::none());
    sink.marker(MarkerTarget::File, "allow", vec![], Span::new(0, 5));
    let ev = sink.finish();
    assert!(ev.markers.is_empty());
    assert!(
        ev.diagnostics
            .iter()
            .any(|d| d.message.contains("undeclared stream Markers")),
        "{:?}",
        ev.diagnostics
    );
}

#[test]
fn a_selector_is_unique_within_its_file_by_construction() {
    use kndo_contract::evidence::{EvidenceStreams, Reach, SymbolKind};
    let mut sink = EvidenceSink::new(400, EvidenceStreams::none());
    let widget = sink.declaration(
        "Widget",
        SymbolKind::Type,
        Span::new(0, 400),
        Reach::Exported,
    );
    // Two overloads the language tells apart by signature, a field of the
    // same name it tells apart by having none, and a Python-style
    // redefinition nothing but position tells apart.
    let by_int = sink.declaration(
        "size",
        SymbolKind::Method,
        Span::new(10, 40),
        Reach::Exported,
    );
    sink.signature(by_int, "(int)");
    let by_str = sink.declaration(
        "size",
        SymbolKind::Method,
        Span::new(50, 90),
        Reach::Exported,
    );
    sink.signature(by_str, "(String)");
    let field = sink.declaration(
        "size",
        SymbolKind::Variable,
        Span::new(100, 110),
        Reach::File,
    );
    let first = sink.declaration(
        "helper",
        SymbolKind::Function,
        Span::new(200, 250),
        Reach::File,
    );
    let again = sink.declaration(
        "helper",
        SymbolKind::Function,
        Span::new(300, 350),
        Reach::File,
    );
    for id in [by_int, by_str, field] {
        sink.member_of(id, widget);
    }
    let ev = sink.finish();

    let render = |id| ev.selector_of(id).render();
    assert_eq!(render(by_int), "Widget.size(int)");
    assert_eq!(render(by_str), "Widget.size(String)");
    assert_eq!(
        render(field),
        "Widget.size",
        "no signature: the field keeps the bare spelling"
    );
    assert_eq!(render(first), "helper");
    assert_eq!(
        render(again),
        "helper#2",
        "position, when the language spells nothing else"
    );

    let path = ProjectPath::new("src/Widget.java");
    let ids: std::collections::BTreeSet<FindingId> = [by_int, by_str, field, first, again]
        .into_iter()
        .map(|id| FindingId::derive(&Category::UNUSED, &ev.subject_of(&path, id), ""))
        .collect();
    assert_eq!(ids.len(), 5, "five declarations, five identities");
}

#[test]
fn an_import_written_twice_is_two_subjects() {
    use kndo_contract::evidence::{EvidenceStreams, ImportShape, ImportTarget};
    let mut sink = EvidenceSink::new(100, EvidenceStreams::none());
    for start in [0, 30] {
        sink.import(
            ImportTarget::Relative("./x".into()),
            ImportShape::SideEffect,
            Span::new(start, start + 10),
            Confidence::Certain,
        );
    }
    sink.import(
        ImportTarget::Relative("./y".into()),
        ImportShape::SideEffect,
        Span::new(60, 70),
        Confidence::Certain,
    );
    let ev = sink.finish();
    assert_eq!(
        [ev.imports[0].nth, ev.imports[1].nth, ev.imports[2].nth],
        [0, 1, 0]
    );

    let path = ProjectPath::new("src/a.js");
    let first = ev.import_subject(&path, 0);
    let again = ev.import_subject(&path, 1);
    assert_eq!(first.label(), "import './x'");
    assert_eq!(again.label(), "import './x' #2");
    assert_ne!(
        FindingId::derive(&Category::UNRESOLVED, &first, ""),
        FindingId::derive(&Category::UNRESOLVED, &again, ""),
        "two statements, two identities"
    );
}

#[test]
fn a_suppression_carries_what_it_allows_and_its_position() {
    let allow = |nth| Subject::Suppression {
        path: ProjectPath::new("src/a.js"),
        categories: vec![Category::UNUSED],
        nth,
        span: Span::new(0, 10),
    };
    assert_eq!(allow(0).label(), "allow unused");
    assert_eq!(allow(1).label(), "allow unused #2");
    assert_eq!(allow(0).render(), "src/a.js — allow unused");
    assert_ne!(
        FindingId::derive(&Category::STALE, &allow(0), ""),
        FindingId::derive(&Category::STALE, &allow(1), ""),
        "the position is identity, with no discriminator to carry it"
    );
    let other = Subject::Suppression {
        path: ProjectPath::new("src/a.js"),
        categories: vec![Category::DUPLICATE],
        nth: 0,
        span: Span::new(0, 10),
    };
    assert_ne!(
        FindingId::derive(&Category::STALE, &allow(0), ""),
        FindingId::derive(&Category::STALE, &other, ""),
        "what an allow allows is part of what it is"
    );
}

#[test]
fn a_ladder_names_the_narrowest_step_a_declaration_can_take() {
    use kndo_contract::extension::{Bearer, Ladder, PublishedSurface, Rung, Step};
    // Kotlin's shape: `private` twice — the class on a member, the file on a
    // top-level declaration — then `internal`, then `public`.
    let ladder = Ladder::new(vec![
        Step::for_members(Rung::Owner, "private"),
        Step::for_free(Rung::File, "private"),
        Step::new(Rung::Unit, "internal"),
        Step::new(Rung::Exported, "public"),
    ]);
    fn word(s: Option<&Step>) -> Option<&str> {
        s.map(|s| s.word.as_str())
    }
    // A member used only inside its class falls to the class's `private`.
    assert_eq!(
        word(ladder.step_down(Rung::Unit, Rung::Owner, true)),
        Some("private")
    );
    // A top-level declaration used only in its file falls to the file's.
    assert_eq!(
        word(ladder.step_down(Rung::Unit, Rung::File, false)),
        Some("private")
    );
    // A member used elsewhere in its file has no keyword between the two.
    assert_eq!(ladder.step_down(Rung::Unit, Rung::File, true), None);
    // Nothing falls to its own rung or above it.
    assert_eq!(ladder.step_down(Rung::Unit, Rung::Unit, false), None);
    assert_eq!(ladder.step_down(Rung::File, Rung::Unit, false), None);
    // A rung the language spelled in its evidence but left off its ladder
    // still has a word — the engine's own, never an empty one.
    assert_eq!(ladder.word(Rung::Unit), "internal");
    assert_eq!(ladder.word(Rung::Namespace), "namespace");
    assert!(Bearer::Any.admits(true) && Bearer::Any.admits(false));
    assert!(Bearer::Free.admits(false) && !Bearer::Free.admits(true));
    assert!(Bearer::Member.admits(true) && !Bearer::Member.admits(false));
    // The defaults are silence: no ladder, and every export published.
    assert!(Ladder::default().is_empty());
    assert_eq!(PublishedSurface::default(), PublishedSurface::Exports);
}

#[test]
fn a_regions_writes_land_in_the_files_coordinates_and_never_nest() {
    use kndo_contract::evidence::{
        EvidenceSink, EvidenceStream, EvidenceStreams, ImportShape, ImportTarget, Reach,
        RegionMode, SymbolKind,
    };
    use kndo_contract::vocab::{Confidence, Span};
    // A 100-byte host declaring no optional stream, with a region at 40..60.
    let mut sink = EvidenceSink::new(100, EvidenceStreams::none());
    let id = sink
        .region(Span::new(40, 60), "kmock", RegionMode::Script)
        .expect("a region of the file");
    sink.within(id, |sink| {
        // Region-relative spans shift by the region's start …
        sink.declaration("f", SymbolKind::Function, Span::new(2, 8), Reach::File);
        sink.import(
            ImportTarget::Relative("./x".into()),
            ImportShape::SideEffect,
            Span::new(0, 3),
            Confidence::Certain,
        );
        // … and clamp to the region, not the file.
        sink.reference(
            "g",
            kndo_contract::evidence::RefKind::Call,
            Span::new(10, 30),
        );
        // What the host never declared is dropped, silently.
        sink.comment(Span::new(0, 1), Span::new(0, 1));
        // A region inside a region is refused.
        assert!(
            sink.region(Span::new(0, 1), "kmock", RegionMode::Module)
                .is_none()
        );
    });
    // Back at the file's level, a write is the file's own again.
    sink.import(
        ImportTarget::Relative("./y".into()),
        ImportShape::SideEffect,
        Span::new(90, 93),
        Confidence::Certain,
    );
    let evidence = sink.finish();
    assert_eq!(evidence.declarations[0].span, Span::new(42, 48));
    assert_eq!(evidence.imports[0].span, Span::new(40, 43));
    assert_eq!(evidence.imports[0].embedded_in, Some(id));
    assert_eq!(evidence.references[0].span, Span::new(50, 60));
    assert!(evidence.comments.is_empty());
    assert_eq!(evidence.imports[1].span, Span::new(90, 93));
    assert_eq!(evidence.imports[1].embedded_in, None);
    assert_eq!(evidence.embedded.len(), 1);
    let messages: Vec<&str> = evidence
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages[0].contains("exceeds region length 20"));
    assert!(messages[1].contains("never nests"));
    assert!(!evidence.declared.contains(EvidenceStream::Comments));
}

#[test]
fn a_reach_stands_on_a_rung_and_never_reaches_wider_than_its_owner() {
    use kndo_contract::evidence::{EvidenceSink, EvidenceStreams, Reach, SymbolKind};
    use kndo_contract::extension::Rung;
    use kndo_contract::vocab::Span;
    let named = Reach::Named {
        namespace: vec!["crate".into(), "a".into()],
    };
    for (reach, rung) in [
        (Reach::Owner, Some(Rung::Owner)),
        (Reach::File, Some(Rung::File)),
        (Reach::Namespace { up: 0 }, Some(Rung::Namespace)),
        (Reach::Namespace { up: 2 }, Some(Rung::Namespace)),
        (named.clone(), Some(Rung::Namespace)),
        (Reach::Directory { up: 1 }, Some(Rung::Directory)),
        (
            Reach::Heirs {
                and_namespace: true,
            },
            Some(Rung::Heirs),
        ),
        (Reach::Unit { up: 0 }, Some(Rung::Unit)),
        (Reach::Unit { up: 1 }, Some(Rung::Group)),
        (Reach::Exported, Some(Rung::Exported)),
        (Reach::Inherited, None),
    ] {
        assert_eq!(reach.rung(), rung, "{reach:?}");
    }
    assert_eq!(Reach::Exported.capped_by(&Reach::File), Reach::File);
    assert_eq!(Reach::Owner.capped_by(&Reach::Exported), Reach::Owner);
    assert_eq!(
        Reach::Inherited.capped_by(&Reach::Unit { up: 0 }),
        Reach::Unit { up: 0 }
    );
    assert_eq!(
        Reach::Unit { up: 1 }.capped_by(&Reach::Unit { up: 0 }),
        Reach::Unit { up: 0 }
    );
    assert_eq!(
        Reach::Exported.capped_by(&Reach::Inherited),
        Reach::Inherited,
        "an unresolved owner caps as a namespace: the slip narrows, never publishes"
    );
    assert_eq!(Reach::Owner.capped_by(&Reach::Inherited), Reach::Owner);
    let heirs = Reach::Heirs {
        and_namespace: false,
    };
    assert_eq!(
        heirs.capped_by(&Reach::Namespace { up: 0 }),
        Reach::Namespace { up: 0 },
        "a protected member of a package-private type reaches the package"
    );
    assert_eq!(heirs.capped_by(&Reach::Exported), heirs);

    // A chain: a file-private type owning an exported type owning a member
    // that inherits — the member reaches the file, and so does the inner
    // type; a top-level `Inherited` has nothing to inherit and reads exported.
    let mut sink = EvidenceSink::new(100, EvidenceStreams::none());
    let outer = sink.declaration("Outer", SymbolKind::Type, Span::new(0, 90), Reach::File);
    let inner = sink.declaration(
        "Inner",
        SymbolKind::Type,
        Span::new(10, 80),
        Reach::Exported,
    );
    sink.member_of(inner, outer);
    let m = sink.declaration("m", SymbolKind::Method, Span::new(20, 30), Reach::Inherited);
    sink.member_of(m, inner);
    let stray = sink.declaration(
        "stray",
        SymbolKind::Function,
        Span::new(91, 99),
        Reach::Inherited,
    );
    let ev = sink.finish();
    assert_eq!(ev.effective_reach(outer), Reach::File);
    assert_eq!(ev.effective_reach(inner), Reach::File);
    assert_eq!(ev.effective_reach(m), Reach::File);
    assert_eq!(ev.effective_reach(stray), Reach::Exported);
}

#[test]
fn a_mount_reach_reads_from_where_it_stands_and_the_fewer_levels_cap() {
    use kndo_contract::evidence::Reach;

    // A mount's reach is written in the mounting file; read one namespace
    // deeper it addresses the same node one level further up.
    assert_eq!(
        Reach::Namespace { up: 0 }.shifted(2),
        Reach::Namespace { up: 2 }
    );
    // A unit, a name and an export are the same node wherever they are read.
    assert_eq!(Reach::Unit { up: 0 }.shifted(3), Reach::Unit { up: 0 });
    assert_eq!(Reach::Exported.shifted(1), Reach::Exported);

    // On one rung the address that climbs fewer levels is the narrower, in
    // either position.
    assert_eq!(
        Reach::Namespace { up: 5 }.capped_by(&Reach::Namespace { up: 2 }),
        Reach::Namespace { up: 2 }
    );
    assert_eq!(
        Reach::Namespace { up: 0 }.capped_by(&Reach::Namespace { up: 2 }),
        Reach::Namespace { up: 0 }
    );
    assert_eq!(
        Reach::Directory { up: 3 }.capped_by(&Reach::Directory { up: 1 }),
        Reach::Directory { up: 1 }
    );
    // Across rungs the narrower rung wins, which is how a fence takes an
    // export off a surface.
    assert_eq!(
        Reach::Exported.capped_by(&Reach::Namespace { up: 1 }),
        Reach::Namespace { up: 1 }
    );
}

#[test]
fn the_heirs_step_is_another_axis_of_the_ladder() {
    use kndo_contract::extension::{Ladder, Rung, Step};
    let ladder = Ladder::new(vec![
        Step::for_members(Rung::Owner, "private"),
        Step::for_members(Rung::Heirs, "protected"),
        Step::new(Rung::Unit, "internal"),
        Step::new(Rung::Exported, "public"),
    ]);
    // A member used in its file alone cannot fall to `protected`: nothing on
    // this ladder spells a file, so there is no advice.
    assert!(ladder.step_down(Rung::Unit, Rung::File, true).is_none());
    // Used in its owner alone, `private`; used from its subtypes alone,
    // `protected`.
    assert_eq!(
        ladder
            .step_down(Rung::Unit, Rung::Owner, true)
            .map(|s| s.word.as_str()),
        Some("private")
    );
    assert_eq!(
        ladder
            .step_down(Rung::Exported, Rung::Heirs, true)
            .map(|s| s.word.as_str()),
        Some("protected")
    );
}
