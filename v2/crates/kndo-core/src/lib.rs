//! The engine at its M1 size: discover → claim → extract (through the evidence
//! cache) → assemble → analyze → report, in the Session/Snapshot shape the design
//! fixes — `Session` owns configuration and effects; `Snapshot` is the immutable
//! result values and queries run on. Frontends consume this crate; adapters never do.

pub mod analysis;
mod cache;
mod discover;
mod extract;
mod graph;
pub mod plugin;
mod report;
mod session;
mod suppress;

pub use analysis::{Abstention, AbstentionReason, AbstentionScope, Analysis};
pub use graph::{Graph, GraphFile};
pub use plugin::{
    Activation, ActivationReason, ActivationRule, CONTENT_MAX_BYTES, CONTENT_MAX_FILES,
    ContentView, GraphView, Plugin, PluginContribution, PluginSeverity, PluginSink, PluginSpec,
    PluginTarget, RuleDescriptor, WellKnown, is_reserved_coordinate,
};
pub use report::{Report, ReportDiagnostic};
pub use session::{
    Config, GatePolicy, PhaseTimings, Refusal, RunMode, RunOutcome, Session, Snapshot, Threads,
};
pub use suppress::SuppressedSummary;

#[cfg(feature = "schema")]
pub use report::report_schema;
