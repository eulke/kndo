//! `cargo xtask vendor --crate C --version V`: re-derives what a vendored tree IS.
//!
//! A vendored tree is upstream plus a named, reviewable change — and until this
//! command existed that sentence lived in prose, where nothing checked it. The
//! record it writes is data: every file's upstream digest, and for each file the
//! tree changes, the reason plus the upstream copy kept beside it. The diff a
//! reviewer reads is then `diff -u` between two files that both exist, which is
//! why no `.patch` is stored: a stored diff can disagree with the tree it claims
//! to describe, and two files cannot.
//!
//! The gate `vendored_trees_are_upstream_plus_patches` reads the record back.

use crate::{Result, workspace_root};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Files a `.crate` archive carries for packaging that a path dependency never
/// reads. They are not part of the vendored tree and not part of its record.
const PACKAGING_ONLY: &[&str] = &[".cargo_vcs_info.json", "Cargo.toml.orig"];

pub(crate) fn run(args: &[String]) -> Result<()> {
    let name = crate::flag(args, "--crate").ok_or("--crate is required")?;
    let version = crate::flag(args, "--version").ok_or("--version is required")?;
    let root = workspace_root();
    let tree = root.join("vendor").join(&name);
    if !tree.is_dir() {
        return Err(format!("{} is not a vendored tree", tree.display()));
    }
    let archive = archive_of(&name, &version)?;
    let upstream = read_archive(&archive)?;
    let vendored = read_tree(&tree)?;
    let record = compare(&name, &version, &archive, &upstream, &vendored)?;

    let upstream_dir = root.join("vendor").join("upstream").join(&name);
    if upstream_dir.exists() {
        std::fs::remove_dir_all(&upstream_dir).map_err(|e| e.to_string())?;
    }
    for path in record.changed.keys() {
        let out = upstream_dir.join(path);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&out, &upstream[path]).map_err(|e| e.to_string())?;
    }
    let toml = root.join("vendor").join(format!("{name}.provenance.toml"));
    let previous = std::fs::read_to_string(&toml).unwrap_or_default();
    std::fs::write(&toml, record.render(&previous)).map_err(|e| e.to_string())?;
    println!(
        "{name} {version}: {} files, {} changed, {} packaging-only files dropped",
        record.files.len(),
        record.changed.len(),
        record.dropped.len()
    );
    Ok(())
}

/// The `.crate` archive in the local registry cache. Vendoring is an authoring
/// step, so requiring the crate to have been fetched once is the whole
/// prerequisite — the gate that reads the record back needs no archive at all.
fn archive_of(name: &str, version: &str) -> Result<PathBuf> {
    let home = std::env::var("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".cargo")))
        .map_err(|_| "neither CARGO_HOME nor HOME is set".to_string())?;
    let cache = home.join("registry").join("cache");
    let wanted = format!("{name}-{version}.crate");
    let mut found = None;
    for entry in std::fs::read_dir(&cache).map_err(|e| format!("{}: {e}", cache.display()))? {
        let dir = entry.map_err(|e| e.to_string())?.path();
        let candidate = dir.join(&wanted);
        if candidate.is_file() {
            found = Some(candidate);
        }
    }
    found.ok_or_else(|| {
        format!(
            "{wanted} is not in {} — `cargo fetch` it first",
            cache.display()
        )
    })
}

fn read_archive(path: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut out = BTreeMap::new();
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let full = entry
            .path()
            .map_err(|e| e.to_string())?
            .display()
            .to_string();
        // Every path in a `.crate` sits under one `<name>-<version>/` directory.
        let Some((_, rest)) = full.split_once('/') else {
            continue;
        };
        let rest = rest.to_string();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).map_err(|e| e.to_string())?;
        out.insert(rest, bytes);
    }
    Ok(out)
}

fn read_tree(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))? {
            let p = entry.map_err(|e| e.to_string())?.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let rel = p
                .strip_prefix(dir)
                .map_err(|e| e.to_string())?
                .display()
                .to_string()
                .replace('\\', "/");
            out.insert(rel, std::fs::read(&p).map_err(|e| e.to_string())?);
        }
    }
    Ok(out)
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

