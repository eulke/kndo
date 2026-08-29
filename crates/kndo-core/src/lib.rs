//! kndo-core — the system behind every frontend.
//!
//! This crate is **language-blind** (the ignorance rule): it must never contain
//! the name of a language. It defines the neutral vocabulary ([`vocab`]), the contracts
//! adapters and plugins implement ([`adapter`], [`plugin`]), and the only surface frontends
//! may touch ([`engine::Engine`]). The core never prints; frontends never
//! compute.
//!
//! Every contract a component or frontend relies on is legible from this code and its doc
//! comments alone: the traits in [`adapter`] and [`plugin`], the vocabulary in [`vocab`], and
//! the surface [`engine::Engine`] exposes are the complete, load-bearing definition of what
//! this crate promises.

pub mod adapter;
/// Plumbing behind `RunResult::to_agent_format`/`QueryResult::to_agent_format` — reached
/// through those methods, never named directly by a frontend, so it carries no
/// public-facing API of its own worth advertising in rustdoc.
#[doc(hidden)]
pub mod agent_format;
pub mod analysis;
mod baseline;
pub mod cache;
pub mod config;
pub mod conformance;
pub mod coverage;
/// Delta budgets (`[delta]`) — the gate's aggregate half, alongside `--fail-on`'s severity
/// half. Public because `Budget` lands in the output schema and frontends render it.
pub mod delta;
pub mod discovery;
pub mod engine;
mod gitutil;
pub mod graph;
pub mod paths;
pub mod plugin;
mod plugin_gate;
pub mod query;
pub mod query_envelope;
mod rkyv_support;
pub mod sarif;
mod suppression;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
pub mod vocab;

// ---------------------------------------------------------------- frontend facade
//
// The surface a frontend (CLI, `kndo serve`/MCP, LSP, GUI) actually needs to drive an
// `Engine` end to end, re-exported at the crate root so "import from a module path" is never
// the answer to "I need a new piece of data" — only `Engine`, and what it returns, is
// contract. A frontend needing something not listed here
// is a core PR that adds it to `RunResult`/exports the helper, not a deeper import of
// `engine`/`vocab`/`query_envelope` internals. Adapter/plugin authoring types
// (`adapter::LanguageAdapter`, `plugin::Plugin`, `graph::GraphView`, …) are a different
// surface — component authors, not frontends — and stay reached through their own modules.
pub use crate::delta::{Budget, BudgetRule, BudgetVerdict};
pub use config::{SkipSpec, SkipSpecError};
pub use engine::{
    sort_findings_for_display, BaselineOp, BaselineResult, ConfigOverrides, Delta, DeltaOrigin,
    DoctorReport, Engine, EngineError, Finding, Location, RunMode, RunResult, Severity,
    SuppressedSummary, KNDO_VERSION, SCHEMA_VERSION,
};
pub use query_envelope::{QueryFlags, QueryRequest, QueryResult, ResultEntry, Verb};
pub use vocab::{
    Category, Confidence, Diagnostic, DiagnosticLevel, Group, ProjectPath, SubjectKind,
};
