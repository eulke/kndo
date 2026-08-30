//! The engine at its M1 size: discover → claim → extract (through the evidence
//! cache) → assemble → analyze → report, in the Session/Snapshot shape the design
//! fixes — `Session` owns configuration and effects; `Snapshot` is the immutable
//! result values and queries run on. Frontends consume this crate; adapters never do.

pub mod analysis;
mod cache;
mod discover;
mod extract;
mod graph;
mod report;
mod session;

pub use analysis::{Abstention, AbstentionReason, AbstentionScope, Analysis};
pub use graph::{Graph, GraphFile};
pub use report::Report;
pub use session::{Config, GatePolicy, Refusal, RunMode, RunOutcome, Session, Snapshot, Threads};
