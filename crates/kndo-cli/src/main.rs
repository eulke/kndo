//! kndo CLI — a frontend over the `kndo` distribution crate, nothing more (contracts §5).
//!
//! Pure presentation: argument parsing, exit codes, human rendering (RFC 0009). Which
//! languages exist is the distribution crate's knowledge (RFC 0001 §2) — this binary never
//! names one. If code here needs a graph fact, that is a core PR adding it to `RunResult`,
//! never a deeper import.

use std::io::IsTerminal;
use std::process::ExitCode;

use kndo::engine::{
    BaselineOp, BaselineResult, CheckRequest, ConfigOverrides, Finding, RunMode, Severity,
    SCHEMA_VERSION,
};

mod nav;
mod render;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!(
                "kndo {} (schema {})",
                env!("CARGO_PKG_VERSION"),
                SCHEMA_VERSION
            );
            ExitCode::SUCCESS
        }
        Some("check") => check(&args[1..]),
        Some("baseline") => baseline_cmd(&args[1..]),
        Some("doctor") => doctor_cmd(),
        Some("init") => init_cmd(&args[1..]),
        Some("find") => nav::find_cmd(&args[1..]),
        Some("describe") => nav::describe_cmd(&args[1..]),
        Some("uses") => nav::uses_cmd(&args[1..]),
        Some("used-by") => nav::used_by_cmd(&args[1..]),
        Some("trace") => nav::trace_cmd(&args[1..]),
        Some("query") => nav::query_cmd(),
        // Bare flags with no subcommand (`kndo --format json`) are an implicit `check`, same
        // as no arguments at all — `kndo` = `kndo check` (RFC 0006 §2).
        Some(s) if s.starts_with('-') => check(&args),
        None => check(&args),
        Some(other) => {
            eprintln!(
                "kndo: unknown command `{other}` (check, baseline, doctor, init, find, describe, uses, used-by, trace, query, --version)"
            );
            ExitCode::from(2)
        }
    }
}

const KNDO_TOML_TEMPLATE: &str = r#"# kndo.toml — everything here is optional; every setting already has the default shown.
# Written by `kndo init`. Full reference: RFC 0006 §7.

# [project]
# roots = ["src", "packages/*"]          # default: auto (git ls-files minus ignores)
# exclude = ["**/generated/**"]

# [analysis]
# skip = []                              # categories or category:subject, e.g. ["unused:enum-member"]
# min-confidence = "probable"            # report floor; "possible" only with --verbose

# [analysis.duplicate]
# min-tokens = 50

# [analysis.crap]
# threshold = 30

# [performance]
# threads = 0                            # 0 = physical cores; --threads flag wins

# [delta]                                # diff-mode gate budgets, see RFC 0006 §5
# max-health-drop = 0.0
# max-net-findings = 0

# [[rule]]                               # per-path overrides
# paths = ["examples/**"]
# skip = ["unused"]
"#;

const PRE_COMMIT_HOOK: &str = "#!/bin/sh\nexec kndo check --staged --fail-on warning\n";

