//! Compliance suite for the `coverage-ingester` world: proves the unidirectional coverage
//! ABI — host locates/freshness-checks/pushes bytes, guest returns facts, host writes the
//! sink and rebases — through a real `kndo_core::engine::Engine`, same discipline as
//! `plugin_compliance.rs`. The demo guest (`examples/kndo-coverage-demo`) is always built
//! fresh from source and componentized in-process; nothing is checked in as a binary.

use std::path::{Path, PathBuf};
use std::process::Command;

use kndo_core::adapter::ProjectPath;
use kndo_core::coverage::CoverageSink;
use kndo_core::engine::{CheckRequest, ConfigOverrides, Engine, RunMode};
use kndo_core::plugin::Plugin;
use kndo_plugin_api::{WasmCoverageIngester, WasmPlugin};

#[path = "harness/mini_adapter.rs"]
mod mini_adapter;
use self::mini_adapter::MiniAdapter;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kndo-plugin-api has two ancestors up to the workspace root")
        .to_path_buf()
}

/// Same isolation as `plugin_compliance.rs`: a process-unique `--target-dir`, since several
/// test binaries build demo crates concurrently under `cargo test --workspace`.
fn isolated_target_dir() -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before the epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("kndo-wasm-target-{}-{nonce}", std::process::id()))
}

fn build_coverage_demo_component() -> Vec<u8> {
    let demo_dir = workspace_root().join("examples/kndo-coverage-demo");
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
        .expect("failed to invoke cargo to build the coverage demo");
    assert!(status.success(), "coverage demo guest build failed");

    let core_wasm_path = target_dir.join("wasm32-unknown-unknown/release/kndo_coverage_demo.wasm");
    let core_wasm = std::fs::read(&core_wasm_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", core_wasm_path.display()));
    let _ = std::fs::remove_dir_all(&target_dir);
    wit_component::ComponentEncoder::default()
        .module(&core_wasm)
        .expect("attaching the core module to the component encoder")
        .encode()
        .expect("encoding the coverage demo as a WASM component")
}

#[test]
fn coverage_ingester_world_end_to_end() {
    let component_bytes = build_coverage_demo_component();
    let dir = tempfile::tempdir().expect("temp dir for component artifacts");
    let component_path = dir.path().join("coverage-demo.wasm");
    std::fs::write(&component_path, &component_bytes).unwrap();

    // Load contract: the coverage loader accepts it; the graph-hooks loader rejects it
    // (disjoint export sets — the mixed-directory sorting mechanism), and vice versa is
    // covered by plugin_compliance's own mutual-rejection matrix.
    assert!(
        WasmPlugin::load(&component_path).is_err(),
        "a coverage component has none of the plugin world's exports — must fail"
    );
    let ingester = WasmCoverageIngester::load(&component_path).expect("coverage demo must load");

    let descriptor = ingester.descriptor();
    assert_eq!(descriptor.id, "coverage-demo");
    assert_eq!(descriptor.requested_file_access, vec!["cov.demo"]);
    assert!(
        !ingester.mutates_graph(),
        "the coverage world is structurally mutation-free — both graph fast paths stay alive"
    );
    assert_eq!(
        ingester.content_hash(),
        Some(*blake3::hash(&component_bytes).as_bytes()),
        "content hash must be over the raw component bytes"
    );

    // Direct hook call: guest facts land in the host's sink verbatim (paths as reported —
    // rebasing belongs to the engine, exercised below).
    let mut sink = CoverageSink::default();
    ingester.ingest_coverage(
        &ProjectPath("cov.demo".into()),
        b"# demo report\nsrc/a.mock:1=1\nsrc/a.mock:2=0\nnot a record\n",
        &mut sink,
    );
    let map = sink.into_map();
    let cov = map
        .function_coverage(
            &ProjectPath("src/a.mock".into()),
            kndo_core::adapter::Span {
                start: (1, 1),
                end: (5, 1),
            },
        )
        .expect("the guest's line facts must reach the sink");
    assert!((cov - 0.5).abs() < 1e-9);

    // A trap-shaped failure contributes nothing: unparseable bytes yield an empty map,
    // never an error surfaced to the caller.
    let mut sink = CoverageSink::default();
    ingester.ingest_coverage(&ProjectPath("cov.demo".into()), &[0xff, 0xfe], &mut sink);
    assert!(sink.into_map().files.is_empty());

    // End to end through a real Engine: the host finds cov.demo at the descriptor's
    // well-known path, freshness-checks it, calls the guest, and crap runs with the
    // ingested coverage instead of skipping — the whole unidirectional contract at once.
    let project = tempfile::tempdir().expect("temp project");
    std::fs::write(project.path().join("a.mock"), "decl covered\n").unwrap();
    std::fs::write(project.path().join("cov.demo"), "a.mock:1=1\n").unwrap();
    let ingester = WasmCoverageIngester::load(&component_path).expect("fresh load for the engine");
    let mut engine = Engine::open_with_plugins(
        project.path(),
        ConfigOverrides::default(),
        vec![Box::new(MiniAdapter)],
        vec![Box::new(ingester)],
    )
    .expect("engine over the mini adapter and the WASM ingester");
    let result = engine.check(CheckRequest {
        mode: RunMode::Full,
    });
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.message.starts_with("crap: no coverage ingested")),
        "the WASM-ingested report must reach the analyses: {:?}",
        result.diagnostics
    );
}
