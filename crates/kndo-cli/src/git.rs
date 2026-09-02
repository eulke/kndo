//! The CLI's git edge — the ENGINE never speaks git. A diff-mode run is the
//! composition of two full analyses over two pinned trees; this module's whole job
//! is materializing those trees: `git archive <tree-ish> | tar -x` into a scratch
//! directory, stateless (no worktree registration to leak on failure). Tracked
//! files only, by construction — which is also why the worktree's untracked
//! `.kndo/plugins/` is copied in afterwards: both sides of a comparison must run
//! the same composition, or the diff reports the composition, not the change.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The base and current trees of one comparison, resolved to tree-ish revisions.
pub enum Comparison {
    /// `--staged`: what `git commit` would commit (the index) against HEAD.
    Staged,
    /// `--diff <ref>`: the worktree against `merge-base(<ref>, HEAD)`.
    Against(String),
}

pub struct MaterializedTree {
    pub root: PathBuf,
    /// Deleting the scratch directory is best-effort on drop.
    _dir: tempfile::TempDir,
}

/// Run `git` in `root`, returning trimmed stdout or the command's own stderr.
fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The tree a revision names — the identity a pinned side is keyed by. Two
/// commits with one tree (an amend that only rewords) share it.
pub fn tree_id(root: &Path, rev: &str) -> Result<String, String> {
    git(root, &["rev-parse", &format!("{rev}^{{tree}}")])
}

/// Materialize one tree-ish into a scratch directory via `git archive | tar -x`.
pub fn materialize(root: &Path, tree_ish: &str) -> Result<MaterializedTree, String> {
    let dir = tempfile::tempdir().map_err(|e| format!("could not create a scratch dir: {e}"))?;
    let mut archive = Command::new("git")
        .args(["archive", "--format=tar", tree_ish])
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run git archive: {e}"))?;
    let tar_in: Stdio = archive
        .stdout
        .take()
        .expect("piped stdout is present")
        .into();
    let untar = Command::new("tar")
        .args(["-x", "-C"])
        .arg(dir.path())
        .stdin(tar_in)
        .output()
        .map_err(|e| format!("could not run tar: {e}"))?;
    let archive = archive
        .wait_with_output()
        .map_err(|e| format!("git archive did not finish: {e}"))?;
    if !archive.status.success() {
        return Err(String::from_utf8_lossy(&archive.stderr).trim().to_string());
    }
    if !untar.status.success() {
        return Err(String::from_utf8_lossy(&untar.stderr).trim().to_string());
    }
    copy_untracked_plugins(root, dir.path());
    Ok(MaterializedTree {
        root: dir.path().to_path_buf(),
        _dir: dir,
    })
}

/// Resolve the comparison's base tree-ish (and for `--staged`, the current one).
impl Comparison {
    /// The base side: HEAD for staged, the merge-base commit for a ref diff.
    pub fn base_tree(&self, root: &Path) -> Result<String, String> {
        match self {
            Comparison::Staged => git(root, &["rev-parse", "HEAD"]),
            Comparison::Against(reference) => git(root, &["merge-base", reference, "HEAD"]),
        }
    }

    /// The current side: `--staged` compares the INDEX (written out as a tree —
    /// plumbing, side-effect-free for the worktree), while a ref diff compares
    /// the worktree itself, in place.
    pub fn current_tree(&self, root: &Path) -> Result<Option<String>, String> {
        match self {
            Comparison::Staged => git(root, &["write-tree"]).map(Some),
            Comparison::Against(_) => Ok(None),
        }
    }
}

/// An archived tree holds tracked files only; the analysis composition also loads
/// `.kndo/plugins/*.wasm`, which is typically untracked. Copy it so both sides of
/// the comparison run identical compositions. Best-effort: a failure here shows
/// up as the composition diagnostic the loader already emits.
fn copy_untracked_plugins(root: &Path, tree: &Path) {
    let source = root.join(".kndo/plugins");
    let Ok(entries) = std::fs::read_dir(&source) else {
        return;
    };
    let target = tree.join(".kndo/plugins");
    if std::fs::create_dir_all(&target).is_err() {
        return;
    }
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        if target.join(&name).exists() {
            continue;
        }
        let _ = std::fs::copy(entry.path(), target.join(&name));
    }
}
