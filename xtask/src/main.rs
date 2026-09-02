//! v2's task runner. `gen-ci` renders the workflow from kndo-gates' registry;
//! `bench` measures the release binary against the recorded baseline;
//! `package` builds and archives the release binary with a checksum; `verify-artifact`
//! checksum-verifies, extracts and runs it — the install half of the release loop,
//! written in Rust so all three CI platforms run the identical check instead of three
//! shell dialects.

mod bench;

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

type Result<T> = std::result::Result<T, String>;

const BIN: &str = if cfg!(windows) { "kndo.exe" } else { "kndo" };

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("gen-ci") => gen_ci(),
        Some("gen-fingerprint") => gen_fingerprint(),
        Some("gen-schema") => gen_schema(),
        Some("package") => package(&args[1..]),
        Some("verify-artifact") => verify_artifact(&args[1..]),
        Some("corpus") => corpus(&args[1..]),
        Some("pin-abi") => pin_abi(),
        Some("bench") => bench::run(&args[1..]),
        _ => {
            eprintln!(
                "usage: cargo xtask <gen-ci | gen-fingerprint | gen-schema | package --tag T --out-dir D | verify-artifact --dir D | corpus --corpus-dir D [--out-dir D] | pin-abi | bench [--sizes 1k,5k] [--update-baseline] [--gate]>"
            );
            exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("xtask: {e}");
        exit(1);
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn gen_ci() -> Result<()> {
    let path = kndo_gates::workflow_path();
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(&path, kndo_gates::render_ci()).map_err(|e| e.to_string())?;
    println!("wrote {}", kndo_gates::WORKFLOW_REPO_PATH);
    fs::write(
        kndo_gates::release_workflow_path(),
        kndo_gates::render_release(),
    )
    .map_err(|e| e.to_string())?;
    println!("wrote {}", kndo_gates::RELEASE_WORKFLOW_REPO_PATH);
    Ok(())
}

/// Rebuilds the reference guests and rewrites the PINNED components under
/// `abi/compat/` — the deliberate act the `abi_compat_matrix` gate demands after
/// a WIT change: the rebuilt binaries landing in the same commit are the explicit,
/// reviewable record of a compatibility break, which a silent breakage never is.
fn pin_abi() -> Result<()> {
    let guests = workspace_root().join("abi/guests");
    let status = Command::new("cargo")
        .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .current_dir(&guests)
        .status()
        .map_err(|e| format!("invoking cargo for the guest build: {e}"))?;
    if !status.success() {
        return Err("reference guest build failed".into());
    }
    let out_dir = workspace_root().join("abi/compat");
    fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    for name in [
        "acme_framework",
        "kmini_adapter",
        "probe_plugin",
        "records_ingester",
    ] {
        let module = guests.join(format!("target/wasm32-unknown-unknown/release/{name}.wasm"));
        let bytes = fs::read(&module).map_err(|e| format!("{}: {e}", module.display()))?;
        let component = wit_component::ComponentEncoder::default()
            .module(&bytes)
            .map_err(|e| format!("attaching {name}: {e}"))?
            .encode()
            .map_err(|e| format!("componentizing {name}: {e}"))?;
        let out = out_dir.join(format!("{name}.wasm"));
        fs::write(&out, component).map_err(|e| e.to_string())?;
        println!("pinned abi/compat/{name}.wasm");
    }
    Ok(())
}

/// Rewrites the committed contract fingerprint — the deliberate act the
/// `contract_fingerprint_is_intentional` gate demands after a shape change.
fn gen_fingerprint() -> Result<()> {
    let path = workspace_root().join("crates/kndo-contract/fingerprint.txt");
    let hex = kndo_contract::contract_fingerprint_hex();
    fs::write(&path, format!("{hex}\n")).map_err(|e| e.to_string())?;
    println!("wrote crates/kndo-contract/fingerprint.txt = {hex}");
    Ok(())
}

/// The measurement loop: run the default engine over every clone in `--corpus-dir`
/// and version the per-repo reports plus one summary table into `--out-dir` (default
/// `corpus-findings/`). Everything written is deterministic — timings go to stdout
/// only, never into the versioned files.
fn corpus(args: &[String]) -> Result<()> {
    let corpus_dir = PathBuf::from(flag(args, "--corpus-dir").ok_or("--corpus-dir is required")?);
    let out_dir =
        workspace_root().join(flag(args, "--out-dir").unwrap_or_else(|| "corpus-findings".into()));
    fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;

    let mut repos: Vec<PathBuf> = fs::read_dir(&corpus_dir)
        .map_err(|e| format!("{}: {e}", corpus_dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    repos.sort();
    if repos.is_empty() {
        return Err(format!("no repositories under {}", corpus_dir.display()));
    }

    let mut summary = String::from(
        "# v2 corpus measurement\n\n\
         The default adapter set over the corpus pinned in `corpus/corpus.toml`.\n\
         Regenerate with `cargo xtask corpus --corpus-dir <clones>`; the oracle to\n\
         compare against is `oracle/`. A repo with zero claimed files speaks a\n\
         language no default adapter claims yet; an `unused` abstention means the\n\
         graph has no roots — nothing in the tree (manifest entries, convention\n\
         roots, dispatch anchors) said where execution starts, so the analysis\n\
         declines to judge rather than accuse everything.\n\n\
         | repo | discovered | claimed | decls | refs | import edges | unresolved | findings | abstentions | diagnostics |\n\
         |---|---|---|---|---|---|---|---|---|---|\n",
    );

    for repo in &repos {
        let name = repo
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("unnameable repo dir")?;
        let session = kndo::open(
            repo,
            kndo::Config {
                threads: kndo::Threads::Auto,
                use_cache: false,
                ..kndo::Config::default()
            },
        )
        .map_err(|e| format!("{name}: {e}"))?;
        let snap = session
            .analyze(kndo::RunMode::Full)
            .map_err(|e| format!("{name}: {e}"))?;
        let report = snap.report();

        let (mut decls, mut refs, mut edges, mut unresolved) = (0u64, 0u64, 0u64, 0u64);
        for f in &snap.graph.files {
            decls += f.evidence.declarations.len() as u64;
            refs += f.evidence.references.len() as u64;
            edges += f.imports.len() as u64;
            unresolved += u64::from(f.unresolved_imports);
        }
        summary.push_str(&format!(
            "| {name} | {} | {} | {decls} | {refs} | {edges} | {unresolved} | {} | {} | {} |\n",
            report.run.files_discovered,
            report.run.files_claimed,
            report.findings.len(),
            report.abstained.len(),
            report.diagnostics.len(),
        ));

        fs::write(
            out_dir.join(format!("{name}.report.json")),
            report.to_json(),
        )
        .map_err(|e| e.to_string())?;
        println!(
            "{name}: {} claimed, {} findings, {} abstentions ({:.2?} total)",
            report.run.files_claimed,
            report.findings.len(),
            report.abstained.len(),
            snap.timings.total(),
        );
    }

    fs::write(out_dir.join("SUMMARY.md"), summary).map_err(|e| e.to_string())?;
    println!("wrote {}", out_dir.display());
    Ok(())
}

/// Rewrites the committed report schema from the types — the deliberate act the
/// `report_schema_is_generated_and_valid` gate demands after an envelope change.
fn gen_schema() -> Result<()> {
    let dir = workspace_root().join("schemas");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(dir.join("report.schema.json"), kndo_core::report_schema())
        .map_err(|e| e.to_string())?;
    fs::write(
        dir.join("query.request.schema.json"),
        kndo_core::query::request_schema(),
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        dir.join("query.response.schema.json"),
        kndo_core::query::response_schema(),
    )
    .map_err(|e| e.to_string())?;
    println!("wrote schemas/report.schema.json + query.request/response.schema.json");
    Ok(())
}

fn host_triple() -> Result<String> {
    let out = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|e| e.to_string())?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(str::to_owned)
        .ok_or_else(|| "rustc -vV had no host line".into())
}

/// The one producer of a release artifact: builds `kndo-cli` for `--target` (the
/// host by default; through `cross` with `--cross`), and packs the binary under
/// the archive's staged directory — `<stem>/kndo` — with a `checksums.txt`
/// beside it. The name and layout are the release table's
/// (`kndo_gates::release`), which every consumer is checked against.
fn package(args: &[String]) -> Result<()> {
    let tag = flag(args, "--tag").ok_or("--tag is required")?;
    let out_dir = workspace_root().join(flag(args, "--out-dir").ok_or("--out-dir is required")?);
    let triple = match flag(args, "--target") {
        Some(t) => t,
        None => host_triple()?,
    };
    let cross = args.iter().any(|a| a == "--cross");
    let root = workspace_root();
    let status = Command::new(if cross { "cross" } else { "cargo" })
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            "kndo-cli",
            "--target",
            &triple,
        ])
        .current_dir(&root)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("release build failed".into());
    }
    let bin = root.join("target").join(&triple).join("release").join(BIN);
    fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let stem = kndo_gates::release::stem(&tag, &triple);
    let archive_name = kndo_gates::release::archive_name(&tag, &triple);
    let archive_path = out_dir.join(&archive_name);
    let file = fs::File::create(&archive_path).map_err(|e| e.to_string())?;
    let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(enc);
    tar.append_path_with_name(&bin, format!("{stem}/{BIN}"))
        .map_err(|e| e.to_string())?;
    tar.into_inner()
        .map_err(|e| e.to_string())?
        .finish()
        .map_err(|e| e.to_string())?;
    let digest = sha256_hex(&archive_path)?;
    fs::write(
        out_dir.join("checksums.txt"),
        format!("{digest}  {archive_name}\n"),
    )
    .map_err(|e| e.to_string())?;
    println!("packaged {archive_name} ({digest})");
    Ok(())
}

