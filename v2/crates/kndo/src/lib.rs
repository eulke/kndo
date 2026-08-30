//! The facade. Frontends — the CLI, `kndo serve`, anything that renders results —
//! import `kndo::<Name>` and nothing deeper; when a frontend needs data or logic this
//! root does not export, that is a change to core, never a local re-derivation. This
//! crate also owns composition: [`default_adapters`] and [`default_plugins`] are the
//! one list each of what a stock kndo run speaks and runs.

pub use kndo_contract::adapter::{AdapterSpec, LanguageAdapter};
pub use kndo_contract::evidence::RootKind;
pub use kndo_contract::finding::{Finding, Severity, sort_findings};
pub use kndo_contract::subject::{FindingId, Subject, SymbolSelector};
pub use kndo_contract::vocab::{Category, Confidence, ProjectPath, Span, SubjectKind};
pub use kndo_core::{
    Abstention, AbstentionReason, AbstentionScope, Activation, ActivationRule, CONTENT_MAX_BYTES,
    CONTENT_MAX_FILES, Config, ContentView, GatePolicy, Graph, GraphView, PhaseTimings, Plugin,
    PluginContribution, PluginSeverity, PluginSink, PluginSpec, PluginTarget, Refusal, Report,
    RuleDescriptor, RunMode, RunOutcome, Session, Snapshot, SuppressedSummary, Threads, WellKnown,
    is_reserved_coordinate,
};
pub use kndo_coverage::{Coverage, FileCoverage};

use kndo_adapter_go::GoAdapter;
use kndo_adapter_rust::RustAdapter;
use kndo_adapter_ts::TypeScriptAdapter;
use kndo_plugin_coverage::LcovPlugin;

/// Every language a stock run speaks, in deterministic registration order.
pub fn default_adapters() -> Vec<Box<dyn LanguageAdapter>> {
    vec![
        Box::new(TypeScriptAdapter::new()),
        Box::new(RustAdapter::new()),
        Box::new(GoAdapter::new()),
    ]
}

/// Every plugin a stock run carries, in registration order — which is also
/// coverage-ingestion precedence. The `builtin_plugin_proofs` gate closes over this
/// list: a coordinate added here without a baseline-then-plugin proof fails the suite.
pub fn default_plugins() -> Vec<Box<dyn Plugin>> {
    vec![Box::new(LcovPlugin)]
}

/// A session over `root` with the default adapter and plugin sets — the one-call
/// entry frontends start from.
pub fn open(root: impl Into<std::path::PathBuf>, config: Config) -> Result<Session, Refusal> {
    Ok(Session::open(root, config, default_adapters())?.with_plugins(default_plugins()))
}
