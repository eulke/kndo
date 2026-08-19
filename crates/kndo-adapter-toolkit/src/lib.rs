//! kndo-adapter-toolkit — the paved road for first-party adapters (ADR 0002).
//!
//! Grows in M1 alongside the JS/TS adapter: tree-sitter query helpers, the consistent-`$n`
//! token normalizer (RFC 0005 §6), and the shared cyclomatic-complexity walker. An adapter is
//! largely grammar + queries + resolver logic on top of this crate.

pub mod parsing;
pub mod stdlib;
