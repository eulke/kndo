//! kndo-plugin-wrapper-demo — the reference *wrapper* plugin for kndo-plugin-api v1, existing
//! for exactly one contract point: its descriptor declares
//! `dependencies: ["kndo:express"]`.
//!
//! This is the shape a company framework has. The framework uses Express internally, so a
//! project that depends on the framework does **not** declare `express` in its own manifest —
//! and `kndo:express`'s activation rule (`ManifestDependency("express")`) can therefore never
//! fire for it. The framework ships a plugin for its own conventions, that plugin names
//! `kndo:express` in its `dependencies`, and being active is what activates the built-in
//! (RFC 0015 §3). There is no other path to a plugin whose framework is an indirect
//! dependency.
//!
//! It also proves the crossing that matters most: an EXTERNAL component naming a BUILT-IN.
//! The `kndo:` namespace is reserved — a component claiming such an id is refused at load —
//! which is exactly what makes naming one as a dependency unambiguous from any source.
//!
//! `crates/kndo/tests/plugin_dependency_implication.rs` installs this component, satisfies
//! only *this* plugin's activation rule, and asserts `kndo:express` joins composition as
//! `ImpliedBy` and does real work once there.
//!
//! The hook surface is deliberately empty: what is under test is what this plugin's
//! *dependency* does once implied, not anything this plugin contributes. Everything
//! interesting about it is in `descriptor()`.

// kndo:allow-file untested this guest crate is exercised end-to-end by the WASM-bridge
// integration tests, which build it as a subprocess and run the component in-process —
// reachability the host repo's graph cannot see (internal/detection-gaps.md §1).

// Only ever invoked through the macro path below, never an ordinary `use` — kept as a real
// import anyway (not a suppression-shaped workaround) so dependency hygiene sees a genuine
// usage edge rather than none at all.
use wit_bindgen as _;

wit_bindgen::generate!({
    path: "../../crates/kndo-plugin-api/wit/plugin.wit",
    world: "plugin",
});

use crate::kndo::plugin::types::*;

struct WrapperPlugin;

impl Guest for WrapperPlugin {
    fn descriptor() -> PluginDescriptor {
        PluginDescriptor {
            // An external coordinate, never the reserved `kndo:` namespace — a component
            // claiming that is refused at load.
            id: "github.com/acme/framework".to_string(),
            version: "1".to_string(),
            detection: vec![
                "an *.acme-framework-enable marker, standing in for this framework's own \
                 manifest signal"
                    .to_string(),
            ],
            requested_file_access: Vec::new(),
            // The wrapper's own opt-in marker — deliberately NOT anything express could match,
            // so the integration test can satisfy this rule alone and prove express's
            // activation arrived through `dependencies` and nowhere else.
            activation: vec![ActivationRule::FileExists(
                "*.acme-framework-enable".to_string(),
            )],
            // The whole reason this component exists.
            dependencies: vec!["kndo:express".to_string()],
        }
    }

    fn classify_file(_path: String, _current: FileClass) -> Option<FileClass> {
        None
    }

    fn contribute_roots() -> Vec<ContributedRoot> {
        Vec::new()
    }

    fn contribute_edges() -> Vec<ContributedEdge> {
        Vec::new()
    }

    fn annotate_symbols() -> Vec<PluginTarget> {
        Vec::new()
    }
}

export!(WrapperPlugin);
