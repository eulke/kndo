//! The host half of the ABI: wasmtime, isolated. Loads `kndo:vocab@1` components
//! and adapts each world onto the same native trait its native siblings implement —
//! [`WasmAdapter`] is a [`kndo_contract::adapter::LanguageAdapter`], [`WasmPlugin`]
//! and [`WasmIngester`] are [`kndo_core::Plugin`]s — so the engine cannot tell
//! tiers apart and every containment rule (budgets, drops, the graph-cache bypass,
//! namespaced findings) applies identically by construction.
//!
//! This crate is the workspace's one wasmtime dependency: the build shell and
//! embedders that want no WASM leave it out without touching anything else.

mod adapter;
mod convert;
mod engine;
mod ingester;
mod plugin;

pub use adapter::WasmAdapter;
pub use ingester::WasmIngester;
pub use plugin::WasmPlugin;

/// A component that could not become a citizen. Everything AFTER a successful load
/// degrades instead (a trap contributes nothing); refusing to load is the one
/// moment the host says no out loud.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("could not read the component: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a loadable component for this world: {0}")]
    Component(String),
    #[error(
        "the component claims the reserved `kndo:` coordinate namespace \
         (`{coordinate}`) — built-ins are native; an external coordinate names its \
         own provenance"
    )]
    ReservedCoordinate { coordinate: String },
}

mod bindings {
    //! One `bindgen!` per world. Each generation elides the vocabulary down to
    //! what its world reaches and names its own Rust types for them, so the
    //! conversions in [`crate::convert`] are written ONCE as macro bodies and
    //! instantiated per world — one source text, however many spellings the
    //! generator insists on.

    pub mod adapter {
        wasmtime::component::bindgen!({
            path: "../../wit",
            world: "adapter",
        });
    }

    pub mod plugin {
        wasmtime::component::bindgen!({
            path: "../../wit",
            world: "plugin",
        });
    }

    pub mod ingester {
        wasmtime::component::bindgen!({
            path: "../../wit",
            world: "coverage-ingester",
        });
    }
}
