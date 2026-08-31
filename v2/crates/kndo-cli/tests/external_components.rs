//! The M5 exit criterion, as a test: external components dropped into
//! `.kndo/plugins/` run through the SHIPPED frontend — the same `kndo check` a
//! user types, the same build-shell feature the release binary carries. The
//! components are the pinned compat-matrix binaries: what a third party would
//! have on disk, not something this test builds.

use kndo_cli::{Host, run_args};
use kndo_testkit::TempProject;
use std::path::Path;

fn install(p: &TempProject, component: &str) {
    let pinned = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../abi/compat");
    std::fs::create_dir_all(p.root().join(".kndo/plugins")).expect("plugins dir");
    std::fs::copy(
        pinned.join(component),
        p.root().join(".kndo/plugins").join(component),
    )
    .expect("install the pinned component");
}

#[test]
fn a_wasm_language_and_plugin_run_from_kndo_plugins_through_the_cli() {
    let p = TempProject::new();
    install(&p, "kmini_adapter.wasm");
    install(&p, "probe_plugin.wasm");
    p.file("app.kmini", "entry\nfn local_dead\n");
    p.file("wired.kmini", "fn wired_dead\n");
    p.file("config.probe", "sixteen bytes!!\n");

    let root = p.root().to_string_lossy().into_owned();
    let out = run_args(
        ["kndo", "check", &root, "--no-cache"],
        Host {
            tty: false,
            format_env: None,
            no_color: false,
        },
    );
    let report: serde_json::Value = serde_json::from_str(&out.stdout).expect("stdout is JSON");

    // The kmini language exists to this binary only through the component.
    assert_eq!(
        report["run"]["extensions"]
            .as_array()
            .and_then(|a| a.iter().find(|e| e["id"] == "kmini"))
            .and_then(|e| e["files"].as_u64()),
        Some(2),
        "{}",
        out.stdout
    );
    // Its extraction drives real findings…
    let findings = report["findings"].as_array().expect("findings");
    assert!(
        findings
            .iter()
            .any(|f| f["category"] == "unused" && out_contains(f, "local_dead")),
        "{}",
        out.stdout
    );
    // …and the probe plugin contributed beside the built-ins (which stay first
    // in registration order): a root that keeps wired.kmini, a finding from its
    // scoped read, and its described drops.
    let contribution = report["plugins"]
        .as_array()
        .and_then(|c| c.iter().find(|e| e["coordinate"] == "demo:probe"))
        .unwrap_or_else(|| panic!("demo:probe contributed: {}", out.stdout));
    assert_eq!(contribution["roots"], 1);
    assert!(
        findings
            .iter()
            .any(|f| f["category"] == "ext:demo:probe/note"
                && f["message"] == "config.probe is 16 bytes"),
        "{}",
        out.stdout
    );
    assert!(
        !findings
            .iter()
            .any(|f| f["subject"]["kind"] == "file" && f["subject"]["path"] == "wired.kmini"),
        "the component's root keeps the file: {}",
        out.stdout
    );
}

#[test]
fn a_broken_component_degrades_to_a_visible_diagnostic() {
    let p = TempProject::new();
    std::fs::create_dir_all(p.root().join(".kndo/plugins")).expect("plugins dir");
    std::fs::write(p.root().join(".kndo/plugins/broken.wasm"), b"not wasm").expect("write");
    p.file("index.js", "export function api() { return 1; }\n");
    p.file("package.json", r#"{ "name": "demo", "main": "index.js" }"#);

    let root = p.root().to_string_lossy().into_owned();
    let out = run_args(
        ["kndo", "check", &root, "--no-cache"],
        Host {
            tty: true,
            format_env: None,
            no_color: false,
        },
    );
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert!(
        out.stdout.contains(
            "diagnostic .kndo/plugins/broken.wasm: not a loadable kndo:vocab extension component"
        ),
        "an opted-in component never vanishes silently: {}",
        out.stdout
    );
}

fn out_contains(finding: &serde_json::Value, needle: &str) -> bool {
    finding.to_string().contains(needle)
}
