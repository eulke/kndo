//! The CLI end-to-end through its library: real projects via the testkit, every
//! exit code, every format, and the baseline round trip. Each test states its
//! [`Host`] — the terminal and environment facts are inputs here, not ambience.

use kndo_cli::{Host, run_args};
use kndo_testkit::{TempProject, js_demo_project as project_with_findings};

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
    assert!(
        out.stdout.contains("\nhealth ") && out.stdout.contains("· implicated 1 of "),
        "{}",
        out.stdout
    );
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
        out.stdout.starts_with("kndo agent format 2 (kndo-v2/m6)\n"),
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
    // …but health still measures the tree: acknowledged debt is still debt.
    assert!(
        after.stdout.contains("· implicated 1 of "),
        "the baseline must not launder health: {}",
        after.stdout
    );
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

fn sh_git(root: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn committed_project() -> TempProject {
    let p = project_with_findings();
    sh_git(p.root(), &["init", "-q"]);
    sh_git(p.root(), &["add", "-A"]);
    sh_git(p.root(), &["commit", "-qm", "base"]);
    p
}

#[test]
fn staged_mode_reports_what_the_change_moves() {
    let p = committed_project();
    // The staged change fixes the orphan (imports it) and introduces new dead code.
    p.file(
        "src/index.js",
        "export function api() { return used() + floats(); }\n\
         import { used } from \"./used.js\";\nimport { floats } from \"./orphan.js\";\n",
    );
    p.file(
        "src/leftover.js",
        "export function leftover() { return 9; }\n",
    );
    sh_git(p.root(), &["add", "-A"]);

    let out = check(&p, &["--staged"], terminal());
    assert_eq!(
        out.code, 1,
        "a new warning gates: {}{}",
        out.stdout, out.stderr
    );
    assert!(
        out.stdout.contains("staged: 1 new · 1 fixed · 0 carried"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("fixed by this change:"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("warning unused src/orphan.js"),
        "the fixed listing names the healed finding: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains(" → ") && out.stdout.contains("health "),
        "the health arrow shows which way the change moves it: {}",
        out.stdout
    );

    // The worktree still has the OLD index.js on disk? No — TempProject writes the
    // worktree; --staged must judge the INDEX, which equals the worktree here.
    // The unstaged case is the diff test's subject.
    let envelope = check(&p, &["--staged", "--format", "json"], piped());
    let report: serde_json::Value = serde_json::from_str(&envelope.stdout).expect("stdout is JSON");
    assert_eq!(report["run"]["mode"], "staged");
    assert_eq!(report["fixed"].as_array().map(|f| f.len()), Some(1));
    assert!(report["base_health"].is_object(), "{}", envelope.stdout);
}

#[test]
fn diff_mode_compares_the_worktree_against_a_ref() {
    let p = committed_project();
    // Unstaged worktree change: one more dead export.
    p.file(
        "src/used.js",
        "export function used() { return 1; }\nexport function fresh() { return 2; }\n",
    );
    let out = check(&p, &["--diff", "HEAD"], piped());
    let report: serde_json::Value = serde_json::from_str(&out.stdout).expect("stdout is JSON");
    assert_eq!(report["run"]["mode"], "diff");
    let new: Vec<String> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            f["subject"]["selector"]["Free"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })
        .collect();
    assert_eq!(new, ["fresh"], "{}", out.stdout);
    // The orphan exists in BOTH trees: carried, not new — and not gated.
    assert_eq!(report["baselined"], 1, "{}", out.stdout);
}

#[test]
fn a_diff_outside_git_is_a_plain_failure_not_a_panic() {
    let p = project_with_findings();
    let out = check(&p, &["--staged"], piped());
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("kndo: git:"), "{}", out.stderr);
}

#[test]
fn only_and_skip_narrow_judgment_not_display() {
    let p = project_with_findings();
    let out = check(&p, &["--only", "unused"], piped());
    let report: serde_json::Value = serde_json::from_str(&out.stdout).expect("stdout is JSON");
    assert_eq!(
        report["run"]["selection"],
        serde_json::json!({"only": ["unused"]})
    );
    assert!(
        report["findings"].as_array().is_some_and(|f| !f.is_empty()),
        "{}",
        out.stdout
    );

    let skipped = check(&p, &["--skip", "unused"], piped());
    let report: serde_json::Value = serde_json::from_str(&skipped.stdout).expect("stdout is JSON");
    assert_eq!(
        report["run"]["selection"],
        serde_json::json!({"skip": ["unused"]})
    );
    assert_eq!(
        report["findings"].as_array().map(|f| f.len()),
        Some(0),
        "{}",
        skipped.stdout
    );
    // Health follows judgment: with `unused` un-judged there is no health, and
    // the skipped category leaves no abstention behind either.
    assert!(report.get("health").is_none(), "{}", skipped.stdout);
    assert_eq!(skipped.code, 0, "nothing judged at the gate's floor");
}

#[test]
fn an_unknown_category_is_a_refused_invocation() {
    let p = project_with_findings();
    let out = check(&p, &["--only", "unusedd"], piped());
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("unknown category `unusedd`") && out.stderr.contains("unused"),
        "{}",
        out.stderr
    );
}

