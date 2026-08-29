//! Proves the adapter co-activation fixpoint with real WASM components — the adapter-tier
//! mirror of what `global_plugin_activation.rs` proves for plugin implication. Two
//! components sit in the global directory: the kdemo demo adapter (activation
//! `FileExists("*.kdemo-enable")`) and the kwrap wrapper adapter (activation
//! `FileExists("*.kwrap-enable")`, `dependencies: ["kdemo"]`). The project carries only the
//! *wrapper's* marker — kdemo's own rule never fires — yet kdemo must join composition as
//! `ImpliedBy("kwrap")` and claim `.kdemo` files for real analysis.
//!
//! Own test binary (not another `#[test]` in `global_adapter_activation.rs`): `KNDO_PLUGIN_DIR`
//! is process-wide state, and separate binaries are separate processes.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kndo has two ancestors up to the workspace root")
        .to_path_buf()
}

/// Builds one of the `examples/` demo guests and encodes it as a WASM component.
fn build_component(example_dir: &str, artifact: &str) -> Vec<u8> {
    let demo_dir = workspace_root().join(example_dir);
    // A `TempDir`: unique by construction and removed on drop, unwind included — a hand-rolled
    // fixed name would leak the whole build tree if the process panics before cleanup.
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
        .join(format!("wasm32-unknown-unknown/release/{artifact}"));
    let core_wasm = std::fs::read(&core_wasm_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", core_wasm_path.display()));

    wit_component::ComponentEncoder::default()
        .module(&core_wasm)
        .expect("attaching the core module to the component encoder")
        .encode()
        .expect("encoding the demo guest as a WASM component")
}

#[test]
fn an_active_adapters_dependencies_imply_a_globally_installed_adapter() {
    let kdemo_bytes = build_component("examples/kndo-plugin-demo", "kndo_plugin_demo.wasm");
    let kwrap_bytes = build_component(
        "examples/kndo-adapter-wrapper-demo",
        "kndo_adapter_wrapper_demo.wasm",
    );

    let global_dir = tempfile::tempdir().expect("temp global plugin dir");
    std::fs::write(global_dir.path().join("kdemo.wasm"), &kdemo_bytes).unwrap();
    std::fs::write(global_dir.path().join("kwrap.wasm"), &kwrap_bytes).unwrap();
    // SAFETY: this binary holds exactly one #[test], so the var is set/removed sequentially.
    unsafe {
        std::env::set_var("KNDO_PLUGIN_DIR", global_dir.path());
    }

    // Only the WRAPPER's marker exists — `*.kdemo-enable` deliberately absent, so any kdemo
    // activation observable below can only have come through kwrap's `dependencies`.
    let project = tempfile::tempdir().expect("temp project fixture dir");
    std::fs::write(
        project.path().join("program.kdemo"),
        "pub fn main() {\n    helper();\n}\nfn helper() {\n}\nfn dead() {\n}\n",
    )
    .unwrap();
    std::fs::write(project.path().join("marker.kwrap-enable"), "").unwrap();

    let candidates = kndo::global_adapter_candidates(project.path());
    let by_id = |id: &str| {
        candidates.iter().find(|c| c.id == id).unwrap_or_else(|| {
            panic!(
                "candidate {id} missing from {candidates:?}",
                candidates = candidates.iter().map(|c| c.id.as_str()).collect::<Vec<_>>()
            )
        })
    };
    assert_eq!(
        by_id("kwrap").active,
        Some(kndo::ActivationReason::RuleMatched(
            kndo::plugin::ActivationRule::FileExists("*.kwrap-enable".into())
        )),
        "the wrapper's own *.kwrap-enable rule must have fired"
    );
    assert_eq!(
        by_id("kdemo").active,
        Some(kndo::ActivationReason::ImpliedBy("kwrap".into())),
        "kdemo's own rule never matched — it must be active purely as kwrap's dependency"
    );
    assert!(
        kndo::adapter_resolution(project.path())
            .missing_dependencies
            .is_empty(),
        "kdemo is present, so kwrap's dependency declaration is satisfied"
    );

    // Implication must be real composition, not a doctor-only label: the implied adapter
    // claims and analyzes `.kdemo` files exactly as if its own rule had matched.
    let overrides = kndo_core::engine::ConfigOverrides {
        use_cache: false,
        threads: Some(1),
        ..kndo_core::engine::ConfigOverrides::default()
    };
    let mut engine = kndo::open(project.path(), overrides).expect("kndo::open");
    let result = engine.check(kndo_core::engine::RunMode::Full);
    let unused_symbols: Vec<&str> = result
        .findings
        .iter()
        .filter(|f| f.category == "unused")
        .filter_map(|f| f.location.symbol.as_deref())
        .collect();
    assert!(
        unused_symbols.contains(&"dead"),
        "the implied adapter's reachability rules must fire for real: {unused_symbols:?}"
    );

    // SAFETY: same reasoning as the set_var above.
    unsafe {
        std::env::remove_var("KNDO_PLUGIN_DIR");
    }
}
