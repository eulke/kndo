//! Proves the `Plugin` WASM bridge through the *full* product composition —
//! `kndo::open`'s real `.kndo/plugins/*.wasm` auto-discovery, not a hand-built
//! `Engine` — mirroring `external_adapter.rs`'s own discipline for the adapter ABI. Both an
//! external adapter (`examples/kndo-plugin-demo`) and an external plugin
//! (`examples/kndo-plugin-hooks-demo`) are dropped into the *same* `.kndo/plugins/` directory,
//! proving the single-directory-two-loaders discovery design (`kndo::external_plugins`'s own
//! doc comment) actually sorts them correctly rather than by convention or configuration.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kndo has two ancestors up to the workspace root")
        .to_path_buf()
}

fn build_component(example_dir: &str, wasm_name: &str) -> Vec<u8> {
    let demo_dir = workspace_root().join(example_dir);
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
        .unwrap_or_else(|e| panic!("failed to invoke cargo for {example_dir}: {e}"));
    assert!(status.success(), "{example_dir} guest build failed");

    let core_wasm_path = target_dir
        .path()
        .join("wasm32-unknown-unknown/release")
        .join(wasm_name);
    let core_wasm = std::fs::read(&core_wasm_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", core_wasm_path.display()));

    wit_component::ComponentEncoder::default()
        .module(&core_wasm)
        .expect("attaching the core module to the component encoder")
        .encode()
        .expect("encoding the component")
}

#[test]
fn kndo_open_sorts_a_mixed_adapter_and_plugin_plugins_directory() {
    let adapter_bytes = build_component("examples/kndo-plugin-demo", "kndo_plugin_demo.wasm");
    let plugin_bytes = build_component(
        "examples/kndo-plugin-hooks-demo",
        "kndo_plugin_hooks_demo.wasm",
    );

    let project_dir = tempfile::tempdir().expect("temp project fixture dir");
    let plugins_dir = project_dir.path().join(".kndo").join("plugins");
    std::fs::create_dir_all(&plugins_dir).expect("creating .kndo/plugins");
    // Same directory, both components — nothing names or sorts them by ABI kind.
    std::fs::write(plugins_dir.join("kdemo.wasm"), &adapter_bytes).unwrap();
    std::fs::write(plugins_dir.join("hooks-demo.wasm"), &plugin_bytes).unwrap();

    let fixture = r#"
pub fn main() {
    consumed_x();
}

pub fn consumed_x() {
}

fn root_target() {
}

fn wire_target() {
}

fn trulyDead() {
}
"#;
    std::fs::write(project_dir.path().join("program.kdemo"), fixture)
        .expect("writing the kdemo fixture");
    std::fs::write(
        project_dir.path().join("noise.banner.kdemo"),
        "fn bannerDecl() {\n}\n",
    )
    .expect("writing the banner fixture");

    let overrides = kndo_core::engine::ConfigOverrides {
        use_cache: false,
        threads: Some(1),
        min_confidence: None,
    };

    // Baseline: same fixture, no plugin present (delete it from the directory first) — every
    // scenario the plugin later rescues must actually fire on its own.
    std::fs::remove_file(plugins_dir.join("hooks-demo.wasm")).unwrap();
    let mut baseline =
        kndo::open(project_dir.path(), overrides.clone()).expect("kndo::open (baseline)");
    let baseline_result = baseline.check(kndo_core::engine::RunMode::Full);
    let baseline_unused: Vec<&str> = baseline_result
        .findings
        .iter()
        .filter(|f| f.category == "unused")
        .filter_map(|f| f.location.symbol.as_deref())
        .collect();
    assert!(
        baseline_unused.contains(&"root_target"),
        "{baseline_unused:?}"
    );
    assert!(
        baseline_unused.contains(&"wire_target"),
        "{baseline_unused:?}"
    );
    let baseline_unused_files: Vec<String> = baseline_result
        .findings
        .iter()
        .filter(|f| f.category == "unused" && f.subject_kind == "file")
        .filter_map(|f| f.location.path.as_ref())
        .map(|p| p.0.to_string())
        .collect();
    assert!(baseline_unused_files.contains(&"noise.banner.kdemo".to_string()));

    std::fs::write(plugins_dir.join("hooks-demo.wasm"), &plugin_bytes).unwrap();

    let mut engine = kndo::open(project_dir.path(), overrides)
        .expect("kndo::open must succeed with a mixed adapter+plugin directory");

    let result = engine.check(kndo_core::engine::RunMode::Full);

    assert_eq!(
        result.files_claimed, 2,
        "both .kdemo files must be claimed by the auto-discovered WASM adapter"
    );

    let unused_symbols: Vec<&str> = result
        .findings
        .iter()
        .filter(|f| f.category == "unused")
        .filter_map(|f| f.location.symbol.as_deref())
        .collect();
    assert!(
        !unused_symbols.contains(&"root_target"),
        "the auto-discovered plugin's contribute_roots should have kept this reachable: \
         {unused_symbols:?}"
    );
    assert!(
        !unused_symbols.contains(&"wire_target"),
        "contribute_edges should have kept this reachable: {unused_symbols:?}"
    );
    assert!(
        unused_symbols.contains(&"trulyDead"),
        "the untouched control declaration must still be flagged: {unused_symbols:?}"
    );

    let unused_files: Vec<String> = result
        .findings
        .iter()
        .filter(|f| f.category == "unused" && f.subject_kind == "file")
        .filter_map(|f| f.location.path.as_ref())
        .map(|p| p.0.to_string())
        .collect();
    assert!(
        !unused_files.contains(&"noise.banner.kdemo".to_string()),
        "classify_file's Generated override should exempt this file: {unused_files:?}"
    );
}
