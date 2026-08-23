//! Compliance suite: proves the `kndo-plugin-api` v1 ABI end to end against a real, external,
//! out-of-tree WASM component — `examples/kndo-plugin-demo` — driven through the real
//! `kndo_core::engine::Engine`, the same facade the shipped `kndo` binary uses (the exit
//! bar: a third-party demo adapter runs against the released binary).
//!
//! This test builds the demo guest itself (via `cargo build --target wasm32-unknown-unknown`)
//! and componentizes the resulting core module with the `wit-component` library — the same
//! library-only, no-external-CLI path a third-party author can use instead of installing
//! `cargo-component`. Nothing is checked in pre-built: the
//! component is always freshly built from the demo's current source, so this test can never
//! pass against a stale artifact.

use std::path::{Path, PathBuf};
use std::process::Command;

use kndo_core::adapter::LanguageAdapter;
use kndo_core::engine::{CheckRequest, ConfigOverrides, Engine, RunMode};
use kndo_plugin_api::WasmAdapter;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kndo-plugin-api has two ancestors up to the workspace root")
        .to_path_buf()
}

/// A process-unique `--target-dir` (not the demo crate's own shared `target/`) — several
/// independent test binaries build these same demo crates, and under `cargo test --workspace`'s
/// default parallelism a reader has been observed to pick up a wrong-shaped artifact from a
/// concurrent writer despite cargo's own target-dir lock.
fn isolated_target_dir() -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before the epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("kndo-wasm-target-{}-{nonce}", std::process::id()))
}

/// Builds `examples/kndo-plugin-demo` to a core WASM module and componentizes it in-process.
/// Returns the component bytes — no files touched outside a fresh temp dir the caller owns.
fn build_demo_component() -> Vec<u8> {
    let demo_dir = workspace_root().join("examples/kndo-plugin-demo");
    let target_dir = isolated_target_dir();
    let status = Command::new("cargo")
        .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
        .env("CARGO_TARGET_DIR", &target_dir)
        .current_dir(&demo_dir)
        .status()
        .expect("failed to invoke cargo to build the demo adapter");
    assert!(status.success(), "demo adapter guest build failed");

    let core_wasm_path = target_dir.join("wasm32-unknown-unknown/release/kndo_plugin_demo.wasm");
    let core_wasm = std::fs::read(&core_wasm_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", core_wasm_path.display()));
    let _ = std::fs::remove_dir_all(&target_dir);

    wit_component::ComponentEncoder::default()
        .module(&core_wasm)
        .expect("attaching the core module to the component encoder")
        .encode()
        .expect("encoding the demo adapter as a WASM component")
}

#[test]
fn external_wasm_adapter_runs_a_real_engine_check_end_to_end() {
    let component_bytes = build_demo_component();

    let component_dir = tempfile::tempdir().expect("temp dir for the component artifact");
    let component_path = component_dir.path().join("kdemo-adapter.wasm");
    std::fs::write(&component_path, &component_bytes).expect("writing the component artifact");

    let adapter = WasmAdapter::load(&component_path).expect("loading the demo WASM component");
    assert_eq!(adapter.descriptor().id.as_str(), "kdemo");

    let project_dir = tempfile::tempdir().expect("temp project fixture dir");
    std::fs::write(
        project_dir.path().join("program.kdemo"),
        "pub fn main() {\n    helper();\n}\n\nfn helper() {\n}\n\nfn dead() {\n}\n",
    )
    .expect("writing the kdemo fixture");

    let mut engine = Engine::open(
        project_dir.path(),
        ConfigOverrides {
            use_cache: false,
            threads: Some(1),
        },
        vec![Box::new(adapter)],
    )
    .expect("opening the engine over the fixture project");

    let result = engine.check(CheckRequest {
        mode: RunMode::Full,
    });

    assert_eq!(result.files_claimed, 1, "the .kdemo file must be claimed");
    assert_eq!(
        result.symbols, 3,
        "main, helper and dead must all be extracted as declarations"
    );

    let unused_symbols: Vec<&str> = result
        .findings
        .iter()
        .filter(|f| f.category == "unused")
        .filter_map(|f| f.location.symbol.as_deref())
        .collect();

    assert_eq!(
        unused_symbols,
        vec!["dead"],
        "only the never-called `dead` function should be unused — `main` is a root, `helper` \
         is reachable from it through a real References edge the WASM adapter's extract() \
         produced"
    );
}
