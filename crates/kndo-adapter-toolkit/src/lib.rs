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
pub mod paths;
pub mod stdlib;
pub mod suppression;
