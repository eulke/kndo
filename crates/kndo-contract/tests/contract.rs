use kndo_contract::evidence::{
    DiagnosticLevel, EvidenceSink, EvidenceStream, EvidenceStreams, FunctionMetrics, MarkerTarget,
    Reach, RefKind, RootKind, RootTarget, SymbolKind,
};
use kndo_contract::subject::{FindingId, Subject, SymbolSelector};
use kndo_contract::vocab::{Category, ProjectPath, Span, SubjectKind};

#[test]
fn sink_attaches_metrics_and_membership_by_id() {
    let mut sink = EvidenceSink::new(100, EvidenceStreams::of(&[EvidenceStream::Metrics]));
    let owner = sink.declaration(
        "Widget",
        SymbolKind::Type,
        Span::new(0, 40),
        Reach::Exported,
    );
    let method = sink.declaration(
        "draw",
        SymbolKind::Method,
        Span::new(10, 30),
        Reach::Private,
    );
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
    let d = sink.declaration("x", SymbolKind::Function, Span::new(4, 99), Reach::Private);
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
    let f = sink.declaration("f", SymbolKind::Function, Span::new(10, 30), Reach::Private);
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
        Reach::Private,
    );
    let first = sink.declaration(
        "helper",
        SymbolKind::Function,
        Span::new(200, 250),
        Reach::Private,
    );
    let again = sink.declaration(
        "helper",
        SymbolKind::Function,
        Span::new(300, 350),
        Reach::Private,
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