fn verify_artifact(args: &[String]) -> Result<()> {
    let dir = workspace_root().join(flag(args, "--dir").ok_or("--dir is required")?);
    let checks =
        fs::read_to_string(dir.join("checksums.txt")).map_err(|e| format!("checksums.txt: {e}"))?;
    let line = checks.lines().next().ok_or("checksums.txt is empty")?;
    let (expected, name) = line.split_once("  ").ok_or("malformed checksums.txt")?;
    let archive = dir.join(name);
    let actual = sha256_hex(&archive)?;
    if actual != expected {
        return Err(format!(
            "checksum mismatch for {name}: expected {expected}, got {actual}"
        ));
    }

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let dest = std::env::temp_dir().join(format!("kndo-install-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let tar_gz = fs::File::open(&archive).map_err(|e| e.to_string())?;
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(tar_gz));
    ar.unpack(&dest).map_err(|e| e.to_string())?;

    // The archive holds one directory named for itself; the binary is inside it.
    let stem = name
        .strip_suffix(".tar.gz")
        .ok_or("the artifact is not a .tar.gz")?;
    let bin = dest.join(stem).join(BIN);
    // The install check is a version handshake — bare `kndo` is a real analysis of
    // the current directory, which is the product, not the smoke test.
    let out = Command::new(&bin)
        .arg("--version")
        .output()
        .map_err(|e| format!("running {}: {e}", bin.display()))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() || !stdout.starts_with("kndo ") {
        return Err(format!(
            "installed binary misbehaved: status {:?}, stdout {stdout:?}",
            out.status
        ));
    }
    println!("verified, installed and ran: {}", stdout.trim());
    let _ = fs::remove_dir_all(&dest);
    Ok(())
}

fn sha256_hex(path: &Path) -> Result<String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
