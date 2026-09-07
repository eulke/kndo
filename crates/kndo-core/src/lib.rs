//! The engine at its M1 size: discover → claim → extract (through the evidence
//! cache) → assemble → analyze → report, in the Session/Snapshot shape the design
//! fixes — `Session` owns configuration and effects; `Snapshot` is the immutable
//! result values and queries run on. Frontends consume this crate; adapters never do.

pub mod analysis;
mod cache;
pub mod conduct;
pub mod coverage;
mod discover;
mod dispatch;
mod extract;
mod graph;
mod health;
pub(crate) mod navigate;
pub mod project;
pub mod query;
mod render;
mod report;
mod scopes;
mod session;
mod suppress;

pub use analysis::{Abstention, AbstentionReason, AbstentionScope, Analysis};
pub use conduct::{
    Activation, ActivationReason, ActivationRule, CONTENT_MAX_BYTES, CONTENT_MAX_FILES,
    ConductSeverity, ConductSink, ConductTarget, ContentView, Contribution, DeclaredSymbol,
    Extension, GraphAccess, GraphView, RuleDescriptor, WellKnown, activate, is_reserved_coordinate,
    run_round,
};
pub use graph::{Graph, GraphFile, declared_roles};
pub use health::{CategoryCount, Health, Universe};
pub use kndo_contract::extension::{
    ConductBuilder, ExtensionSpec, ExtensionSpecBuilder, ExtensionSpecParts, MutatesGraph,
};
pub use report::{ExtensionRun, Mode, REPORT_SCHEMA, Report, ReportDiagnostic, RunInfo};
pub use session::{
    CacheLocation, Categories, Config, GatePolicy, PhaseTimings, PinnedSide, Refusal, RunMode,
    RunOutcome, Session, Snapshot, Threads,
};
pub use suppress::SuppressedSummary;

#[cfg(feature = "schema")]
pub use report::report_schema;
