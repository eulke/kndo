//! `cargo xtask` — development-time tasks. One generic tool; languages are table entries.
//!
//! # gen-stdlib
//!
//! `cargo xtask gen-stdlib <language>|--all` regenerates an adapter's `kndo-stdlib v1`
//! dataset from its authoritative source (RFC 0002 §6). The pipeline is language-agnostic:
//! run the source command → filter/sort/dedup → emit v1 format → **validate with the same
//! `StdlibIndex` loader that consumes it at build time** → write. Adding a language is one
//! `SOURCES` table entry, never a new script.
//!
//! Runs at kndo development time only — the analyzed machine's toolchains are never queried
//! at analysis time (determinism, RFC 0008 §4).

use std::process::{Command, ExitCode};

/// Everything language-specific about stdlib generation, as data.
struct StdlibSource {
    /// Adapter language id — also the CLI argument and the `language:` header value.
    language: &'static str,
    /// Where the dataset lives, relative to the workspace root.
    output: &'static str,
    /// Command producing one candidate name per line.
    list_command: &'static [&'static str],
    /// Command whose first output line identifies the source toolchain version.
    version_command: &'static [&'static str],
    /// Entries starting with any of these prefixes are dropped (e.g. Node's `node:`-only
    /// builtins are handled structurally by the adapter, not by the list).
    exclude_prefixes: &'static [&'static str],
}

const SOURCES: &[StdlibSource] = &[
    StdlibSource {
        language: "js-ts",
        output: "crates/kndo-adapter-js/src/stdlib.txt",
        list_command: &["node", "-p", "require('module').builtinModules.join('\\n')"],
        version_command: &["node", "--version"],
        exclude_prefixes: &["node:"],
    },
    // Future entries — one line of data each, no new tooling:
    //   go:    list `go list std`,            version `go version`
    //   java:  list `java --list-modules`,    version `java --version`
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("gen-stdlib") => gen_stdlib(args.get(1).map(String::as_str)),
        _ => {
            eprintln!("usage: cargo xtask gen-stdlib <language>|--all");
            eprintln!(
                "  languages: {}",
                SOURCES
                    .iter()
                    .map(|s| s.language)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            ExitCode::from(2)
        }
    }
}

fn gen_stdlib(which: Option<&str>) -> ExitCode {
    let selected: Vec<&StdlibSource> = match which {
        Some("--all") => SOURCES.iter().collect(),
        Some(lang) => match SOURCES.iter().find(|s| s.language == lang) {
            Some(s) => vec![s],
            None => {
                eprintln!(
                    "xtask: unknown language `{lang}` — known: {}",
                    SOURCES
                        .iter()
                        .map(|s| s.language)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                return ExitCode::from(2);
            }
        },
        None => {
            eprintln!("usage: cargo xtask gen-stdlib <language>|--all");
            return ExitCode::from(2);
        }
    };

    for source in selected {
        if let Err(msg) = generate(source) {
            eprintln!("xtask: gen-stdlib {} failed: {msg}", source.language);
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn generate(source: &StdlibSource) -> Result<(), String> {
    let raw = run(source.list_command)?;
    let version = run(source.version_command)?
        .lines()
        .next()
        .unwrap_or("unknown")
        .trim()
        .to_string();

    let mut entries: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| !source.exclude_prefixes.iter().any(|p| l.starts_with(p)))
        .collect();
    entries.sort_unstable();
    entries.dedup();

    let mut out = String::new();
    out.push_str("# kndo-stdlib v1\n");
    out.push_str(&format!("# language: {}\n", source.language));
    out.push_str(&format!("# source: {}\n", source.list_command.join(" ")));
    out.push_str(&format!("# source-version: {version}\n"));
    out.push_str(&format!(
        "# regenerate: cargo xtask gen-stdlib {}\n",
        source.language
    ));
    for e in &entries {
        out.push_str(e);
        out.push('\n');
    }

    // Validate with the exact loader that consumes this file at build time — the generator
    // can never emit something the product would reject.
    kndo_adapter_toolkit::stdlib::StdlibIndex::parse(&out)
        .map_err(|e| format!("generated data failed loader validation: {e:?}"))?;

    let path = workspace_root()?.join(source.output);
    std::fs::write(&path, &out).map_err(|e| format!("writing {}: {e}", path.display()))?;
    println!(
        "xtask: wrote {} ({} entries, {version})",
        source.output,
        entries.len()
    );
    Ok(())
}

fn run(cmd: &[&str]) -> Result<String, String> {
    let output = Command::new(cmd[0]).args(&cmd[1..]).output().map_err(|e| {
        format!(
            "`{}` not runnable ({e}) — is the toolchain installed?",
            cmd[0]
        )
    })?;
    if !output.status.success() {
        return Err(format!(
            "`{}` exited with {}: {}",
            cmd.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("non-UTF8 output: {e}"))
}

fn workspace_root() -> Result<std::path::PathBuf, String> {
    // xtask always runs via `cargo xtask` from within the workspace; CARGO_MANIFEST_DIR of
    // this crate is <root>/xtask.
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "cannot locate workspace root".into())
}
