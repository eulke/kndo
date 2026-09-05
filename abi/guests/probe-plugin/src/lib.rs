//! The reference external plugin — written against the REAL [`Extension`]
//! trait, the same one a built-in implements: the spec through the two-stage
//! builder, roots and findings through the contract's own `ConductSink`, content
//! through the same scoped view. It exercises rule-based activation, a
//! contributed root (its spec declares `MutatesGraph::Yes`), findings under a
//! declared rule — one built from a scoped content probe — plus two deliberate
//! misbehaviors (an undeclared rule, a missing target) whose DROPS the host must
//! report on the contribution: the containment model, observed from outside.

use kndo_contract::evidence::RootKind;
use kndo_contract::extension::{
    Activation, ActivationRule, ConductSeverity, ConductSink, ConductTarget, ContentAccess,
    Extension, ExtensionSpec, GraphAccess, MutatesGraph,
};
use kndo_contract::vocab::{Confidence, ProjectPath};
use std::sync::LazyLock;

static SPEC: LazyLock<ExtensionSpec> = LazyLock::new(|| {
    ExtensionSpec::builder("demo:probe", 1)
        .conduct(
            Activation::AnyRule(vec![ActivationRule::FileExists("*.kmini".into())]),
            MutatesGraph::Yes,
        )
        .requested_file_access(&["config.probe"])
        .rule("note", "reports what the probe observed")
        .build()
});

#[derive(Default)]
struct ProbePlugin;

impl Extension for ProbePlugin {
    fn spec(&self) -> &ExtensionSpec {
        &SPEC
    }

    fn contribute_roots(
        &self,
        graph: &dyn GraphAccess,
        _content: &dyn ContentAccess,
        out: &mut ConductSink,
    ) {
        // Anchor liveness the language cannot see, when the target exists.
        if graph.contains(&ProjectPath::new("wired.kmini")) {
            out.root(
                ConductTarget::File(ProjectPath::new("wired.kmini")),
                RootKind::Production,
                Confidence::Certain,
            );
        }
        // A root at a file no graph holds — the host must drop it, described.
        out.root(
            ConductTarget::File(ProjectPath::new("nowhere.kmini")),
            RootKind::Production,
            Confidence::Certain,
        );
    }

    fn report_findings(
        &self,
        graph: &dyn GraphAccess,
        content: &dyn ContentAccess,
        out: &mut ConductSink,
    ) {
        let first = graph
            .paths()
            .next()
            .cloned()
            .unwrap_or_else(|| ProjectPath::new(""));
        let seen = match content.read(&ProjectPath::new("config.probe")) {
            Some(bytes) => format!("config.probe is {} bytes", bytes.len()),
            None => "config.probe unreadable".to_string(),
        };
        out.finding(
            "note",
            ConductSeverity::Info,
            ConductTarget::File(first),
            Confidence::Certain,
            seen,
        );
        // Under a rule the spec never declared — the host must drop it.
        out.finding(
            "ghost",
            ConductSeverity::Info,
            ConductTarget::File(ProjectPath::new("wired.kmini")),
            Confidence::Possible,
            "never lands",
        );
    }
}

kndo_sdk::export_extension!(ProbePlugin);
