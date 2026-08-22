//! kndo-plugin-api — the host-side bridge for the WASM component-model ABI external adapters
//! target (ADR 0003, RFC 0002/0003 §3). Normative spec: `docs/contracts/wasm-abi.md`.
//!
//! This crate is the "generated bridge" ADR 0003 promises: [`WasmAdapter`] implements the same
//! native [`kndo_core::adapter::LanguageAdapter`] trait every first-party adapter implements, so
//! from the `Engine`'s perspective a loaded WASM component is indistinguishable from a compiled-
//! in adapter — the distribution layer (`kndo` crate) can push a `WasmAdapter` onto the same
//! `Vec<Box<dyn LanguageAdapter>>` `default_adapters()` returns.

mod engine;
mod host;
mod plugin_host;

pub use host::{LoadError, WasmAdapter};
pub use plugin_host::LoadError as WasmPluginLoadError;
pub use plugin_host::WasmPlugin;

/// The `kndo:plugin` WIT world this host was built against, verbatim (RFC 0017 §7's author
/// kit): what `kndo plugin new` writes into a scaffold and `kndo plugin wit` prints, so an
/// author vendors the ABI from the binary they actually run — never from a possibly-mismatched
/// git checkout.
pub const PLUGIN_WIT: &str = include_str!("../wit/plugin.wit");

/// The `kndo:adapter` WIT world this host was built against, verbatim — same contract as
/// [`PLUGIN_WIT`].
pub const ADAPTER_WIT: &str = include_str!("../wit/adapter.wit");
