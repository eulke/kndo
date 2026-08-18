//! kndo CLI — a frontend over `kndo_core::engine::Engine`, nothing more (contracts §5).
//!
//! Everything analysis-shaped lives behind the Engine; this binary owns argument parsing,
//! exit codes, and human rendering (RFC 0009). If code here needs a graph fact, that is a
//! core PR adding it to `RunResult`, never a core import beyond the facade.

use std::process::ExitCode;

use kndo_core::engine::{CheckRequest, ConfigOverrides, Engine, RunMode, SCHEMA_VERSION};

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
    let mut engine = match Engine::open(&cwd, ConfigOverrides::default()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
    };
    let result = engine.check(CheckRequest {
        mode: RunMode::Full,
    });

    if !result.diagnostics.is_empty() {
        for d in &result.diagnostics {
            eprintln!("kndo: {d}");
        }
        return ExitCode::from(2);
    }

    // RFC 0009 §2 quiet success — one line. (Real rendering lands with real findings.)
    if result.findings.is_empty() {
        println!(
            "kndo · clean · {} files discovered · 0 findings (M1 skeleton — analyses land next)",
            result.files_discovered
        );
        ExitCode::SUCCESS
    } else {
        println!("kndo · {} findings", result.findings.len());
        ExitCode::from(1)
    }
}
