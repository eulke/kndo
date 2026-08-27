//! kndo-adapter-toolkit — the paved road for first-party adapters.
//!
//! Tree-sitter query helpers, the consistent-`$n` token normalizer, and the shared
//! cyclomatic-complexity walker. An adapter is largely grammar + queries + resolver logic
//! on top of this crate.

pub mod classify;
/// Maven `pom.xml` parsing — real XML via `roxmltree`, gated behind the `jvm-xml` feature
/// so a non-JVM adapter never compiles it. Only `kndo-adapter-java`/`kndo-adapter-kotlin`
/// enable the feature.
#[cfg(feature = "jvm-xml")]
pub mod jvm_manifest;
pub mod metrics;
pub mod parsing;
/// Re-export: the arithmetic moved to `kndo-core` when a plugin needed it too (the
/// toolkit's audience is adapters). Adapters keep calling `kndo_adapter_toolkit::paths::*`.
pub use kndo_core::paths;
pub mod stdlib;
pub mod suppression;
