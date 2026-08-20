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
//!
//! # gen-schema
//!
//! `cargo xtask gen-schema` regenerates `schemas/kndo-output.schema.json` from
//! `kndo_core::engine::Envelope` via `schemars` (contracts/output-schema.md's normative
//! promise: "generated from the Rust types," never a second hand-written document).

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
    StdlibSource {
        language: "go",
        output: "crates/kndo-adapter-go/src/stdlib.txt",
        list_command: &["go", "list", "std"],
        version_command: &["go", "version"],
        // `internal/...` stdlib packages (~a quarter of `go list std`'s output) are real
        // entries but uncompilable outside the standard library itself — Go's `internal/`
        // boundary (docs/adapters/go.md §0) is a structural, compiler-enforced signal, not
        // something the stdlib-classification list needs to carry.
        exclude_prefixes: &["internal/"],
    },
    // Future entries — one line of data each, no new tooling:
    //   java:  list `java --list-modules`,    version `java --version`
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("gen-stdlib") => gen_stdlib(args.get(1).map(String::as_str)),
        Some("gen-schema") => gen_schema(),
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
            eprintln!("usage: cargo xtask gen-schema");
            ExitCode::from(2)
        }
    }
}

fn gen_schema() -> ExitCode {
    let root = match workspace_root() {
        Ok(root) => root,
        Err(e) => {
            eprintln!("xtask: gen-schema failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    type SchemaTarget = (&'static str, fn() -> schemars::Schema);
    let targets: [SchemaTarget; 2] = [
        (
            "schemas/kndo-output.schema.json",
            kndo_core::engine::json_schema,
        ),
        (
            "schemas/kndo-query-output.schema.json",
            kndo_core::query_envelope::json_schema,
        ),
    ];
    for (rel_path, schema_fn) in targets {
        if write_schema(&root, rel_path, schema_fn()) == ExitCode::FAILURE {
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn write_schema(root: &std::path::Path, rel_path: &str, schema: schemars::Schema) -> ExitCode {
    let text = match serde_json::to_string_pretty(&schema) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("xtask: gen-schema failed to serialize {rel_path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let path = root.join(rel_path);
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("xtask: gen-schema failed creating {}: {e}", dir.display());
            return ExitCode::FAILURE;
        }
    }
    if let Err(e) = std::fs::write(&path, format!("{text}\n")) {
        eprintln!("xtask: gen-schema failed writing {}: {e}", path.display());
        return ExitCode::FAILURE;
    }
    println!("xtask: wrote {}", path.display());
    ExitCode::SUCCESS
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
