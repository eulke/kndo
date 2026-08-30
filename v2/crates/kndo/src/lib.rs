//! The facade. Frontends — the CLI, `kndo serve`, anything that renders results —
//! import `kndo::<Name>` and nothing deeper; when a frontend needs data or logic this
//! root does not export, that is a change to core, never a local re-derivation. This
//! crate also owns composition: [`default_adapters`] is the one list of what a stock
//! kndo run speaks.

pub use kndo_contract::adapter::{AdapterSpec, LanguageAdapter};
pub use kndo_contract::finding::{Finding, Severity, sort_findings};
pub use kndo_contract::subject::{FindingId, Subject, SymbolSelector};
pub use kndo_contract::vocab::{Category, Confidence, ProjectPath, Span, SubjectKind};
pub use kndo_core::{
    Abstention, AbstentionReason, AbstentionScope, Config, GatePolicy, Graph, PhaseTimings,
    Refusal, Report, RunMode, RunOutcome, Session, Snapshot, SuppressedSummary, Threads,
};

use kndo_adapter_rust::RustAdapter;
use kndo_adapter_ts::TypeScriptAdapter;

/// Every language a stock run speaks, in deterministic registration order.
pub fn default_adapters() -> Vec<Box<dyn LanguageAdapter>> {
    vec![
        Box::new(TypeScriptAdapter::new()),
        Box::new(RustAdapter::new()),
    ]
}

/// A session over `root` with the default adapter set — the one-call entry frontends
/// start from.
pub fn open(root: impl Into<std::path::PathBuf>, config: Config) -> Result<Session, Refusal> {
    Session::open(root, config, default_adapters())
}
