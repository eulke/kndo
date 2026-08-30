use kndo_contract::evidence::{
    DiagnosticLevel, EvidenceSink, FunctionMetrics, Reach, RefKind, RootKind, RootTarget,
    SymbolKind,
};
use kndo_contract::subject::{FindingId, Subject, SymbolSelector};
use kndo_contract::vocab::{Category, ProjectPath, Span, SubjectKind};

#[test]
fn sink_attaches_metrics_and_membership_by_id() {
    let mut sink = EvidenceSink::new(100);
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
    let mut sink = EvidenceSink::new(10);
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
    let c = Category::plugin("github.com/acme/kndo-x", "no-foo");
    assert!(c.is_plugin());
    assert_eq!(c.as_str(), "plugin:github.com/acme/kndo-x/no-foo");
    assert!(!Category::UNUSED.is_plugin());
}

#[test]
fn contract_fingerprint_is_stable_within_a_build() {
    assert_eq!(
        kndo_contract::contract_fingerprint_hex(),
        kndo_contract::contract_fingerprint_hex()
    );
}
