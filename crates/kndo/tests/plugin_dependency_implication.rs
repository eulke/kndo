//! Proves the premise a company wrapper framework depends on: **a plugin named in another
//! plugin's `dependencies` activates even when its own rules never match**, across the crossing
//! that matters most — an EXTERNAL component naming a BUILT-IN.
//!
//! The shape is real. A framework uses Express internally, so a project depending on that
//! framework does not declare `express` in its own manifest, and `kndo:express`'s
//! `ManifestDependency("express")` rule can never fire for it. The framework's own plugin names
//! `kndo:express` in its `dependencies`, and being active is what activates the built-in.
//! Nothing else reaches a plugin whose framework is an indirect dependency.
//!
//! The adapter tier's own proof lives in `adapter_dependency_implication.rs`; this file is the
//! plugin-tier counterpart — the half real users depend on — so a refactor of the descriptor
//! that quietly breaks implication cannot pass unnoticed on either side.
//!
//! Own test binary (not another `#[test]` elsewhere): `KNDO_PLUGIN_DIR` is process-wide state,
//! and separate binaries are separate processes. Both halves — implied and not implied — live
//! in the single `#[test]` below for the same reason.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kndo has two ancestors up to the workspace root")
        .to_path_buf()
}

fn build_component(example_dir: &str, artifact: &str) -> Vec<u8> {
    let demo_dir = workspace_root().join(example_dir);
    // A `TempDir`: unique by construction and removed on drop, unwind included — a hand-rolled
    // fixed name would leak the whole build tree if the process panics before cleanup.
    let target_dir = tempfile::tempdir().expect("wasm target dir");
    let status = Command::new("cargo")
        .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
        // Cross-target guest build: host instrumentation flags must not leak into a target
        // that cannot link the profiling runtime.
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("LLVM_PROFILE_FILE")
        .env("CARGO_TARGET_DIR", target_dir.path())
        .current_dir(&demo_dir)
        .status()
        .unwrap_or_else(|e| panic!("failed to invoke cargo for {example_dir}: {e}"));
    assert!(status.success(), "{example_dir} guest build failed");

    let core_wasm_path = target_dir
        .path()
        .join(format!("wasm32-unknown-unknown/release/{artifact}"));
    let core_wasm = std::fs::read(&core_wasm_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", core_wasm_path.display()));

    wit_component::ComponentEncoder::default()
        .module(&core_wasm)
        .expect("attaching the core module to the component encoder")
        .encode()
        .expect("encoding the demo guest as a WASM component")
}

const WRAPPER_ID: &str = "github.com/acme/framework";

/// A project using the framework: a manifest that does **not** mention express, and `app.js`,
/// which ONLY express roots — by its entry-name convention, the one an Express server is
/// launched by rather than imported through. The marker decides whether the wrapper — and
/// therefore express — turns on.
///
/// `main` deliberately points at a different file (`index.js`): pointing it at `app.js` would
/// let the JS adapter's own manifest-main rooting keep the entry alive regardless of express,
/// which is exactly the false positive the negative half below rules out.
fn framework_project(with_marker: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp project fixture dir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"acme-app","main":"index.js","dependencies":{"@acme/framework":"1.0.0"}}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("index.js"), "export function lib() {}\n").unwrap();
    std::fs::write(dir.path().join("app.js"), "export function boot() {}\n").unwrap();
    if with_marker {
        std::fs::write(dir.path().join("acme.acme-framework-enable"), "").unwrap();
    }
    dir
}

fn unused_paths(root: &Path) -> Vec<String> {
    let overrides = kndo_core::engine::ConfigOverrides {
        use_cache: false,
        threads: Some(1),
        ..kndo_core::engine::ConfigOverrides::default()
    };
    let mut engine = kndo::open(root, overrides).expect("kndo::open");
    engine
        .check(kndo_core::engine::RunMode::Full)
        .findings
        .iter()
        .filter(|f| f.category == "unused")
        .filter_map(|f| f.location.path.as_ref().map(|p| p.0.to_string()))
        .collect()
}

fn express(root: &Path) -> kndo::ResolvedPlugin {
    kndo::plugin_resolution(root)
        .plugins
        .into_iter()
        .find(|p| p.id == "kndo:express")
        .expect("kndo:express is compiled into this build")
}

#[test]
fn an_external_plugins_dependencies_activate_a_builtin_whose_own_rule_never_fires() {
    let wrapper = build_component(
        "examples/kndo-plugin-wrapper-demo",
        "kndo_plugin_wrapper_demo.wasm",
    );
    let global_dir = tempfile::tempdir().expect("temp global plugin dir");
    std::fs::write(global_dir.path().join("acme-framework.wasm"), &wrapper).unwrap();
    // SAFETY: this binary holds exactly one #[test], so the var is set/removed sequentially.
    unsafe {
        std::env::set_var("KNDO_PLUGIN_DIR", global_dir.path());
    }

    // --- the premise -------------------------------------------------------------------
    let project = framework_project(true);
    let resolution = kndo::plugin_resolution(project.path());
    let wrapper_state = resolution
        .plugins
        .iter()
        .find(|p| p.id == WRAPPER_ID)
        .expect("the wrapper component is installed globally");
    assert_eq!(
        wrapper_state.active,
        Some(kndo::ActivationReason::RuleMatched(
            kndo::plugin::ActivationRule::FileExists("*.acme-framework-enable".into())
        )),
        "the wrapper's own *.acme-framework-enable rule must have fired"
    );
    assert_eq!(
        express(project.path()).active,
        Some(kndo::ActivationReason::ImpliedBy(WRAPPER_ID.into())),
        "no manifest declares express, so this activation can only have come through the \
         wrapper's `dependencies` — if it did not, a plugin whose framework is an indirect \
         dependency is unreachable"
    );
    assert!(
        resolution.missing_dependencies.is_empty(),
        "kndo:express is compiled in, so the wrapper's declaration is satisfied: {:?}",
        resolution.missing_dependencies
    );

    // Implication has to be real composition, not a doctor-only label: the implied plugin
    // roots the entry file exactly as if its own rule had matched.
    assert!(
        !unused_paths(project.path()).contains(&"app.js".to_string()),
        "the implied plugin's roots must fire for real"
    );

    // --- and the same project without the marker, which is what makes the above mean
    // something: nothing activates express on its own here.
    let bare = framework_project(false);
    assert_eq!(
        express(bare.path()).active,
        None,
        "with the wrapper inactive, nothing else can activate express in this project"
    );
    assert!(
        unused_paths(bare.path()).contains(&"app.js".to_string()),
        "and without express, the entry file is unreachable — the contrast the assertion above \
         rests on"
    );

    // SAFETY: same reasoning as the set_var above.
    unsafe {
        std::env::remove_var("KNDO_PLUGIN_DIR");
    }
}