/// `kndo init` (RFC 0006 §2): "write minimal kndo.toml, .gitignore entry, offer pre-commit
/// hook." Deliberately not an `Engine` method — contracts §5's `Engine` trait doesn't list
/// `init` alongside `check`/`baseline`/`doctor`, and this command does no analysis at all, just
/// project scaffolding, so there's nothing for the analysis facade to own.
///
/// "Offer" is read literally: writing directly into `.git/hooks/pre-commit` unprompted could
/// silently clobber an existing hook (or a hook manager's own file) — a hard-to-reverse,
/// surprising action for a tool to take on its own. Default behavior only *prints* the
/// recommended hook and how to install it; `--hook` opts into actually writing it, and even
/// then only when `.git/hooks/pre-commit` doesn't already exist.
fn init_cmd(args: &[String]) -> ExitCode {
    let install_hook = args.iter().any(|a| a == "--hook");
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("kndo: cannot determine working directory: {e}");
            return ExitCode::from(2);
        }
    };

    let toml_path = cwd.join("kndo.toml");
    if toml_path.is_file() {
        println!("kndo.toml: already exists, left untouched");
    } else {
        match std::fs::write(&toml_path, KNDO_TOML_TEMPLATE) {
            Ok(()) => println!("kndo.toml: written"),
            Err(e) => {
                eprintln!("kndo: failed to write kndo.toml: {e}");
                return ExitCode::from(2);
            }
        }
    }

    match ensure_gitignore_entry(&cwd) {
        Ok(GitignoreOutcome::AlreadyPresent) => println!(".gitignore: .kndo/ already present"),
        Ok(GitignoreOutcome::Appended) => println!(".gitignore: added .kndo/"),
        Err(e) => {
            eprintln!("kndo: failed to update .gitignore: {e}");
            return ExitCode::from(2);
        }
    }

    let hook_path = cwd.join(".git/hooks/pre-commit");
    if !install_hook {
        println!("pre-commit hook: not installed (recommended — install with `kndo init --hook`, or add manually):");
        println!("  {}", PRE_COMMIT_HOOK.lines().last().unwrap());
    } else if !cwd.join(".git").is_dir() {
        println!(
            "pre-commit hook: skipped — {} is not a git repository",
            cwd.display()
        );
    } else if hook_path.is_file() {
        eprintln!(
            "kndo: .git/hooks/pre-commit already exists — refusing to overwrite it; add this line yourself:"
        );
        eprintln!("  {}", PRE_COMMIT_HOOK.lines().last().unwrap());
        return ExitCode::from(2);
    } else {
        if let Some(parent) = hook_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("kndo: failed to create .git/hooks: {e}");
                return ExitCode::from(2);
            }
        }
        if let Err(e) = std::fs::write(&hook_path, PRE_COMMIT_HOOK) {
            eprintln!("kndo: failed to write .git/hooks/pre-commit: {e}");
            return ExitCode::from(2);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&hook_path) {
                let mut perms = meta.permissions();
                perms.set_mode(perms.mode() | 0o111);
                let _ = std::fs::set_permissions(&hook_path, perms);
            }
        }
        println!("pre-commit hook: installed at .git/hooks/pre-commit");
    }

    ExitCode::SUCCESS
}

enum GitignoreOutcome {
    AlreadyPresent,
    Appended,
}

/// Idempotent: only appends `.kndo/` when no line already matches it exactly, and creates the
/// file if the project has none yet.
fn ensure_gitignore_entry(root: &std::path::Path) -> std::io::Result<GitignoreOutcome> {
    let path = root.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == ".kndo/") {
        return Ok(GitignoreOutcome::AlreadyPresent);
    }
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(".kndo/\n");
    std::fs::write(&path, updated)?;
    Ok(GitignoreOutcome::Appended)
}

/// `kndo doctor` (RFC 0006 §2): plain-text only for now — output-schema.md doesn't specify a
/// JSON shape for this command yet, so `--format` isn't wired here (a deliberate scoping choice,
/// not an oversight; `check`/`explain`/navigation verbs are where the JSON contract matters).
fn doctor_cmd() -> ExitCode {
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("kndo: cannot determine working directory: {e}");
            return ExitCode::from(2);
        }
    };
    let engine = match kndo::open(&cwd, ConfigOverrides::default()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
    };
    let report = engine.doctor();

    println!("project root: {}", report.project_root);
    println!();
    println!("adapters:");
    if report.adapters.is_empty() {
        println!("  (none registered)");
    }
    for a in &report.adapters {
        println!("  {}  (grammar {})", a.id, a.grammar_version);
        println!("    files:     {}", a.file_globs.join(", "));
        println!("    manifests: {}", a.manifest_globs.join(", "));
    }
    println!();
    println!("plugins: none registered (plugin system is internal-only pre-1.0, RFC 0003 §6)");
    println!();
    println!(
        "cache: {}",
        if report.cache_enabled {
            "enabled"
        } else {
            "disabled (--no-cache)"
        }
    );
    if let Some(c) = &report.cache {
        println!("  writable:       {}", c.writable);
        println!(
            "  facts entries:  {} ({} bytes)",
            c.facts_entries, c.facts_bytes
        );
        println!(
            "  graph snapshot: {}",
            if c.graph_snapshot_present {
                format!("present ({} bytes)", c.graph_snapshot_bytes)
            } else {
                "absent".to_string()
            }
        );
    }
    println!();
    if report.baseline_present {
        println!("baseline: present ({} entries)", report.baseline_entries);
    } else {
        println!("baseline: absent (kndo baseline to create one)");
    }

    ExitCode::SUCCESS
}

