//! Proves the global-install activation path for `LanguageAdapter`s — the same mechanism
//! `crates/kndo/tests/global_plugin_activation.rs` proves for `Plugin`s, extended to
//! adapters. A `LanguageAdapter` dropped into `KNDO_PLUGIN_DIR` only joins
//! composition for a project whose files actually satisfy one of its
//! `AdapterDescriptor.activation` rules; a project-local `.kndo/plugins/` copy stays
//! unconditional either way (`external_adapter.rs` proves that half). Also proves
//! `kndo::global_adapter_candidates` reports the same candidate correctly in both states.
//!
//! Both scenarios run inside one `#[test]` (rather than two) for the same reason the plugin
//! test does: `KNDO_PLUGIN_DIR` is process-wide state, and cargo runs `#[test]`s concurrently.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kndo has two ancestors up to the workspace root")
        .to_path_buf()
}

fn build_adapter_demo_component() -> Vec<u8> {
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
        .expect("failed to invoke cargo for examples/kndo-plugin-demo");
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

fn claimed_adapter_ids(root: &Path) -> Vec<String> {
    let engine =
        kndo::open(root, kndo_core::engine::ConfigOverrides::default()).expect("kndo::open");
    engine.doctor().adapters.into_iter().map(|a| a.id).collect()
}

#[test]
fn a_globally_installed_adapter_only_activates_when_its_rule_matches() {
    let adapter_bytes = build_adapter_demo_component();

    let global_dir = tempfile::tempdir().expect("temp global plugin dir");
    std::fs::write(global_dir.path().join("kdemo.wasm"), &adapter_bytes).unwrap();
    // SAFETY: sequential within this one #[test] function, same reasoning as the plugin test.
    unsafe {
        std::env::set_var("KNDO_PLUGIN_DIR", global_dir.path());
    }

    let fixture = "pub fn main() {\n    helper();\n}\nfn helper() {\n}\nfn dead() {\n}\n";
    let overrides = kndo_core::engine::ConfigOverrides {
        use_cache: false,
        threads: Some(1),
        min_confidence: None,
    };

    // Scenario 1: no `*.kdemo-enable` marker — the globally installed adapter's own activation
    // rule must NOT match, so `.kdemo` files stay unclaimed by anything.
    let project_without = tempfile::tempdir().expect("temp project fixture dir");
    std::fs::write(project_without.path().join("program.kdemo"), fixture).unwrap();
    assert!(
        !claimed_adapter_ids(project_without.path()).contains(&"kdemo".to_string()),
        "with no *.kdemo-enable marker the globally installed adapter must stay inactive"
    );
    let mut engine_without =
        kndo::open(project_without.path(), overrides.clone()).expect("kndo::open (inactive)");
    let result_without = engine_without.check(kndo_core::engine::RunMode::Full);
    assert!(
        result_without.findings.is_empty(),
        "an unclaimed .kdemo file must produce no findings at all: {:?}",
        result_without.findings
    );
    let candidates_without = kndo::global_adapter_candidates(project_without.path());
    assert_eq!(candidates_without.len(), 1);
    assert_eq!(candidates_without[0].id, "kdemo");
    assert!(candidates_without[0].active.is_none());
    assert!(candidates_without[0]
        .activation
        .iter()
        .any(|r| r.contains("*.kdemo-enable")));

    // Scenario 2: same project, plus the marker file — the rule matches, the adapter joins
    // composition, and the previously invisible file is claimed and analyzed for real.
    let project_with = tempfile::tempdir().expect("temp project fixture dir");
    std::fs::write(project_with.path().join("program.kdemo"), fixture).unwrap();
    std::fs::write(project_with.path().join("marker.kdemo-enable"), "").unwrap();
    assert!(
        claimed_adapter_ids(project_with.path()).contains(&"kdemo".to_string()),
        "with the marker file present the globally installed adapter must activate"
    );
    let mut engine_with = kndo::open(project_with.path(), overrides).expect("kndo::open (active)");
    let result_with = engine_with.check(kndo_core::engine::RunMode::Full);
    let unused_symbols: Vec<&str> = result_with
        .findings
        .iter()
        .filter(|f| f.category == "unused")
        .filter_map(|f| f.location.symbol.as_deref())
        .collect();
    assert!(
        unused_symbols.contains(&"dead"),
        "once claimed, the adapter's own reachability rules must fire for real: {unused_symbols:?}"
    );
    let candidates_with = kndo::global_adapter_candidates(project_with.path());
    assert_eq!(candidates_with.len(), 1);
    assert_eq!(
        candidates_with[0].active,
        Some(kndo::ActivationReason::RuleMatched(
            kndo::plugin::ActivationRule::FileExists("*.kdemo-enable".into())
        ))
    );

    // Claim priority: project-local > global > compiled-in. Drop a second copy of the same
    // component project-local, in the same project the global one is already active for,
    // and check the composed order directly — `adapter_resolution` lists exactly what
    // `open`'s adapters `Vec` contains, in the order claim resolution scans it. This test is
    // only about whether composition puts project-local ahead of global ahead of builtin.
    let plugins_dir = project_with.path().join(".kndo").join("plugins");
    std::fs::create_dir_all(&plugins_dir).unwrap();
    std::fs::write(plugins_dir.join("kdemo.wasm"), &adapter_bytes).unwrap();
    let resolution = kndo::adapter_resolution(project_with.path());
    let kdemo_sources: Vec<kndo::AdapterSource> = resolution
        .adapters
        .iter()
        .filter(|a| a.id == "kdemo")
        .map(|a| a.source)
        .collect();
    assert_eq!(
        kdemo_sources,
        vec![
            kndo::AdapterSource::ProjectLocal,
            kndo::AdapterSource::Global
        ],
        "project-local must be listed — and so claim-resolved — ahead of the global copy: \
         {kdemo_sources:?}"
    );
    let builtin_positions: Vec<usize> = resolution
        .adapters
        .iter()
        .enumerate()
        .filter(|(_, a)| a.source == kndo::AdapterSource::Builtin)
        .map(|(i, _)| i)
        .collect();
    let kdemo_positions: Vec<usize> = resolution
        .adapters
        .iter()
        .enumerate()
        .filter(|(_, a)| a.id == "kdemo")
        .map(|(i, _)| i)
        .collect();
    assert!(
        builtin_positions
            .iter()
            .all(|b| kdemo_positions.iter().all(|k| k < b)),
        "compiled-in adapters must never outrank an external one for claim priority: \
         kdemo at {kdemo_positions:?}, builtins at {builtin_positions:?}"
    );

    // SAFETY: same reasoning as the set_var above.
    unsafe {
        std::env::remove_var("KNDO_PLUGIN_DIR");
    }
}
