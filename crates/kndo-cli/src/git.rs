//! The CLI's git edge — the ENGINE never speaks git. A diff-mode run is the
//! composition of two full analyses over two pinned trees; this module's whole job
//! is materializing those trees: `git archive <tree-ish> | tar -x` into a scratch
//! directory, stateless (no worktree registration to leak on failure). Tracked
//! files only, by construction — which is also why the worktree's untracked
//! `.kndo/plugins/` is copied in afterwards: both sides of a comparison must run
//! the same composition, or the diff reports the composition, not the change.
//! One tree is never materialized: when the worktree already IS the index as
//! discovery sees it ([`worktree_is_the_index`]), `--staged` judges it in place.

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

    /// The current side: `--staged` compares the INDEX — written out as a tree
    /// (plumbing, side-effect-free for the worktree) unless the worktree already
    /// is the index as discovery sees it, in which case the worktree is judged
    /// in place and nothing is materialized — while a ref diff compares the
    /// worktree itself, in place.
    pub fn current_tree(&self, root: &Path) -> Result<Option<String>, String> {
        match self {
            Comparison::Staged => {
                if worktree_is_the_index(root)? {
                    Ok(None)
                } else {
                    git(root, &["write-tree"]).map(Some)
                }
            }
            Comparison::Against(_) => Ok(None),
        }
    }
}

/// Whether the worktree IS the index as discovery would see it: no tracked file
/// differs from the index (content or presence), and no untracked file is
/// visible under the tree's own `.gitignore` files — the one exclusion source
/// discovery shares with git. `.git/info/exclude` and the global excludes are
/// machine state discovery never consults, so an untracked file only they hide
/// still counts; a file only `.ignore` hides, or a hidden entry the walk would
/// skip, counts too — the conservative direction, which costs a materialization
/// and never a wrong tree. `.kndo/` is kndo's own and never analyzed.
pub fn worktree_is_the_index(root: &Path) -> Result<bool, String> {
    let diff = Command::new("git")
        .args(["diff", "--quiet", "--no-ext-diff"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    match diff.status.code() {
        Some(0) => {}
        Some(1) => return Ok(false),
        _ => return Err(String::from_utf8_lossy(&diff.stderr).trim().to_string()),
    }
    let untracked = git(
        root,
        &["ls-files", "--others", "--exclude-per-directory=.gitignore"],
    )?;
    Ok(untracked
        .lines()
        .all(|line| line == ".kndo" || line.starts_with(".kndo/")))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn committed() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        std::fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        for args in [
            &["init", "-q"][..],
            &["add", "-A"][..],
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "base",
            ][..],
        ] {
            let out = Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .expect("git runs");
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        dir
    }

    #[test]
    fn the_worktree_is_the_index_only_when_nothing_unstaged_is_visible() {
        let repo = committed();
        let root = repo.path();
        assert!(worktree_is_the_index(root).unwrap(), "clean after a commit");
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        assert!(
            !worktree_is_the_index(root).unwrap(),
            "an unstaged modification"
        );
        git(root, &["add", "-A"]).unwrap();
        assert!(
            worktree_is_the_index(root).unwrap(),
            "staged: the index caught up"
        );
        std::fs::write(root.join("new.txt"), "x\n").unwrap();
        assert!(
            !worktree_is_the_index(root).unwrap(),
            "an untracked file discovery would see"
        );
        std::fs::remove_file(root.join("new.txt")).unwrap();
        std::fs::write(root.join("ignored.txt"), "x\n").unwrap();
        assert!(
            worktree_is_the_index(root).unwrap(),
            "an untracked file .gitignore hides is invisible to both"
        );
        std::fs::create_dir_all(root.join(".kndo/cache")).unwrap();
        std::fs::write(root.join(".kndo/cache/x"), "x").unwrap();
        assert!(
            worktree_is_the_index(root).unwrap(),
            "kndo's own directory never counts"
        );
        std::fs::remove_file(root.join("a.txt")).unwrap();
        assert!(
            !worktree_is_the_index(root).unwrap(),
            "an unstaged deletion"
        );
    }
}
