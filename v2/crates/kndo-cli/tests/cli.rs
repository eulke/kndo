//! The CLI end-to-end through its library: real projects via the testkit, every
//! exit code, both output modes, and the baseline round trip.

use kndo_cli::run_args;
use kndo_testkit::TempProject;

fn project_with_findings() -> TempProject {
    let p = TempProject::new();
    p.file(
        "package.json",
        r#"{ "name": "demo", "main": "src/index.js" }"#,
    );
    p.file(
        "src/index.js",
        "export function api() { return used(); }\nimport { used } from \"./used.js\";\n",
    );
    p.file("src/used.js", "export function used() { return 1; }\n");
    p.file("src/orphan.js", "export function floats() {}\n");
    p
}

fn check(p: &TempProject, extra: &[&str]) -> kndo_cli::CliOutput {
    let root = p.root().to_string_lossy().into_owned();
    let mut args = vec!["kndo", "check", &root];
    args.extend_from_slice(extra);
    run_args(args)
}

#[test]
fn findings_fail_the_gate_and_render_as_text() {
    let p = project_with_findings();
    let out = check(&p, &[]);
    assert_eq!(out.code, 1, "unused findings are warnings: {}", out.stdout);
    assert!(out.stderr.is_empty());
    assert!(
        out.stdout.contains("warning unused src/orphan.js"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("1 finding\n"), "{}", out.stdout);
}

#[test]
fn fail_on_maps_to_the_gate() {
    let p = project_with_findings();
    // The same findings pass when the gate only fails on errors, or never.
    assert_eq!(check(&p, &["--fail-on", "error"]).code, 0);
    assert_eq!(check(&p, &["--fail-on", "never"]).code, 0);
    assert_eq!(check(&p, &["--fail-on", "info"]).code, 1);
}

#[test]
fn json_mode_emits_the_report_envelope() {
    let p = project_with_findings();
    let out = check(&p, &["--json", "--no-cache"]);
    let report: serde_json::Value = serde_json::from_str(&out.stdout).expect("stdout is JSON");
    assert_eq!(report["run"]["schema"], "kndo-v2/m5");
    assert!(report["findings"].as_array().is_some_and(|f| !f.is_empty()));
}

#[test]
fn clean_project_passes_with_a_measured_summary() {
    let p = TempProject::new();
    p.file(
        "package.json",
        r#"{ "name": "demo", "main": "src/index.js" }"#,
    );
    p.file("src/index.js", "export function api() { return 1; }\n");
    let out = check(&p, &[]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert!(out.stdout.contains("no findings"), "{}", out.stdout);
}

#[test]
fn baseline_accepts_findings_and_check_diffs_against_it() {
    let p = project_with_findings();
    let root = p.root().to_string_lossy().into_owned();

    let write = run_args(["kndo", "baseline", &root]);
    assert_eq!(write.code, 0, "{}", write.stderr);
    assert!(
        write.stdout.contains("baseline written"),
        "{}",
        write.stdout
    );
    assert!(p.root().join(".kndo/baseline.json").is_file());

    // Everything is baselined now: the gate counts only NEW findings.
    let after = check(&p, &[]);
    assert_eq!(after.code, 0, "{}", after.stdout);
    assert!(after.stdout.contains("baselined"), "{}", after.stdout);
}

#[test]
fn a_missing_root_is_a_refusal_on_stderr() {
    let out = run_args(["kndo", "check", "/definitely/not/a/real/root"]);
    assert_eq!(out.code, 2);
    assert!(out.stdout.is_empty());
    assert!(out.stderr.contains("refused"), "{}", out.stderr);
}

#[test]
fn help_is_a_conversation_not_an_error() {
    let out = run_args(["kndo", "--help"]);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("kndo"), "{}", out.stdout);
    assert!(out.stderr.is_empty());

    let bad = run_args(["kndo", "check", "--not-a-flag"]);
    assert_eq!(bad.code, 2);
    assert!(bad.stdout.is_empty());
}
