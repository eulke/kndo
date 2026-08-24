//! Proves the global-install activation path: a `Plugin` dropped into
//! `KNDO_PLUGIN_DIR` (the override for the platform XDG data dir — see
//! `kndo::activation::global_plugin_dir`) only joins composition for a project whose files
//! actually satisfy one of its `PluginDescriptor.activation` rules. Reuses the same
//! `examples/kndo-plugin-hooks-demo` component `external_plugin.rs` already builds — that test
//! proves the project-local `.kndo/plugins/` path is unconditional; this one proves the global
//! path is not. Also proves `kndo::global_plugin_candidates` (the `kndo doctor` visibility into
//! *skipped* global candidates, not just the composed set `Engine::doctor` sees) reports the
//! same candidate correctly in both states.
//!
//! Both scenarios run inside one `#[test]` (rather than two) because `KNDO_PLUGIN_DIR` is
//! process-wide state — cargo runs a test binary's `#[test]` functions concurrently by default,
//! so splitting this into two would race.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kndo has two ancestors up to the workspace root")
        .to_path_buf()
}

/// A process-unique `--target-dir` (not the demo crate's own shared `target/`) — see the
/// identical helper's doc comment in `external_adapter.rs` for why: several independent test
/// binaries build these same demo crates, and under `cargo test --workspace`'s default
/// parallelism a reader has been observed to pick up a wrong-shaped artifact from a
/// concurrent writer despite cargo's own target-dir lock.
fn isolated_target_dir() -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before the epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("kndo-wasm-target-{}-{nonce}", std::process::id()))
}

fn build_component(example_dir: &str, wasm_name: &str) -> Vec<u8> {
    let demo_dir = workspace_root().join(example_dir);
    let target_dir = isolated_target_dir();
    let status = Command::new("cargo")
        .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
        // Cross-target guest build: instrumentation flags from the host environment
        // (cargo-llvm-cov's `-C instrument-coverage` in RUSTFLAGS) must not leak into a
        // target that cannot link the profiling runtime.
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("LLVM_PROFILE_FILE")
        .env("CARGO_TARGET_DIR", &target_dir)
        .current_dir(&demo_dir)
        .status()
        .unwrap_or_else(|e| panic!("failed to invoke cargo for {example_dir}: {e}"));
    assert!(status.success(), "{example_dir} guest build failed");

    let core_wasm_path = target_dir
        .join("wasm32-unknown-unknown/release")
        .join(wasm_name);
    let core_wasm = std::fs::read(&core_wasm_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", core_wasm_path.display()));
    let _ = std::fs::remove_dir_all(&target_dir);

    wit_component::ComponentEncoder::default()
        .module(&core_wasm)
        .expect("attaching the core module to the component encoder")
        .encode()
        .expect("encoding the component")
}

fn unused_symbols(result: &kndo_core::engine::RunResult) -> Vec<&str> {
    result
        .findings
        .iter()
        .filter(|f| f.category == "unused")
        .filter_map(|f| f.location.symbol.as_deref())
        .collect()
}

#[test]
fn a_globally_installed_plugin_only_activates_when_its_rule_matches() {
    // The adapter demo stays project-local (the global-activation path is `Plugin`-only, see
    // `kndo::external_adapters`'s doc comment) — every project below needs it just to get its
    // `.kdemo` file claimed at all, independent of what's under test here.
    let adapter_bytes = build_component("examples/kndo-plugin-demo", "kndo_plugin_demo.wasm");
    let plugin_bytes = build_component(
        "examples/kndo-plugin-hooks-demo",
        "kndo_plugin_hooks_demo.wasm",
    );

    // The "machine": one global plugin directory, shared across whatever projects this test
    // opens below — exactly the real-world shape (installed once, used by many projects).
    let global_dir = tempfile::tempdir().expect("temp global plugin dir");
    std::fs::write(global_dir.path().join("hooks-demo.wasm"), &plugin_bytes).unwrap();
    // SAFETY: this test's own process — no other thread reads env concurrently within it, and
    // every `kndo::open` call below happens sequentially in this one #[test] function.
    unsafe {
        std::env::set_var("KNDO_PLUGIN_DIR", global_dir.path());
    }

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
"#;
    let overrides = kndo_core::engine::ConfigOverrides {
        use_cache: false,
        threads: Some(1),
        min_confidence: None,
    };

    // Scenario 1: no `*.trigger` file anywhere — the global plugin's own activation rule
    // (`FileExists("*.trigger")`) must NOT match, so it never joins composition.
    let project_without_trigger = tempfile::tempdir().expect("temp project fixture dir");
    let plugins_without = project_without_trigger.path().join(".kndo").join("plugins");
    std::fs::create_dir_all(&plugins_without).unwrap();
    std::fs::write(plugins_without.join("kdemo.wasm"), &adapter_bytes).unwrap();
    std::fs::write(
        project_without_trigger.path().join("program.kdemo"),
        fixture,
    )
    .unwrap();
    let mut engine_without = kndo::open(project_without_trigger.path(), overrides.clone())
        .expect("kndo::open (no trigger file)");
    let result_without = engine_without.check(kndo_core::engine::CheckRequest {
        mode: kndo_core::engine::RunMode::Full,
    });
    let unused_without = unused_symbols(&result_without);
    assert!(
        unused_without.contains(&"root_target") && unused_without.contains(&"wire_target"),
        "with no *.trigger file the globally installed plugin must stay inactive: {unused_without:?}"
    );
    // Doctor visibility: the candidate must still be reported, just not activated — a skipped
    // global plugin isn't invisible, unlike a plugin that never made it into `Engine` at all.
    let candidates_without = kndo::global_plugin_candidates(project_without_trigger.path());
    assert_eq!(candidates_without.len(), 1);
    assert_eq!(candidates_without[0].id, "hooks-demo");
    assert!(!candidates_without[0].activated);
    assert!(candidates_without[0]
        .activation
        .iter()
        .any(|r| r.contains("*.trigger")));

    // Scenario 2: same project, plus a `*.trigger` file — now the rule matches, and the exact
    // same globally installed component must join composition and rescue both symbols.
    let project_with_trigger = tempfile::tempdir().expect("temp project fixture dir");
    let plugins_with = project_with_trigger.path().join(".kndo").join("plugins");
    std::fs::create_dir_all(&plugins_with).unwrap();
    std::fs::write(plugins_with.join("kdemo.wasm"), &adapter_bytes).unwrap();
    std::fs::write(project_with_trigger.path().join("program.kdemo"), fixture).unwrap();
    std::fs::write(project_with_trigger.path().join("ecosystem.trigger"), "").unwrap();
    let mut engine_with =
        kndo::open(project_with_trigger.path(), overrides).expect("kndo::open (with trigger file)");
    let result_with = engine_with.check(kndo_core::engine::CheckRequest {
        mode: kndo_core::engine::RunMode::Full,
    });
    let unused_with = unused_symbols(&result_with);
    assert!(
        !unused_with.contains(&"root_target"),
        "with a *.trigger file the globally installed plugin's contribute_roots should have \
         kept this reachable: {unused_with:?}"
    );
    assert!(
        !unused_with.contains(&"wire_target"),
        "with a *.trigger file the globally installed plugin's contribute_edges should have \
         kept this reachable: {unused_with:?}"
    );
    let candidates_with = kndo::global_plugin_candidates(project_with_trigger.path());
    assert_eq!(candidates_with.len(), 1);
    assert!(candidates_with[0].activated);

    // SAFETY: same reasoning as the set_var above — sequential within this one test.
    unsafe {
        std::env::remove_var("KNDO_PLUGIN_DIR");
    }
}