#[test]
fn the_health_verb_is_the_measurement_alone() {
    let p = project_with_findings();
    let root = p.root().to_string_lossy().into_owned();
    let tty = run_args(["kndo", "health", &root], terminal());
    assert_eq!(tty.code, 0, "health is measurement, not a gate");
    assert!(tty.stdout.starts_with("health "), "{}", tty.stdout);
    assert!(tty.stdout.contains("implicated 1 of"), "{}", tty.stdout);

    let piped_out = run_args(["kndo", "health", &root], piped());
    let health: serde_json::Value =
        serde_json::from_str(&piped_out.stdout).expect("stdout is JSON");
    assert_eq!(health["implicated"], 1);
    assert!(health["subjects"].as_u64().is_some());
}

#[test]
fn kndo_toml_sets_defaults_and_every_flag_beats_it() {
    let p = project_with_findings();
    p.file(
        "kndo.toml",
        "[check]\nfail-on = \"never\"\nformat = \"agent\"\n",
    );

    // The file's defaults apply: agent format on a terminal, gate never fails.
    let out = check(&p, &[], terminal());
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert!(
        out.stdout.starts_with("kndo agent format"),
        "{}",
        out.stdout
    );

    // Flags beat the file…
    let human = check(
        &p,
        &["--format", "human", "--fail-on", "warning"],
        terminal(),
    );
    assert_eq!(human.code, 1);
    assert!(human.stdout.contains("1 finding\n"), "{}", human.stdout);

    // …and KNDO_FORMAT beats the file too, but not the flag.
    let env = check(
        &p,
        &[],
        Host {
            tty: true,
            format_env: Some("json".to_string()),
        },
    );
    assert!(env.stdout.starts_with('{'), "{}", env.stdout);
}

#[test]
fn a_kndo_toml_typo_refuses_the_run() {
    let p = project_with_findings();
    p.file("kndo.toml", "[check]\nfail-onn = \"never\"\n");
    let out = check(&p, &[], piped());
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("kndo.toml"), "{}", out.stderr);
    assert!(out.stderr.contains("fail-onn"), "{}", out.stderr);
}

#[test]
fn init_writes_the_template_once_and_the_hook_gates_staged() {
    let p = project_with_findings();
    sh_git(p.root(), &["init", "-q"]);
    let root = p.root().to_string_lossy().into_owned();

    let out = run_args(["kndo", "init", &root, "--hook"], terminal());
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert!(p.root().join("kndo.toml").is_file());
    let hook = p.root().join(".git/hooks/pre-commit");
    assert!(hook.is_file());
    let script = std::fs::read_to_string(&hook).expect("hook readable");
    assert!(script.contains("kndo check --staged"), "{script}");

    // The template parses as an empty config: a fresh init changes nothing.
    let after = check(&p, &[], terminal());
    assert_eq!(after.code, 1, "{}", after.stdout);

    // A second init refuses — the file is someone's work now.
    let again = run_args(["kndo", "init", &root], terminal());
    assert_eq!(again.code, 2);
    assert!(again.stderr.contains("already exists"), "{}", again.stderr);
}

