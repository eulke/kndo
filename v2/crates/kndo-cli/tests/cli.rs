//! The CLI end-to-end through its library: real projects via the testkit, every
//! exit code, every format, and the baseline round trip. Each test states its
//! [`Host`] — the terminal and environment facts are inputs here, not ambience.

use kndo_cli::{Host, run_args};
use kndo_testkit::TempProject;

fn piped() -> Host {
    Host {
        tty: false,
        format_env: None,
    }
}

fn terminal() -> Host {
    Host {
        tty: true,
        format_env: None,
    }
}

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

fn check(p: &TempProject, extra: &[&str], host: Host) -> kndo_cli::CliOutput {
    let root = p.root().to_string_lossy().into_owned();
    let mut args = vec!["kndo", "check", &root];
    args.extend_from_slice(extra);
    run_args(args, host)
}

#[test]
fn findings_fail_the_gate_and_render_as_text_on_a_terminal() {
    let p = project_with_findings();
    let out = check(&p, &[], terminal());
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
    assert_eq!(check(&p, &["--fail-on", "error"], piped()).code, 0);
    assert_eq!(check(&p, &["--fail-on", "never"], piped()).code, 0);
    assert_eq!(check(&p, &["--fail-on", "info"], piped()).code, 1);
}

#[test]
fn piped_output_is_the_report_envelope() {
    let p = project_with_findings();
    let out = check(&p, &["--no-cache"], piped());
    let report: serde_json::Value = serde_json::from_str(&out.stdout).expect("stdout is JSON");
    assert_eq!(report["run"]["schema"], "kndo-v2/m6");
    assert!(report["findings"].as_array().is_some_and(|f| !f.is_empty()));
}

#[test]
fn the_format_flag_beats_terminal_and_environment() {
    let p = project_with_findings();
    // A terminal would render text; the flag says otherwise.
    let json = check(&p, &["--format", "json"], terminal());
    assert!(json.stdout.starts_with('{'), "{}", json.stdout);
    // The environment would say sarif; the flag still wins.
    let host = Host {
        tty: true,
        format_env: Some("sarif".to_string()),
    };
    let human = check(&p, &["--format", "human"], host);
    assert!(human.stdout.contains("1 finding\n"), "{}", human.stdout);
}

#[test]
fn agent_format_carries_the_finding_id_as_the_handle() {
    let p = project_with_findings();
    let out = check(&p, &["--format", "agent"], piped());
    assert!(
        out.stdout.starts_with("kndo agent format 1 (kndo-v2/m6)\n"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("findings:\n[kndo-"), "{}", out.stdout);
    assert!(
        out.stdout
            .contains("warning unused · file src/orphan.js · certain"),
        "{}",
        out.stdout
    );
}

#[test]
fn sarif_format_emits_the_interchange_envelope() {
    let p = project_with_findings();
    let out = check(&p, &["--format", "sarif"], piped());
    let sarif: serde_json::Value = serde_json::from_str(&out.stdout).expect("stdout is JSON");
    assert_eq!(sarif["version"], "2.1.0");
    assert_eq!(sarif["runs"][0]["tool"]["driver"]["name"], "kndo");
    assert_eq!(sarif["runs"][0]["results"][0]["ruleId"], "unused");
}

#[test]
fn the_environment_picks_the_format_and_garbage_warns_not_refuses() {
    let p = project_with_findings();
    let out = check(
        &p,
        &[],
        Host {
            tty: true,
            format_env: Some("agent".to_string()),
        },
    );
    assert!(
        out.stdout.starts_with("kndo agent format"),
        "{}",
        out.stdout
    );

    let garbled = check(
        &p,
        &[],
        Host {
            tty: true,
            format_env: Some("yaml".to_string()),
        },
    );
    assert!(
        garbled.stderr.contains("unknown KNDO_FORMAT `yaml`"),
        "{}",
        garbled.stderr
    );
    assert!(
        garbled.stdout.contains("1 finding\n"),
        "the terminal default still renders: {}",
        garbled.stdout
    );
}

#[test]
fn clean_project_passes_with_a_measured_summary() {
    let p = TempProject::new();
    p.file(
        "package.json",
        r#"{ "name": "demo", "main": "src/index.js" }"#,
    );
    p.file("src/index.js", "export function api() { return 1; }\n");
    let out = check(&p, &[], terminal());
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert!(out.stdout.contains("no findings"), "{}", out.stdout);
}

#[test]
fn baseline_accepts_findings_and_check_diffs_against_it() {
    let p = project_with_findings();
    let root = p.root().to_string_lossy().into_owned();

    let write = run_args(["kndo", "baseline", &root], piped());
    assert_eq!(write.code, 0, "{}", write.stderr);
    assert!(
        write.stdout.contains("baseline written"),
        "{}",
        write.stdout
    );
    assert!(p.root().join(".kndo/baseline.json").is_file());

    // Everything is baselined now: the gate counts only NEW findings.
    let after = check(&p, &[], terminal());
    assert_eq!(after.code, 0, "{}", after.stdout);
    assert!(after.stdout.contains("baselined"), "{}", after.stdout);
}

#[test]
fn a_missing_root_is_a_refusal_on_stderr() {
    let out = run_args(["kndo", "check", "/definitely/not/a/real/root"], piped());
    assert_eq!(out.code, 2);
    assert!(out.stdout.is_empty());
    assert!(out.stderr.contains("refused"), "{}", out.stderr);
}

#[test]
fn help_is_a_conversation_not_an_error() {
    let out = run_args(["kndo", "--help"], piped());
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("kndo"), "{}", out.stdout);
    assert!(out.stderr.is_empty());

    let bad = run_args(["kndo", "check", "--not-a-flag"], piped());
    assert_eq!(bad.code, 2);
    assert!(bad.stdout.is_empty());
}
