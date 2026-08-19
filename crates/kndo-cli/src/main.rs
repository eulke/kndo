//! kndo CLI — a frontend over the `kndo` distribution crate, nothing more (contracts §5).
//!
//! Pure presentation: argument parsing, exit codes, human rendering (RFC 0009). Which
//! languages exist is the distribution crate's knowledge (RFC 0001 §2) — this binary never
//! names one. If code here needs a graph fact, that is a core PR adding it to `RunResult`,
//! never a deeper import.

use std::io::IsTerminal;
use std::process::ExitCode;

use kndo::engine::{CheckRequest, ConfigOverrides, RunMode, SCHEMA_VERSION};

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
}

fn parse_flags(args: &[String]) -> Flags {
    let mut flags = Flags {
        format: None,
        color: None,
        quiet: false,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--format" => flags.format = it.next().cloned(),
            "--color" => flags.color = it.next().cloned(),
            "--quiet" => flags.quiet = true,
            s if s.starts_with("--format=") => {
                flags.format = Some(s["--format=".len()..].to_string())
            }
            s if s.starts_with("--color=") => flags.color = Some(s["--color=".len()..].to_string()),
            _ => {}
        }
    }
    flags
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
    let result = engine.check(CheckRequest {
        mode: RunMode::Full,
    });

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

    if result.findings.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