#[test]
fn doctor_tells_the_truth_about_what_kndo_sees() {
    let p = project_with_findings();
    p.file("kndo.toml", "[check]\nfail-on = \"never\"\n");
    let root = p.root().to_string_lossy().into_owned();
    let out = run_args(["kndo", "doctor", &root], terminal());
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert!(out.stdout.contains("- kndo:js-ts v"), "{}", out.stdout);
    assert!(out.stdout.contains("- kndo:python v"), "{}", out.stdout);
    assert!(
        out.stdout.contains("config: kndo.toml · fail-on never"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("cache: none yet"), "{}", out.stdout);
    assert!(out.stdout.contains("baseline: none"), "{}", out.stdout);

    // A broken config is doctor's diagnosis, never its crash.
    p.file("kndo.toml", "[check]\nfail-onn = \"never\"\n");
    let broken = run_args(["kndo", "doctor", &root], terminal());
    assert_eq!(broken.code, 0);
    assert!(
        broken.stdout.contains("config: BROKEN"),
        "{}",
        broken.stdout
    );
}

#[test]
fn the_query_verbs_speak_the_contract_end_to_end() {
    let p = project_with_findings();
    let root = p.root().to_string_lossy().into_owned();

    // used-by on the orphan's symbol: nothing keeps it, and that IS the answer
    // (exit 0 — an empty used-by is legitimate, not a failure).
    let dead = run_args(
        ["kndo", "used-by", "src/orphan.js#floats", "--root", &root],
        piped(),
    );
    assert_eq!(dead.code, 0, "{}{}", dead.stdout, dead.stderr);
    let response: serde_json::Value = serde_json::from_str(&dead.stdout).expect("json");
    assert_eq!(response["schema"], "kndo-query/1");
    assert_eq!(
        response["results"][0]["kept_by"]
            .as_array()
            .map(|k| k.len()),
        Some(0),
        "{}",
        dead.stdout
    );

    // used-by on the live symbol lists its keeper with a site.
    let live = run_args(
        ["kndo", "used-by", "src/used.js#used", "--root", &root],
        terminal(),
    );
    assert!(live.stdout.contains("- "), "{}", live.stdout);
    assert!(
        live.stdout
            .starts_with("kndo agent format 2 (kndo-query/1)\n"),
        "the terminal default is the agent text: {}",
        live.stdout
    );

    // A bad selector is its own not-found (exit 1), never its sibling's failure.
    let mixed = run_args(
        [
            "kndo",
            "describe",
            "src/used.js#used",
            "src/nope.js",
            "--root",
            &root,
        ],
        piped(),
    );
    assert_eq!(mixed.code, 1, "{}", mixed.stdout);
    let response: serde_json::Value = serde_json::from_str(&mixed.stdout).expect("json");
    assert_eq!(response["results"][0]["status"], "ok");
    assert_eq!(response["results"][1]["status"], "not-found");

    // find ranks and filters.
    let found = run_args(
        [
            "kndo", "find", "used", "--kind", "function", "--root", &root,
        ],
        piped(),
    );
    let response: serde_json::Value = serde_json::from_str(&found.stdout).expect("json");
    let selectors: Vec<&str> = response["results"][0]["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["selector"].as_str().unwrap())
        .collect();
    assert_eq!(
        selectors.first(),
        Some(&"src/used.js#used"),
        "exact beats substring: {}",
        found.stdout
    );
}

#[test]
fn trace_impact_and_explain_close_the_loop_end_to_end() {
    let p = project_with_findings();
    let root = p.root().to_string_lossy().into_owned();

    // trace names the rooted file, the edge into each hop, and the in-file
    // keeper — the whole liveness proof in one answer.
    let live = run_args(
        ["kndo", "trace", "src/used.js#used", "--root", &root],
        piped(),
    );
    assert_eq!(live.code, 0, "{}{}", live.stdout, live.stderr);
    let response: serde_json::Value = serde_json::from_str(&live.stdout).expect("json");
    let path = &response["results"][0]["path"];
    assert_eq!(path["roots"], "production", "{}", live.stdout);
    assert_eq!(path["root"]["selector"], "src/index.js");
    assert_eq!(path["hops"][0]["node"]["selector"], "src/used.js");
    assert_eq!(path["hops"][0]["via"], "import");
    assert!(path["keeper"]["kind"].is_string(), "{}", live.stdout);

    // trace of the orphan: ok-status truth with a null path — and exit 1, so
    // `kndo trace x && rm x` cannot delete something reachable.
    let orphan = run_args(["kndo", "trace", "src/orphan.js", "--root", &root], piped());
    assert_eq!(orphan.code, 1, "{}", orphan.stdout);
    let response: serde_json::Value = serde_json::from_str(&orphan.stdout).expect("json");
    assert_eq!(response["results"][0]["status"], "ok");
    assert!(
        response["results"][0]["path"].is_null(),
        "{}",
        orphan.stdout
    );

    // impact --if-deleted: the reverse closure plus the simulated removal.
    let impact = run_args(
        [
            "kndo",
            "impact",
            "src/used.js",
            "--if-deleted",
            "--root",
            &root,
        ],
        piped(),
    );
    assert_eq!(impact.code, 0, "{}{}", impact.stdout, impact.stderr);
    let response: serde_json::Value = serde_json::from_str(&impact.stdout).expect("json");
    let answer = &response["results"][0];
    assert_eq!(answer["affected"][0]["node"]["selector"], "src/index.js");
    assert_eq!(answer["affected"][0]["depth"], 1);
    assert_eq!(answer["by_color"]["production"], 1);
    assert_eq!(answer["affected_roots"][0], "production");
    // index.js is itself a root: deleting used.js orphans nothing upstream.
    assert_eq!(
        answer["if_deleted"]["newly_unreachable"]
            .as_array()
            .map(|n| n.len()),
        Some(0),
        "{}",
        impact.stdout
    );

    // explain closes finding-id -> subject -> why: the id from check answers
    // with the finding brief and the full description of its subject.
    let report = check(&p, &["--format", "json"], piped());
    let report: serde_json::Value = serde_json::from_str(&report.stdout).expect("json");
    let id = report["findings"][0]["id"].as_str().expect("a finding id");
    let explained = run_args(["kndo", "explain", id, "--root", &root], piped());
    assert_eq!(
        explained.code, 0,
        "{}{}",
        explained.stdout, explained.stderr
    );
    let response: serde_json::Value = serde_json::from_str(&explained.stdout).expect("json");
    let answer = &response["results"][0];
    assert_eq!(answer["finding"]["id"], id);
    assert_eq!(answer["finding"]["category"], "unused");
    assert_eq!(answer["subject"]["node"]["selector"], "src/orphan.js");

    // An id from nowhere is not-found, not a crash.
    let unknown = run_args(
        ["kndo", "explain", "kndo-000000000000", "--root", &root],
        piped(),
    );
    assert_eq!(unknown.code, 1, "{}", unknown.stdout);
}
