//! kndo CLI — a frontend over the `kndo` distribution crate, nothing more (contracts §5).
//!
//! Pure presentation: argument parsing, exit codes, human rendering (RFC 0009). Which
//! languages exist is the distribution crate's knowledge (RFC 0001 §2) — this binary never
//! names one. If code here needs a graph fact, that is a core PR adding it to `RunResult`,
//! never a deeper import.

use std::process::ExitCode;

use kndo::engine::{CheckRequest, ConfigOverrides, RunMode, SCHEMA_VERSION};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("check") => check(),
        Some("--version" | "-V") => {
            println!(
                "kndo {} (schema {})",
                env!("CARGO_PKG_VERSION"),
                SCHEMA_VERSION
            );
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("kndo: unknown command `{other}` (M1 skeleton: check, --version)");
            ExitCode::from(2)
        }
    }
}

fn check() -> ExitCode {
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
    // continue — findings and diagnostics are not the same thing.
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

    // RFC 0009 §2 quiet success — one line. (Real rendering lands with real findings.)
    if result.findings.is_empty() {
        println!(
            "kndo · clean · {} files ({} claimed, {} symbols, {} deps, {} edges) · 0 findings (M1 skeleton — analyses land next)",
            result.files_discovered, result.files_claimed, result.symbols, result.dependencies, result.edges
        );
        ExitCode::SUCCESS
    } else {
        println!("kndo · {} findings", result.findings.len());
        ExitCode::from(1)
    }
}
