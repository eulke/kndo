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

pub use kndo_core::{adapter, discovery, engine, graph, plugin, query, query_envelope, vocab};

use kndo_core::adapter::LanguageAdapter;
use kndo_core::engine::{ConfigOverrides, Engine, EngineError};
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
        adapters
    }
}

/// The one-line entry point frontends use: an [`Engine`] over the full default product.
/// Frontends needing a custom adapter set (embedders, tests) still have
/// [`Engine::open`] directly.
pub fn open(root: &Path, overrides: ConfigOverrides) -> Result<Engine, EngineError> {
    Engine::open(root, overrides, default_adapters())
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
