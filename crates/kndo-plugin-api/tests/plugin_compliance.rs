//! Compliance suite for the `kndo:plugin` v1 ABI (docs/contracts/wasm-abi.md §5): proves the
//! bidirectional bridge — the guest calling back into `list-files`/`symbols-in`, not just
//! reporting facts about one file the way the adapter ABI does — through a real
//! `kndo_core::engine::Engine`, same discipline as `compliance.rs`'s adapter test. The demo
//! guest (`examples/kndo-plugin-hooks-demo`) is always built fresh from source and componentized
//! in-process; nothing is checked in as a binary.

use std::path::{Path, PathBuf};
use std::process::Command;

use kndo_core::adapter::{
    AdapterDescriptor, Declaration, FileClaim, FileFacts, LanguageAdapter, ManifestFacts,
    RawImport, RawReference, RawRoot, RawRootTarget, ResolveCtx, SourceFile, Span,
};
use kndo_core::adapter::{ImportKind, ImportSpec, Resolution};
use kndo_core::engine::{CheckRequest, ConfigOverrides, Engine, RunMode};
use kndo_core::plugin::Plugin;
use kndo_core::vocab::{Confidence, RefKind, RootKind, SymbolKind};
use kndo_plugin_api::WasmPlugin;
use smol_str::SmolStr;

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

