//! `cargo xtask` — development-time tasks. One generic tool; languages are table entries.
//!
//! # gen-stdlib
//!
//! `cargo xtask gen-stdlib <language>|--all` regenerates an adapter's `kndo-stdlib v1`
//! dataset from its authoritative source. The pipeline is language-agnostic:
//! run the source command → filter/sort/dedup → emit v1 format → **validate with the same
//! `StdlibIndex` loader that consumes it at build time** → write. Adding a language is one
//! `SOURCES` table entry, never a new script.
//!
//! Runs at kndo development time only — the analyzed machine's toolchains are never queried
//! at analysis time (determinism).
//!
//! # gen-schema
//!
//! `cargo xtask gen-schema` regenerates `schemas/kndo-output.schema.json` from
//! `kndo_core::engine::Envelope` via `schemars` — the schema is generated from the Rust
//! types, never a second hand-written document.

use std::process::{Command, ExitCode};

use xtask::{doc_freshness, package};

mod bench;

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
        // boundary is a structural, compiler-enforced signal, not
        // something the stdlib-classification list needs to carry.
        exclude_prefixes: &["internal/"],
    },
    // Adding a language is one line of data, no new tooling — e.g. for java: list
    // `java --list-modules`, version `java --version`.
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("gen-stdlib") => gen_stdlib(args.get(1).map(String::as_str)),
        Some("gen-schema") => gen_schema(),
        Some("bench") => match bench::run(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("xtask: bench failed: {e}");
                ExitCode::FAILURE
            }
        },
        Some("componentize") => componentize(
            args.get(1).map(String::as_str),
            args.get(2).map(String::as_str),
        ),
        Some("package") => package_cmd(&args[1..]),
        Some("check-doc-freshness") => check_doc_freshness_cmd(&args[1..]),
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
            eprintln!("usage: cargo xtask bench [--sizes 1k,5k,50k] [--update-baseline] [--gate]");
            eprintln!("usage: cargo xtask componentize <core.wasm> <out.wasm>");
            eprintln!(
                "usage: cargo xtask package --target <triple> [--tag vX.Y.Z] [--bin <path>] \
                 [--out-dir <dir>]"
            );
            eprintln!(
                "  targets: {}",
                package::TARGETS
                    .iter()
                    .map(|t| t.triple)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            eprintln!(
                "usage: cargo xtask check-doc-freshness [--base <ref>]  (default: {})",
                doc_freshness::DEFAULT_BASE
            );
            ExitCode::from(2)
        }
    }
}

/// `cargo xtask check-doc-freshness --base <ref>` — fail when this diff touches a source path
/// [`doc_freshness::DOC_COVERAGE`] maps to a document, without touching that document.
fn check_doc_freshness_cmd(args: &[String]) -> ExitCode {
    match doc_freshness::from_args(args, workspace_root()) {
        Ok(()) => {
            println!("xtask: doc freshness clean");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("xtask: check-doc-freshness failed:\n{e}");
            ExitCode::FAILURE
        }
    }
}

/// `cargo xtask package --target <triple>` — build the release artifact for one platform.
///
/// `release.yml` calls this instead of carrying a `tar` line for Unix and a `Compress-Archive`
/// line for Windows: the artifact's name and layout are a contract four consumers depend on
/// (see [`xtask::package`]), and a contract with two producers is not one.
///
/// Prints the archive's path on stdout so the caller can capture it without re-deriving the
/// name it just asked for.
fn package_cmd(args: &[String]) -> ExitCode {
    match package::from_args(args, workspace_root()) {
        Ok(path) => {
            println!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("xtask: package failed: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Wraps a `wasm32-unknown-unknown` core module into a WASM component — the same
/// `wit_component::ComponentEncoder` call `crates/kndo/tests/external_adapter.rs` and
/// kndo-plugin-api's compliance suites already make in-process. Exposed as its own `xtask`
/// step so CI's shell-build smoke check doesn't need a separate `wasm-tools`
/// binary install for a one-line operation this workspace already depends on doing correctly.
fn componentize(core_path: Option<&str>, out_path: Option<&str>) -> ExitCode {
    let (Some(core_path), Some(out_path)) = (core_path, out_path) else {
        eprintln!("usage: cargo xtask componentize <core.wasm> <out.wasm>");
        return ExitCode::from(2);
    };
    match encode_component(core_path, out_path) {
        Ok(()) => {
            println!("xtask: wrote {out_path}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("xtask: componentize failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn encode_component(core_path: &str, out_path: &str) -> Result<(), String> {
    let core_wasm = std::fs::read(core_path).map_err(|e| format!("reading {core_path}: {e}"))?;
    let component = wit_component::ComponentEncoder::default()
        .module(&core_wasm)
        .and_then(|mut enc| enc.encode())
        .map_err(|e| format!("encoding {core_path}: {e:#}"))?;
    std::fs::write(out_path, &component).map_err(|e| format!("writing {out_path}: {e}"))
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