/// `kndo baseline [--update]` (RFC 0006 §6, contracts §5's `Engine::baseline`): snapshot the
/// complete current finding set into `.kndo/baseline.json` (committed — a human reviews the
/// diff). Without `--update`, refuses to overwrite an existing baseline — the RFC's "growth
/// requires an explicit `kndo baseline --update` in a reviewed commit" reads as *every* baseline
/// write after the first needing that explicit flag, not just growth specifically, since a bare
/// re-run can't tell growth from shrinkage without diffing first; `--update` covers both cases
/// identically (a full snapshot replace), matching the RFC's "auto-dropped on `--update`"
/// language for fixed entries. All the actual file I/O lives behind `Engine::baseline` — this is
/// purely argument parsing and rendering the outcome, like every other command here.
fn baseline_cmd(args: &[String]) -> ExitCode {
    let op = if args.iter().any(|a| a == "--update") {
        BaselineOp::Update
    } else {
        BaselineOp::Create
    };

    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("kndo: cannot determine working directory: {e}");
            return ExitCode::from(2);
        }
    };
    let mut engine = match kndo::open(&cwd, ConfigOverrides::default()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
    };

    match engine.baseline(op) {
        BaselineResult::Written { acknowledged } => {
            println!(
                "kndo: baseline written — {acknowledged} findings acknowledged (.kndo/baseline.json)"
            );
            ExitCode::SUCCESS
        }
        BaselineResult::AlreadyExists => {
            eprintln!(
                "kndo: .kndo/baseline.json already exists — use `kndo baseline --update` to refresh it"
            );
            ExitCode::from(2)
        }
        BaselineResult::WriteFailed(e) => {
            eprintln!("kndo: failed to write .kndo/baseline.json: {e}");
            ExitCode::from(2)
        }
    }
}

struct Flags {
    format: Option<String>,
    color: Option<String>,
    quiet: bool,
    no_cache: bool,
    staged: bool,
    diff: Option<String>,
    fail_on: Option<String>,
}

fn parse_flags(args: &[String]) -> Flags {
    let mut flags = Flags {
        format: None,
        color: None,
        quiet: false,
        no_cache: false,
        staged: false,
        diff: None,
        fail_on: None,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--format" => flags.format = it.next().cloned(),
            "--color" => flags.color = it.next().cloned(),
            "--quiet" => flags.quiet = true,
            "--no-cache" => flags.no_cache = true,
            "--staged" => flags.staged = true,
            "--diff" => flags.diff = it.next().cloned(),
            "--fail-on" => flags.fail_on = it.next().cloned(),
            s if s.starts_with("--format=") => {
                flags.format = Some(s["--format=".len()..].to_string())
            }
            s if s.starts_with("--color=") => flags.color = Some(s["--color=".len()..].to_string()),
            s if s.starts_with("--diff=") => flags.diff = Some(s["--diff=".len()..].to_string()),
            s if s.starts_with("--fail-on=") => {
                flags.fail_on = Some(s["--fail-on=".len()..].to_string())
            }
            _ => {}
        }
    }
    flags
}

/// `--staged` and `--diff <ref>` select `RunMode` (RFC 0006 §2); mutually exclusive, checked
/// here rather than left for the engine since "which mode" is entirely a frontend argument-
/// parsing concern. **Known gap, already true before this flag existed and unchanged by it:**
/// neither mode actually scopes the report to the change's effects yet (RFC 0004 §6's
/// derived-effects diffing isn't implemented) — every mode still walks and reports the full
/// tree; only `run.mode`/`run.base_ref` and the `--fail-on` default (below) react to the
/// selected mode today.
fn resolve_mode(flags: &Flags) -> Result<RunMode, String> {
    match (flags.staged, &flags.diff) {
        (true, Some(_)) => Err("--staged and --diff are mutually exclusive".to_string()),
        (true, None) => Ok(RunMode::Staged),
        (false, Some(base)) => Ok(RunMode::Diff { base: base.clone() }),
        (false, None) => Ok(RunMode::Full),
    }
}

/// `--fail-on <severity>` (RFC 0006 §5): explicit flag wins; otherwise the default depends on
/// mode — `warning` in diff modes (a pre-commit gate should actually gate), `none` in full mode
/// (exploratory by default — a legacy repo's pre-existing findings shouldn't fail a plain
/// `kndo check`, RFC 0006 §6's day-one-adoption philosophy). `None` return means "never fail on
/// findings"; `Some(sev)` means "fail if any finding is at least as severe as `sev`".
fn resolve_fail_on(explicit: Option<&str>, mode: &RunMode) -> Result<Option<Severity>, String> {
    let raw = explicit.unwrap_or(match mode {
        RunMode::Full => "none",
        RunMode::Staged | RunMode::Diff { .. } => "warning",
    });
    match raw.to_ascii_lowercase().as_str() {
        "none" => Ok(None),
        "error" => Ok(Some(Severity::Error)),
        "warning" => Ok(Some(Severity::Warning)),
        "info" => Ok(Some(Severity::Info)),
        other => Err(format!(
            "unknown --fail-on `{other}` (none, info, warning, error)"
        )),
    }
}