fn build_adapter_demo_component() -> Vec<u8> {
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

/// The two WASM ABIs (`kndo:adapter`, `kndo:plugin`) are structurally different worlds — a
/// component built for one has none of the other's required exports. Proves that mismatch is
/// caught by wasmtime's own component type-checking at `instantiate` time, not silently
/// tolerated: this is exactly the mechanism `kndo::external_plugins`'s "try both loaders, use
/// whichever succeeds" discovery design depends on to sort a mixed `.kndo/plugins/` directory
/// without any naming convention.
#[test]
fn each_abi_rejects_a_component_built_for_the_other() {
    let adapter_bytes = build_adapter_demo_component();
    let plugin_bytes = build_hooks_demo_component();
    let dir = tempfile::tempdir().expect("temp dir for component artifacts");

    let adapter_path = dir.path().join("adapter.wasm");
    std::fs::write(&adapter_path, &adapter_bytes).unwrap();
    let plugin_path = dir.path().join("plugin.wasm");
    std::fs::write(&plugin_path, &plugin_bytes).unwrap();

    assert!(
        kndo_plugin_api::WasmAdapter::load(&adapter_path).is_ok(),
        "the adapter component must load through the adapter loader"
    );
    assert!(
        WasmPlugin::load(&plugin_path).is_ok(),
        "the plugin component must load through the plugin loader"
    );
    assert!(
        WasmPlugin::load(&adapter_path).is_err(),
        "an adapter component has none of the plugin world's exports — must fail to instantiate"
    );
    assert!(
        kndo_plugin_api::WasmAdapter::load(&plugin_path).is_err(),
        "a plugin component has none of the adapter world's exports — must fail to instantiate"
    );
}

fn build_hooks_demo_component() -> Vec<u8> {
    let demo_dir = workspace_root().join("examples/kndo-plugin-hooks-demo");
    let target_dir = isolated_target_dir();
    let status = Command::new("cargo")
        .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
        .env("CARGO_TARGET_DIR", &target_dir)
        .current_dir(&demo_dir)
        .status()
        .expect("failed to invoke cargo to build the demo plugin");
    assert!(status.success(), "demo plugin guest build failed");

    let core_wasm_path =
        target_dir.join("wasm32-unknown-unknown/release/kndo_plugin_hooks_demo.wasm");
    let core_wasm = std::fs::read(&core_wasm_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", core_wasm_path.display()));
    let _ = std::fs::remove_dir_all(&target_dir);

    wit_component::ComponentEncoder::default()
        .module(&core_wasm)
        .expect("attaching the core module to the component encoder")
        .encode()
        .expect("encoding the demo plugin as a WASM component")
}

/// A minimal text-driven `LanguageAdapter` local to this test — the same tiny grammar
/// `kndo-core`'s own `DiffMockAdapter` uses (`decl <name>`, `ref <name>`, `root-file`,
/// `import ./sibling.mock`), reimplemented here rather than depending on kndo-core's
/// private test module. Its only job is giving the WASM plugin under test a real graph to
/// query and mutate.
struct MiniAdapter;

impl LanguageAdapter for MiniAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            activation: Vec::new(),
            dependencies: Vec::new(),
            id: SmolStr::new("mini"),
            facts_schema_version: 1,
            file_globs: vec![SmolStr::new("**/*.mock")],
            manifest_globs: vec![],
            grammar_version: SmolStr::new("mini"),
            visibility_ladder: vec![
                kndo_core::adapter::VisibilityRung {
                    scope: kndo_core::adapter::VisibilityScope::Unit,
                    label: SmolStr::new("private"),
                },
                kndo_core::adapter::VisibilityRung {
                    scope: kndo_core::adapter::VisibilityScope::Public,
                    label: SmolStr::new("exported"),
                },
            ],
            cycle_policy: kndo_core::adapter::CyclePolicy {
                file_cycles: kndo_core::adapter::CycleTolerance::Idiomatic,
                package_cycles: kndo_core::adapter::CycleTolerance::Idiomatic,
            },
            resolves_dependency_usage: false,
        }
    }

    fn claim(&self, path: &kndo_core::adapter::ProjectPath) -> Option<FileClaim> {
        path.0.ends_with(".mock").then(|| FileClaim {
            language: SmolStr::new("mini"),
            class: Default::default(),
        })
    }

    fn claim_manifest(&self, _path: &kndo_core::adapter::ProjectPath) -> bool {
        false
    }

    fn extract(&self, file: &SourceFile<'_>) -> FileFacts {
        let text = std::str::from_utf8(file.content).unwrap_or("");
        let mut facts = FileFacts::default();
        for line in text.lines() {
            if line == "root-file" {
                facts.roots.push(RawRoot {
                    kind: RootKind::Production,
                    target: RawRootTarget::WholeFile,
                    confidence: Confidence::Certain,
                });
            } else if let Some(spec) = line.strip_prefix("import ") {
                facts.imports.push(RawImport {
                    specifier: SmolStr::new(spec),
                    kind: ImportKind::Relative,
                    span: Span::default(),
                    side_effect_only: true,
                    type_only: false,
                    confidence: Confidence::Certain,
                    bindings: vec![],
                    reexported: false,
                    opaque_namespace_use: false,
                    local_alias: None,
                });
            } else if let Some(name) = line.strip_prefix("decl ") {
                facts.declarations.push(Declaration {
                    name: SmolStr::new(name),
                    kind: SymbolKind::Function,
                    span: Span::default(),
                    exported: true,
                    visibility: kndo_core::adapter::VisibilityLevel(1),
                    member_of: None,
                    signature_span: None,
                });
            } else if let Some(name) = line.strip_prefix("ref ") {
                facts.references.push(RawReference {
                    name: SmolStr::new(name),
                    scope_context: None,
                    span: Span::default(),
                    within: None,
                    kind: RefKind::Read,
                });
            }
        }
        facts
    }

    fn extract_manifest(&self, _file: &SourceFile<'_>, _ctx: &ResolveCtx<'_>) -> ManifestFacts {
        ManifestFacts::default()
    }

    fn resolve(&self, spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
        let Some(rel) = spec.specifier.strip_prefix("./") else {
            return Resolution::Unresolved;
        };
        let dir = spec.from.0.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let candidate = if dir.is_empty() {
            rel.to_string()
        } else {
            format!("{dir}/{rel}")
        };
        let path = kndo_core::adapter::ProjectPath(SmolStr::new(candidate));
        if ctx.contains(&path) {
            Resolution::File(path, Confidence::Certain)
        } else {
            Resolution::Unresolved
        }
    }
}

