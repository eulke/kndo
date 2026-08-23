//! kndo-adapter-toolkit — the paved road for first-party adapters.
//!
//! Tree-sitter query helpers, the consistent-`$n` token normalizer, and the shared
//! cyclomatic-complexity walker. An adapter is largely grammar + queries + resolver logic
//! on top of this crate.

pub mod classify;
pub mod jvm_manifest;
pub mod metrics;
pub mod parsing;
pub mod paths;
pub mod stdlib;
pub mod suppression;
