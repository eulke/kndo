//! kndo — the composed product, as a library (RFC 0001 §2 layering).
//!
//! This is the **distribution layer**: it owns the composition "kndo = core + these
//! languages (+ these built-in plugins, when they land)". That knowledge belongs to the
//! product, not to any presentation layer — a frontend (CLI, `kndo serve`/MCP, LSP, GUI, CI
//! action) depends on this crate alone and can never accidentally ship a kndo missing
//! languages, nor does adding a language ever touch a frontend.
//!
//! The ignorance rule survives intact: `kndo-core` still never depends on an adapter — this
//! crate sits *above* both and points downward at each.
//!
//! Embedders wanting a subset build with `--no-default-features --features js,go,…`
//! (ADR 0006 feature-gated builds).

pub use kndo_core::{
    adapter, analysis, cache, coverage, discovery, engine, graph, plugin, query, query_envelope,
    vocab,
};

use kndo_core::adapter::LanguageAdapter;
use kndo_core::engine::{ConfigOverrides, Engine, EngineError};
use kndo_core::plugin::Plugin;
use std::path::Path;

/// Every first-party adapter this build includes — the product's language registry, defined
/// exactly once. Adding a language: one dependency + feature in this crate's manifest, one
/// entry here. Nothing else in the workspace changes.
pub fn default_adapters() -> Vec<Box<dyn LanguageAdapter>> {
    // One cfg-gated push per adapter, deliberately: with eight feature-gated languages this
    // stays the clearest composition shape (clippy's vec![] suggestion doesn't).
    #[allow(clippy::vec_init_then_push)]
    {
        let mut adapters: Vec<Box<dyn LanguageAdapter>> = Vec::new();
        #[cfg(feature = "js")]
        adapters.push(Box::new(kndo_adapter_js::JsTsAdapter));
        #[cfg(feature = "go")]
        adapters.push(Box::new(kndo_adapter_go::GoAdapter));
        #[cfg(feature = "rust")]
        adapters.push(Box::new(kndo_adapter_rust::RustAdapter));
        #[cfg(feature = "java")]
        adapters.push(Box::new(kndo_adapter_java::JavaAdapter));
        #[cfg(feature = "kotlin")]
        adapters.push(Box::new(kndo_adapter_kotlin::KotlinAdapter));
        #[cfg(feature = "swift")]
        adapters.push(Box::new(kndo_adapter_swift::SwiftAdapter));
        #[cfg(feature = "json")]
        adapters.push(Box::new(kndo_adapter_json::JsonAdapter));
        #[cfg(feature = "css")]
        adapters.push(Box::new(kndo_adapter_css::CssAdapter));
        adapters
    }
}

/// Every first-party plugin this build includes (RFC 0003) — the product's ecosystem registry,
/// same shape and same reasoning as [`default_adapters`]: one entry here, nothing else in the
/// workspace changes. Just the built-in lcov coverage ingester today; framework-convention
/// plugins (react, nextjs, spring…) are RFC 0003 §3's stated launch set, not yet built —
/// tracked in the ROADMAP, not silently implied by this function's name.
pub fn default_plugins() -> Vec<Box<dyn Plugin>> {
    vec![Box::new(kndo_core::plugin::LcovPlugin)]
}

/// The one-line entry point frontends use: an [`Engine`] over the full default product plus
/// whatever third-party WASM adapters this project has installed. Frontends needing a custom
/// adapter or plugin set (embedders, tests) still have [`Engine::open`]/[`Engine::open_with_plugins`]
/// directly.
pub fn open(root: &Path, overrides: ConfigOverrides) -> Result<Engine, EngineError> {
    let mut adapters = default_adapters();
    adapters.extend(external_adapters(root));
    let mut plugins = default_plugins();
    plugins.extend(external_plugins(root));
    Engine::open_with_plugins(root, overrides, adapters, plugins)
}

/// Third-party adapters as WASM components (ADR 0003, `docs/contracts/wasm-abi.md`),
/// auto-discovered from `.kndo/plugins/*.wasm` (RFC 0003 §3's stated convention) — no
/// `kndo.toml` entry needed, the same "drop a file in, it's live" default every other
/// zero-config surface in this product follows. A component that fails to load (not a real
/// component binary, a version mismatch, an instantiation error) is skipped rather than
/// failing the whole run: one broken extension must not take every other language down with
/// it. There is no diagnostic surfaced for a *load*-time failure yet (unlike a per-call
/// fuel/budget trip inside `WasmAdapter::extract`, which does produce one) — an honest gap,
/// not an omission papered over; `kndo doctor`'s adapter list is the way to confirm a plugin
/// actually loaded until one lands.
fn external_adapters(root: &Path) -> Vec<Box<dyn LanguageAdapter>> {
    #[cfg(feature = "external-adapters")]
    {
        wasm_components(root)
            .into_iter()
            .filter_map(|path| kndo_plugin_api::WasmAdapter::load(&path).ok())
            .map(|adapter| Box::new(adapter) as Box<dyn LanguageAdapter>)
            .collect()
    }
    #[cfg(not(feature = "external-adapters"))]
    {
        let _ = root;
        Vec::new()
    }
}

/// Third-party `Plugin`s as WASM components (`docs/contracts/wasm-abi.md` §5), same
/// `.kndo/plugins/*.wasm` directory and zero-config discovery as [`external_adapters`]. A
/// `.wasm` file only ever implements one of the two ABIs (`kndo:adapter` or `kndo:plugin`) — its
/// world's exports say which, so nothing here has to *ask*: [`WasmAdapter::load`] and
/// [`WasmPlugin::load`] both simply fail to instantiate against a component compiled for the
/// other world's exports, and each loader silently skips what it can't load. A single directory
/// scan feeding both loaders is deliberate, not an accident of how [`external_adapters`] already
/// existed — a plugin author never has to name their file `*.adapter.wasm` vs `*.plugin.wasm` or
/// sort it into a subdirectory to say which ABI it targets.
fn external_plugins(root: &Path) -> Vec<Box<dyn Plugin>> {
    #[cfg(feature = "external-adapters")]
    {
        wasm_components(root)
            .into_iter()
            .filter_map(|path| kndo_plugin_api::WasmPlugin::load(&path).ok())
            .map(|plugin| Box::new(plugin) as Box<dyn Plugin>)
            .collect()
    }
    #[cfg(not(feature = "external-adapters"))]
    {
        let _ = root;
        Vec::new()
    }
}

#[cfg(feature = "external-adapters")]
fn wasm_components(root: &Path) -> Vec<std::path::PathBuf> {
    let dir = root.join(".kndo").join("plugins");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("wasm"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_core::engine::{CheckRequest, RunMode};

    #[test]
    fn default_build_registers_at_least_one_language() {
        assert!(!default_adapters().is_empty());
    }

    #[test]
    fn open_composes_the_full_product() {
        let dir = std::env::temp_dir().join("kndo-dist-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.ts"), "export function f() { return 1; }").unwrap();

        let mut engine = open(&dir, ConfigOverrides::default()).unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Full,
        });
        // The js adapter came from the distribution layer, not from this test.
        assert_eq!(result.files_claimed, 1);
        assert_eq!(result.symbols, 1);
    }
}
