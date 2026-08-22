//! kndo-adapter-wrapper-demo — the reference *wrapper* adapter for kndo-plugin-api v1
//! (docs/contracts/wasm-abi.md), existing for exactly one contract point: its descriptor
//! declares `dependencies: ["kdemo"]` (RFC 0017 §6). RFC 0015 §1's wrapper-framework story —
//! "a company framework's component implies the components it builds on" — applies to
//! adapters the same way it does to plugins, and this component is the real-WASM half of
//! that proof: `crates/kndo/tests/adapter_dependency_implication.rs` installs it globally
//! next to the kdemo demo adapter, satisfies only *this* adapter's activation rule, and
//! asserts kdemo joins composition as `ImpliedBy("kwrap")`.
//!
//! The language surface is deliberately near-nothing: `.kwrap` files are claimed as
//! production sources and yield no facts (a "wrapper config" the wrapped language does all
//! the work for). Everything interesting about this adapter is in `descriptor()`.

// Only ever invoked through the macro path below, never an ordinary `use` — kept as a real
// import anyway (not a suppression-shaped workaround) so dependency hygiene sees a genuine
// usage edge rather than none at all.
use wit_bindgen as _;

wit_bindgen::generate!({
    path: "../../crates/kndo-plugin-api/wit/adapter.wit",
    world: "adapter",
});

use crate::kndo::adapter::types::*;

// kndo:allow unused reason: constructed only via the export!() macro below, invisible to kndo's un-expanded parse
struct WrapperAdapter;

impl Guest for WrapperAdapter {
    fn descriptor() -> AdapterDescriptor {
        AdapterDescriptor {
            id: "kwrap".to_string(),
            facts_schema_version: 1,
            file_globs: vec!["**/*.kwrap".to_string()],
            grammar_version: "hand-scanned-v1".to_string(),
            // The wrapper's own opt-in marker — deliberately NOT kdemo's `*.kdemo-enable`,
            // so the integration test can satisfy this rule alone and prove kdemo's
            // activation arrived through `dependencies`, not through its own rules.
            activation: vec![ActivationRule::FileExists("*.kwrap-enable".to_string())],
            dependencies: vec!["kdemo".to_string()],
        }
    }

    fn claim(path: String) -> Option<FileClaim> {
        if path.ends_with(".kwrap") {
            Some(FileClaim {
                class: FileClass {
                    role: FileRole::Production,
                    origin: FileOrigin::Authored,
                },
            })
        } else {
            None
        }
    }

    fn extract(_path: String, _content: String) -> FileFacts {
        FileFacts {
            declarations: Vec::new(),
            references: Vec::new(),
            roots: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

export!(WrapperAdapter);
