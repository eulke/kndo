//! kndo-plugin-api — the host-side bridge for the WASM component-model ABI external adapters
//! target (ADR 0003, RFC 0002/0003 §3). Normative spec: `docs/contracts/wasm-abi.md`.
//!
//! This crate is the "generated bridge" ADR 0003 promises: [`WasmAdapter`] implements the same
//! native [`kndo_core::adapter::LanguageAdapter`] trait every first-party adapter implements, so
//! from the `Engine`'s perspective a loaded WASM component is indistinguishable from a compiled-
//! in adapter — the distribution layer (`kndo` crate) can push a `WasmAdapter` onto the same
//! `Vec<Box<dyn LanguageAdapter>>` `default_adapters()` returns.

mod host;

pub use host::{LoadError, WasmAdapter};
