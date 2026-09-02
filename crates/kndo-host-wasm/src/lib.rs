//! The host half of the ABI: wasmtime, isolated. Loads `kndo:vocab@1` components
//! through the ONE world and adapts them onto the same [`Extension`] trait the
//! built-ins implement — so the engine cannot tell tiers apart and every
//! containment rule (budgets, drops, the graph-cache bypass, namespaced
//! findings, phase discipline) applies identically by construction.
//!
//! This crate is the workspace's one wasmtime dependency: the build shell and
//! embedders that want no WASM leave it out without touching anything else.

mod convert;
mod engine;
mod extension;

pub use extension::WasmExtension;

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

#[doc(hidden)]
pub mod bindings {
    //! One `bindgen!`, one world, one set of generated types — the conversions
    //! in [`crate::convert`] are plain functions again. `trappable_imports`
    //! lets every host import refuse with a real error: the spelling of the
    //! phase discipline in [`crate::extension`].

    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "extension",
        trappable_imports: true,
    });
}
