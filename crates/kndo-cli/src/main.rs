//! kndo CLI — a frontend over the `kndo` distribution crate, nothing more (contracts §5).
//!
//! Pure presentation: argument parsing, exit codes, human rendering (RFC 0009). Which
//! languages exist is the distribution crate's knowledge (RFC 0001 §2) — this binary never
//! names one. If code here needs a graph fact, that is a core PR adding it to `RunResult`,
//! never a deeper import.

use std::io::IsTerminal;
use std::process::ExitCode;

use kndo::engine::{CheckRequest, ConfigOverrides, Finding, RunMode, Severity, SCHEMA_VERSION};

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
        // Bare flags with no subcommand (`kndo --format json`) are an implicit `check`, same
        // as no arguments at all — `kndo` = `kndo check` (RFC 0006 §2).
        Some(s) if s.starts_with('-') => check(&args),
        None => check(&args),
        Some(other) => {
            eprintln!("kndo: unknown command `{other}` (M1 skeleton: check, --version)");
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
fn resolve_format(explicit: Option<&str>) -> String {
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
fn resolve_color(explicit: Option<&str>) -> bool {
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
