//! The ABI compatibility matrix: the two
//! reference components, **pre-built and committed** under `tests/compat/`, run against the
//! HEAD host on every push. The ABI promises a v1 component keeps working against every
//! v1-compatible host indefinitely — this test is that promise as a build-breaking fact
//! instead of a sentence. (Pre-1.0 the WIT may still evolve in place;
//! when it does, the pinned binaries are rebuilt *in the same commit* that
//! changes the WIT — that rebuild is the explicit, reviewable record of a compatibility
//! break, which is exactly what an unreviewable silent breakage isn't.)
//!
//! Deliberately no `cargo build --target wasm32-unknown-unknown` here, unlike every other
//! WASM-bridge test: needing the current toolchain to rebuild the guest would test "today's
//! source against today's host" — the compat question is *yesterday's binary* against
//! today's host, and the committed bytes are yesterday's binary.

use std::path::{Path, PathBuf};

use kndo_core::engine::{ConfigOverrides, Engine, RunMode};

use kndo_core::testkit::MockAdapter;

fn compat_component(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/compat")
        .join(name)
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
fn the_pinned_v1_adapter_component_still_works_against_the_head_host() {
    let adapter = kndo_plugin_api::WasmAdapter::load(&compat_component("adapter-v1.wasm"))
        .expect("the pinned v1 adapter component must load against the HEAD host");
    let descriptor = kndo_core::adapter::LanguageAdapter::descriptor(&adapter);
    assert_eq!(descriptor.id.as_str(), "kdemo");
    assert_eq!(descriptor.dependencies, Vec::<smol_str::SmolStr>::new());

    let project = tempfile::tempdir().expect("temp project fixture dir");
    std::fs::write(
        project.path().join("program.kdemo"),
        "pub fn main() {\n    helper();\n}\nfn helper() {\n}\nfn dead() {\n}\n",
    )
    .unwrap();
    let mut engine = Engine::open(
        project.path(),
        ConfigOverrides {
            use_cache: false,
            threads: Some(1),
            ..ConfigOverrides::default()
        },
        vec![Box::new(adapter)],
    )
    .expect("opening an engine over the pinned adapter");
    let result = engine.check(RunMode::Full);
    let unused = unused_symbols(&result);
    assert!(
        unused.contains(&"dead") && !unused.contains(&"helper"),
        "the pinned adapter's extraction must still drive real reachability: {unused:?}"
    );
}

#[test]
fn the_pinned_v1_plugin_component_still_works_against_the_head_host() {
    let plugin = kndo_plugin_api::WasmPlugin::load(&compat_component("plugin-v1.wasm"))
        .expect("the pinned v1 plugin component must load against the HEAD host");
    assert_eq!(
        kndo_core::plugin::Plugin::descriptor(&plugin).id.as_str(),
        "hooks-demo"
    );

    // The same fixture the compliance suite drives its freshly built copy through — every
    // rescue below exercises a different host import surface (list-files/symbols-in,
    // read-file, importers-of, call-sites-in, the round lifecycle), so a regression in any
    // v1 import fails a named assertion.
    let project = tempfile::tempdir().expect("temp project fixture dir");
    std::fs::write(
        project.path().join("root.mock"),
        "root-file\nimport ./aux.mock\n",
    )
    .unwrap();
    std::fs::write(
        project.path().join("aux.mock"),
        "decl root_target\n\
         decl wire_target\n\
         decl consumed_x\n\
         ref consumed_x\n\
         decl trulyDead\n\
         decl content_target\n\
         decl staged_target\n\
         decl fresh_target\n\
         decl linked_target\n\
         decl sited_target\n\
         callsite use.site promote\n",
    )
    .unwrap();
    std::fs::write(
        project.path().join("noise.banner.mock"),
        "decl bannerDecl\n",
    )
    .unwrap();
    std::fs::write(project.path().join("content.demo"), "promote").unwrap();

    let mut engine = Engine::open_with_plugins(
        project.path(),
        ConfigOverrides {
            use_cache: false,
            threads: Some(1),
            ..ConfigOverrides::default()
        },
        vec![Box::new(MockAdapter)],
        vec![Box::new(plugin)],
    )
    .expect("opening an engine over the pinned plugin");
    let result = engine.check(RunMode::Full);

    let unused = unused_symbols(&result);
    for rescued in [
        "root_target",
        "wire_target",
        "content_target",
        "staged_target",
        "fresh_target",
        "linked_target",
        "sited_target",
    ] {
        assert!(
            !unused.contains(&rescued),
            "the pinned plugin must still rescue {rescued}: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"trulyDead"),
        "the untouched control declaration must still be flagged: {unused:?}"
    );
    let unused_files: Vec<&str> = result
        .findings
        .iter()
        .filter(|f| f.category == "unused" && f.subject_kind == "file")
        .filter_map(|f| f.location.path.as_ref())
        .map(|p| p.0.as_str())
        .collect();
    assert!(
        !unused_files.contains(&"noise.banner.mock"),
        "classify_file's override must still exempt this file: {unused_files:?}"
    );
    let internal_only: Vec<&str> = result
        .findings
        .iter()
        .filter(|f| f.category == "internal-only")
        .filter_map(|f| f.location.symbol.as_deref())
        .collect();
    assert!(
        !internal_only.contains(&"consumed_x"),
        "annotate_symbols must still exempt this: {internal_only:?}"
    );
}