/// Severity's declared enum order is worst-first for *display* sorting (engine.rs's own doc:
/// "declaration order doubles as sort/triage order"), which is the opposite direction from what
/// an "at least as severe as" threshold check wants — spelling out the rank explicitly here
/// avoids relying on readers (or future editors) inferring the right comparison direction from
/// derived `Ord`.
fn severity_rank(s: Severity) -> u8 {
    match s {
        Severity::Error => 3,
        Severity::Warning => 2,
        Severity::Info => 1,
    }
}

/// Exit-code decision from RFC 0006 §5's table, the findings half of it: delta budgets (the
/// other half, "or a delta budget exceeded") depend on health scoring, which doesn't exist yet
/// (M4) — not fabricated here, so today `--fail-on` is the whole gate.
fn exit_code_for_findings(findings: &[Finding], fail_on: Option<Severity>) -> ExitCode {
    let Some(threshold) = fail_on else {
        return ExitCode::SUCCESS;
    };
    if findings
        .iter()
        .any(|f| severity_rank(f.severity) >= severity_rank(threshold))
    {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// `--format` flag > `KNDO_FORMAT` env > TTY auto-detect (human on TTY, json when piped) —
/// RFC 0006 §2.
pub(crate) fn resolve_format(explicit: Option<&str>) -> String {
    if let Some(f) = explicit {
        return f.to_string();
    }
    if let Ok(env_format) = std::env::var("KNDO_FORMAT") {
        if !env_format.is_empty() {
            return env_format;
        }
    }
    if std::io::stdout().is_terminal() {
        "human".to_string()
    } else {
        "json".to_string()
    }
}

/// `NO_COLOR` always wins over `auto`; `--color always|never` overrides the TTY auto-detect
/// (RFC 0009 §4).
pub(crate) fn resolve_color(explicit: Option<&str>) -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    match explicit {
        Some("always") => true,
        Some("never") => false,
        _ => std::io::stdout().is_terminal(),
    }
}

fn check(args: &[String]) -> ExitCode {
    let flags = parse_flags(args);
    let format = resolve_format(flags.format.as_deref());
    let mode = match resolve_mode(&flags) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
    };
    let fail_on = match resolve_fail_on(flags.fail_on.as_deref(), &mode) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
    };

    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("kndo: cannot determine working directory: {e}");
            return ExitCode::from(2);
        }
    };
    let overrides = ConfigOverrides {
        use_cache: !flags.no_cache,
    };
    let mut engine = match kndo::open(&cwd, overrides) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
    };
    let result = engine.check(CheckRequest { mode });

    // Diagnostics degrade the run, they don't kill it (RFC 0001 §6): report on stderr and
    // continue — findings and diagnostics are not the same thing. stderr carries diagnostics
    // in every format; stdout stays the pure report (RFC 0009 §6), JSON included.
    for d in &result.diagnostics {
        let level = match d.level {
            kndo::adapter::DiagnosticLevel::Warn => "warning",
            kndo::adapter::DiagnosticLevel::Info => "info",
        };
        match &d.path {
            Some(p) => eprintln!("kndo: {level}: {}: {}", p.0, d.message),
            None => eprintln!("kndo: {level}: {}", d.message),
        }
    }

    match format.as_str() {
        "json" => println!("{}", result.to_json()),
        "human" => {
            let opts = render::RenderOptions {
                color: resolve_color(flags.color.as_deref()),
                quiet: flags.quiet,
            };
            print!("{}", render::render(&result, &opts));
        }
        "agent" => println!("{}", result.to_agent_format()),
        "sarif" => {
            eprintln!("kndo: --format sarif isn't implemented yet (lands with M4's health/CRAP work); use human, json, or agent");
            return ExitCode::from(2);
        }
        other => {
            eprintln!("kndo: unknown --format `{other}` (human, json, agent)");
            return ExitCode::from(2);
        }
    }

    exit_code_for_findings(&result.findings, fail_on)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("kndo-cli-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn gitignore_created_when_absent() {
        let dir = tmp_dir("gitignore-create");
        assert!(matches!(
            ensure_gitignore_entry(&dir).unwrap(),
            GitignoreOutcome::Appended
        ));
        let text = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(text, ".kndo/\n");
    }

    #[test]
    fn gitignore_entry_appended_to_existing_content() {
        let dir = tmp_dir("gitignore-append");
        std::fs::write(dir.join(".gitignore"), "target/").unwrap(); // no trailing newline
        assert!(matches!(
            ensure_gitignore_entry(&dir).unwrap(),
            GitignoreOutcome::Appended
        ));
        let text = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(text, "target/\n.kndo/\n");
    }

    #[test]
    fn gitignore_entry_is_idempotent() {
        let dir = tmp_dir("gitignore-idempotent");
        std::fs::write(dir.join(".gitignore"), "target/\n.kndo/\n").unwrap();
        assert!(matches!(
            ensure_gitignore_entry(&dir).unwrap(),
            GitignoreOutcome::AlreadyPresent
        ));
        let text = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(text, "target/\n.kndo/\n"); // unchanged, not duplicated
    }

    fn flags(staged: bool, diff: Option<&str>, fail_on: Option<&str>) -> Flags {
        Flags {
            format: None,
            color: None,
            quiet: false,
            no_cache: false,
            staged,
            diff: diff.map(str::to_string),
            fail_on: fail_on.map(str::to_string),
        }
    }

    #[test]
    fn parses_new_flags() {
        let args: Vec<String> = ["--staged", "--fail-on", "error"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let f = parse_flags(&args);
        assert!(f.staged);
        assert_eq!(f.fail_on.as_deref(), Some("error"));

        let args: Vec<String> = ["--diff=main", "--fail-on=none"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let f = parse_flags(&args);
        assert_eq!(f.diff.as_deref(), Some("main"));
        assert_eq!(f.fail_on.as_deref(), Some("none"));
    }

    #[test]
    fn staged_and_diff_are_mutually_exclusive() {
        assert!(resolve_mode(&flags(true, Some("main"), None)).is_err());
    }

    #[test]
    fn mode_resolves_from_flags() {
        assert_eq!(resolve_mode(&flags(false, None, None)), Ok(RunMode::Full));
        assert_eq!(resolve_mode(&flags(true, None, None)), Ok(RunMode::Staged));
        assert_eq!(
            resolve_mode(&flags(false, Some("main"), None)),
            Ok(RunMode::Diff {
                base: "main".to_string()
            })
        );
    }

    #[test]
    fn fail_on_defaults_differ_by_mode_but_an_explicit_flag_always_wins() {
        assert_eq!(resolve_fail_on(None, &RunMode::Full).unwrap(), None);
        assert_eq!(
            resolve_fail_on(None, &RunMode::Staged).unwrap(),
            Some(Severity::Warning)
        );
        assert_eq!(
            resolve_fail_on(
                None,
                &RunMode::Diff {
                    base: "main".to_string()
                }
            )
            .unwrap(),
            Some(Severity::Warning)
        );
        assert_eq!(
            resolve_fail_on(Some("error"), &RunMode::Full).unwrap(),
            Some(Severity::Error)
        );
        assert_eq!(
            resolve_fail_on(Some("none"), &RunMode::Staged).unwrap(),
            None
        );
        assert!(resolve_fail_on(Some("bogus"), &RunMode::Full).is_err());
    }

    fn finding(severity: Severity) -> Finding {
        Finding {
            id: "kndo-000000000000".to_string(),
            category: "unused".to_string(),
            group: "waste".to_string(),
            subject_kind: "symbol".to_string(),
            severity,
            confidence: kndo::vocab::Confidence::Certain,
            message: "example".to_string(),
            location: Default::default(),
            delta: None,
            delta_origin: None,
        }
    }

    #[test]
    fn fail_on_none_never_fails_regardless_of_findings() {
        let findings = vec![finding(Severity::Error)];
        assert_eq!(exit_code_for_findings(&findings, None), ExitCode::SUCCESS);
    }

    #[test]
    fn threshold_trips_on_at_least_as_severe_findings_only() {
        let findings = vec![finding(Severity::Info)];
        assert_eq!(
            exit_code_for_findings(&findings, Some(Severity::Warning)),
            ExitCode::SUCCESS
        );

        let findings = vec![finding(Severity::Warning)];
        assert_eq!(
            exit_code_for_findings(&findings, Some(Severity::Warning)),
            ExitCode::from(1)
        );

        let findings = vec![finding(Severity::Error)];
        assert_eq!(
            exit_code_for_findings(&findings, Some(Severity::Warning)),
            ExitCode::from(1)
        );
    }

    #[test]
    fn empty_findings_never_trip_any_threshold() {
        assert_eq!(
            exit_code_for_findings(&[], Some(Severity::Info)),
            ExitCode::SUCCESS
        );
    }
}
