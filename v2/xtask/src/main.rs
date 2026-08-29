//! v2's task runner. `gen-ci` renders the workflow from kndo-gates' registry;
//! `package` builds and archives the release binary with a checksum; `verify-artifact`
//! checksum-verifies, extracts and runs it — the install half of the release loop,
//! written in Rust so all three CI platforms run the identical check instead of three
//! shell dialects.

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
        Some("package") => package(&args[1..]),
        Some("verify-artifact") => verify_artifact(&args[1..]),
        _ => {
            eprintln!(
                "usage: cargo xtask <gen-ci | package --tag T --out-dir D | verify-artifact --dir D>"
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

fn package(args: &[String]) -> Result<()> {
    let tag = flag(args, "--tag").ok_or("--tag is required")?;
    let out_dir = workspace_root().join(flag(args, "--out-dir").ok_or("--out-dir is required")?);
    let root = workspace_root();

    let status = Command::new("cargo")
        .args(["build", "--release", "-p", "kndo-cli"])
        .current_dir(&root)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("release build failed".into());
    }

    let triple = host_triple()?;
    let bin = root.join("target/release").join(BIN);
    fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let archive_name = format!("kndo-{tag}-{triple}.tar.gz");
    let archive_path = out_dir.join(&archive_name);

    let file = fs::File::create(&archive_path).map_err(|e| e.to_string())?;
    let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(enc);
    tar.append_path_with_name(&bin, BIN)
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

    let bin = dest.join(BIN);
    let out = Command::new(&bin)
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