struct Record {
    name: String,
    version: String,
    archive: String,
    /// Every file of the vendored tree, by its UPSTREAM digest.
    files: BTreeMap<String, String>,
    /// The files the tree changes: upstream digest, vendored digest.
    changed: BTreeMap<String, (String, String)>,
    dropped: Vec<String>,
}

fn compare(
    name: &str,
    version: &str,
    archive: &Path,
    upstream: &BTreeMap<String, Vec<u8>>,
    vendored: &BTreeMap<String, Vec<u8>>,
) -> Result<Record> {
    let mut files = BTreeMap::new();
    let mut changed = BTreeMap::new();
    for (path, bytes) in vendored {
        let Some(up) = upstream.get(path) else {
            return Err(format!(
                "{path} is in the vendored tree but not in {name} {version} — a vendored tree adds no files"
            ));
        };
        files.insert(path.clone(), digest(up));
        if up != bytes {
            changed.insert(path.clone(), (digest(up), digest(bytes)));
        }
    }
    let dropped: Vec<String> = upstream
        .keys()
        .filter(|p| !vendored.contains_key(*p))
        .cloned()
        .collect();
    for path in &dropped {
        if !PACKAGING_ONLY.contains(&path.as_str()) {
            return Err(format!(
                "{path} is in {name} {version} but not in the vendored tree — only packaging files may be dropped"
            ));
        }
    }
    Ok(Record {
        name: name.to_string(),
        version: version.to_string(),
        archive: digest(&std::fs::read(archive).map_err(|e| e.to_string())?),
        files,
        changed,
        dropped,
    })
}

impl Record {
    /// The record, with every `why` the previous one carried carried forward:
    /// the digests are this command's to state and the reasons are a person's.
    fn render(&self, previous: &str) -> String {
        let reasons = reasons_of(previous);
        let mut s = format!(
            "# Generated by `cargo xtask vendor --crate {} --version {}`.\n\
             # What `vendor/{}/` IS: this crate's release, plus the changes listed\n\
             # under `[[changed]]` and nothing else. Every other file must still hash to\n\
             # its upstream digest, and `vendored_trees_are_upstream_plus_patches` checks\n\
             # it. The diff a reviewer reads is `diff -u vendor/upstream/{}/<path>\n\
             # vendor/{}/<path>` — both files exist, so it cannot go stale.\n\
             #\n\
             # A `why` is a person's to write and is carried forward across regenerations;\n\
             # a changed file with no reason fails the gate.\n\n",
            self.name, self.version, self.name, self.name, self.name
        );
        s.push_str(&format!("crate = \"{}\"\n", self.name));
        s.push_str(&format!("version = \"{}\"\n", self.version));
        s.push_str(&format!("archive = \"{}\"\n", self.archive));
        s.push_str("dropped = [");
        s.push_str(
            &self
                .dropped
                .iter()
                .map(|p| format!("\"{p}\""))
                .collect::<Vec<_>>()
                .join(", "),
        );
        s.push_str("]\n");
        for (path, (up, vend)) in &self.changed {
            s.push_str("\n[[changed]]\n");
            s.push_str(&format!("path = \"{path}\"\n"));
            s.push_str(&format!("upstream = \"{up}\"\n"));
            s.push_str(&format!("vendored = \"{vend}\"\n"));
            s.push_str(&format!(
                "why = \"{}\"\n",
                reasons.get(path).map(String::as_str).unwrap_or("")
            ));
        }
        s.push_str("\n[files]\n");
        for (path, up) in &self.files {
            s.push_str(&format!("\"{path}\" = \"{up}\"\n"));
        }
        s
    }
}

/// The `why` of each changed file in an existing record, so regenerating keeps
/// what a person wrote.
fn reasons_of(previous: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut path: Option<String> = None;
    for line in previous.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("path = \"") {
            path = rest.strip_suffix('"').map(str::to_string);
        } else if let Some(rest) = line.strip_prefix("why = \"")
            && let (Some(p), Some(w)) = (path.take(), rest.strip_suffix('"'))
        {
            out.insert(p, w.to_string());
        }
    }
    out
}