#[test]
fn external_wasm_plugin_hooks_affect_a_real_check() {
    let component_bytes = build_hooks_demo_component();
    let component_dir = tempfile::tempdir().expect("temp dir for the component artifact");
    let component_path = component_dir.path().join("hooks-demo.wasm");
    std::fs::write(&component_path, &component_bytes).expect("writing the component artifact");

    let plugin = WasmPlugin::load(&component_path).expect("loading the demo WASM plugin");
    let descriptor = plugin.descriptor();
    assert_eq!(descriptor.id.as_str(), "hooks-demo");
    // RFC 0015 §3 wire round-trip: the guest declares its activation rule and (empty)
    // dependency list; both must survive the WIT boundary — the composition layer's fixpoint
    // (kndo's own unit tests) is only as real as this transport.
    assert_eq!(descriptor.activation.len(), 1);
    assert!(descriptor.dependencies.is_empty());
    // RFC 0016 §6: WasmPlugin::content_hash() must be the real component bytes' own hash, not
    // a placeholder — this is the graph cache key's proof that a swapped .wasm file (even with
    // an unchanged declared version) invalidates a stale snapshot.
    assert_eq!(
        Plugin::content_hash(&plugin),
        Some(*blake3::hash(&component_bytes).as_bytes())
    );

    let project_dir = tempfile::tempdir().expect("temp project fixture dir");
    std::fs::write(
        project_dir.path().join("root.mock"),
        "root-file\nimport ./aux.mock\n",
    )
    .unwrap();
    std::fs::write(
        project_dir.path().join("aux.mock"),
        "decl root_target\n\
         decl wire_target\n\
         decl consumed_x\n\
         ref consumed_x\n\
         decl trulyDead\n\
         decl content_target\n",
    )
    .unwrap();
    std::fs::write(
        project_dir.path().join("noise.banner.mock"),
        "decl bannerDecl\n",
    )
    .unwrap();
    // RFC 0016 §5: content contribute_roots reads through read-file to decide whether to root
    // content_target — proves the WIT host import reaches a real guest computation.
    std::fs::write(project_dir.path().join("content.demo"), "promote").unwrap();

    // Baseline: without the plugin, every one of the four scenarios the plugin later rescues
    // must actually be flagged on its own, so the assertions below can't pass vacuously.
    let mut baseline = Engine::open(
        project_dir.path(),
        ConfigOverrides {
            use_cache: false,
            threads: Some(1),
        },
        vec![Box::new(MiniAdapter)],
    )
    .expect("opening the baseline engine");
    let baseline_result = baseline.check(CheckRequest {
        mode: RunMode::Full,
    });
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
    assert!(
        baseline_unused.contains(&"content_target"),
        "{baseline_unused:?}"
    );
    let baseline_unused_files: Vec<String> = baseline_result
        .findings
        .iter()
        .filter(|f| f.category == "unused" && f.subject_kind == "file")
        .filter_map(|f| f.location.path.as_ref())
        .map(|p| p.0.to_string())
        .collect();
    assert!(baseline_unused_files.contains(&"noise.banner.mock".to_string()));
    let baseline_internal_only: Vec<&str> = baseline_result
        .findings
        .iter()
        .filter(|f| f.category == "internal-only")
        .filter_map(|f| f.location.symbol.as_deref())
        .collect();
    assert!(
        baseline_internal_only.contains(&"consumed_x"),
        "{baseline_internal_only:?}"
    );

    let mut engine = Engine::open_with_plugins(
        project_dir.path(),
        ConfigOverrides {
            use_cache: false,
            threads: Some(1),
        },
        vec![Box::new(MiniAdapter)],
        vec![Box::new(plugin)],
    )
    .expect("opening the plugin-enabled engine");
    let result = engine.check(CheckRequest {
        mode: RunMode::Full,
    });

    let unused_symbols: Vec<&str> = result
        .findings
        .iter()
        .filter(|f| f.category == "unused")
        .filter_map(|f| f.location.symbol.as_deref())
        .collect();
    assert!(
        !unused_symbols.contains(&"root_target"),
        "contribute_roots (via list-files/symbols-in) should have kept this reachable: \
         {unused_symbols:?}"
    );
    assert!(
        !unused_symbols.contains(&"wire_target"),
        "contribute_edges should have kept this reachable: {unused_symbols:?}"
    );
    assert!(
        !unused_symbols.contains(&"content_target"),
        "the read-file-gated root should have kept this reachable: {unused_symbols:?}"
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
        !unused_files.contains(&"noise.banner.mock".to_string()),
        "classify_file's Generated override should exempt this file: {unused_files:?}"
    );

    let internal_only_symbols: Vec<&str> = result
        .findings
        .iter()
        .filter(|f| f.category == "internal-only")
        .filter_map(|f| f.location.symbol.as_deref())
        .collect();
    assert!(
        !internal_only_symbols.contains(&"consumed_x"),
        "annotate_symbols should have exempted this: {internal_only_symbols:?}"
    );
}
