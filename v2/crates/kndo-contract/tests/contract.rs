use kndo_contract::evidence::{
    DiagnosticLevel, EvidenceSink, EvidenceStream, EvidenceStreams, FunctionMetrics, Reach,
    RefKind, RootKind, RootTarget, SymbolKind,
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
        selector: SymbolSelector::Member {
            owner: "Widget".into(),
            name: "draw".into(),
        },
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
