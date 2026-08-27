//! A third-party demo adapter, built out-of-tree, runs against `kndo::open` — the exact
//! entry point the shipped `kndo-cli` binary calls at every command. Unlike
//! `kndo-plugin-api`'s own compliance test (which drives a `WasmAdapter` directly through a
//! hand-built `Engine`), this test goes through the full product composition —
//! `.kndo/plugins/*.wasm` auto-discovery included — so it also pins that the discovery
//! wiring itself works, not just the ABI bridge underneath it.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kndo has two ancestors up to the workspace root")
        .to_path_buf()
}

fn build_demo_component() -> Vec<u8> {
    let demo_dir = workspace_root().join("examples/kndo-plugin-demo");
    // A `TempDir`: unique by construction and removed on drop, unwind included —
    // the hand-rolled pid+nonce name it replaced leaked the whole build tree on panic.
    let target_dir = tempfile::tempdir().expect("wasm target dir");
    let status = Command::new("cargo")
        .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
        // Cross-target guest build: instrumentation flags from the host environment
        // (cargo-llvm-cov's `-C instrument-coverage` in RUSTFLAGS) must not leak into a
        // target that cannot link the profiling runtime.
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("LLVM_PROFILE_FILE")
        .env("CARGO_TARGET_DIR", target_dir.path())
        .current_dir(&demo_dir)
        .status()
        .expect("failed to invoke cargo to build the demo adapter");
    assert!(status.success(), "demo adapter guest build failed");

    let core_wasm_path = target_dir
        .path()
        .join("wasm32-unknown-unknown/release/kndo_plugin_demo.wasm");
    let core_wasm = std::fs::read(&core_wasm_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", core_wasm_path.display()));

    wit_component::ComponentEncoder::default()
        .module(&core_wasm)
        .expect("attaching the core module to the component encoder")
        .encode()
        .expect("encoding the demo adapter as a WASM component")
}

#[test]
fn kndo_open_auto_discovers_a_kndo_plugins_wasm_adapter() {
    let component_bytes = build_demo_component();

    let project_dir = tempfile::tempdir().expect("temp project fixture dir");
    let plugins_dir = project_dir.path().join(".kndo").join("plugins");
    std::fs::create_dir_all(&plugins_dir).expect("creating .kndo/plugins");
    std::fs::write(plugins_dir.join("kdemo.wasm"), &component_bytes)
        .expect("writing the component artifact into .kndo/plugins");

    std::fs::write(
        project_dir.path().join("program.kdemo"),
        "pub fn main() {\n    helper();\n}\n\nfn helper() {\n}\n\nfn dead() {\n}\n",
    )
    .expect("writing the kdemo fixture");

    let mut engine = kndo::open(
        project_dir.path(),
        kndo_core::engine::ConfigOverrides {
            use_cache: false,
            threads: Some(1),
            min_confidence: None,
        },
    )
    .expect("kndo::open must succeed with a plugin present");

    let result = engine.check(kndo_core::engine::RunMode::Full);

    assert_eq!(
        result.files_claimed, 1,
        "the .kdemo file must be claimed by the auto-discovered plugin, not fall through unclaimed"
    );

    let unused_symbols: Vec<&str> = result
        .findings
        .iter()
        .filter(|f| f.category == "unused")
        .filter_map(|f| f.location.symbol.as_deref())
        .collect();
    assert_eq!(unused_symbols, vec!["dead"]);
}
