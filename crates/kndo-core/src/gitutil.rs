//! Git plumbing for diff modes (RFC 0004 §6, RFC 0006 §2) — `--staged`/`--diff <ref>` need to
//! assemble the project graph at two different tree states (the comparison's "before" and,
//! for `--staged`, "after" too) without disturbing the user's real working tree or index.
//!
//! Shells out to the `git` binary rather than a library binding: diff mode is inherently
//! git-specific already (there is no "diff mode" without git), so a subprocess dependency here
//! costs nothing a library wouldn't already require, and it avoids either a heavy C binding
//! (`libgit2`) or reimplementing tree/index plumbing in pure Rust for what's a handful of
//! well-defined, stable commands.
//!
//! Every write here is index-file-scoped via `GIT_INDEX_FILE` pointed at a throwaway path —
//! `read-tree`/`checkout-index` never touch the repository's real index or working tree,
//! regardless of what the user currently has staged or modified.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub struct GitError(pub String);

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for GitError {}

fn run(args: &[&str], envs: &[(&str, &Path)]) -> Result<String, GitError> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let output = cmd
        .output()
        .map_err(|e| GitError(format!("failed to run `git {}`: {e}", args.join(" "))))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(GitError(if stderr.is_empty() {
            format!("`git {}` failed", args.join(" "))
        } else {
            stderr
        }));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The repository root containing `start` (`git rev-parse --show-toplevel`) — every other call
/// here takes this as its working directory, not the arbitrary path `Engine` was opened with:
/// diff modes must work the same way from any subdirectory, exactly like plain git does.
pub fn repo_root(start: &Path) -> Result<PathBuf, GitError> {
    let start_str = start.to_string_lossy();
    let out = run(&["-C", &start_str, "rev-parse", "--show-toplevel"], &[])?;
    Ok(PathBuf::from(out))
}

pub fn rev_parse(repo_root: &Path, refname: &str) -> Result<String, GitError> {
    let root_str = repo_root.to_string_lossy();
    run(&["-C", &root_str, "rev-parse", refname], &[])
}

pub fn merge_base(repo_root: &Path, a: &str, b: &str) -> Result<String, GitError> {
    let root_str = repo_root.to_string_lossy();
    run(&["-C", &root_str, "merge-base", a, b], &[])
}

/// A tree object for the index exactly as it stands right now — "what would be committed if
/// you ran `git commit`" — computed without touching the real index (`git write-tree` only
/// ever reads it).
pub fn write_tree(repo_root: &Path) -> Result<String, GitError> {
    let root_str = repo_root.to_string_lossy();
    run(&["-C", &root_str, "write-tree"], &[])
}

/// Checks out `treeish`'s complete content into a fresh temporary directory, returned so the
/// caller controls its lifetime (cleaned up automatically on drop). Uses a second, equally
/// throwaway directory to hold the temporary index file `GIT_INDEX_FILE` points `read-tree`/
/// `checkout-index` at — never the repository's real index.
pub fn materialize(repo_root: &Path, treeish: &str) -> Result<tempfile::TempDir, GitError> {
    let dest = tempfile::Builder::new()
        .prefix("kndo-tree-")
        .tempdir()
        .map_err(|e| GitError(format!("failed to create a temp directory: {e}")))?;
    let index_holder = tempfile::Builder::new()
        .prefix("kndo-index-")
        .tempdir()
        .map_err(|e| GitError(format!("failed to create a temp directory: {e}")))?;
    let index_file = index_holder.path().join("index");

    let root_str = repo_root.to_string_lossy();
    run(
        &["-C", &root_str, "read-tree", treeish],
        &[("GIT_INDEX_FILE", index_file.as_path())],
    )?;
    let work_tree_flag = format!("--work-tree={}", dest.path().display());
    run(
        &["-C", &root_str, &work_tree_flag, "checkout-index", "--all"],
        &[("GIT_INDEX_FILE", index_file.as_path())],
    )?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command as StdCommand;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kndo-gitutil-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = StdCommand::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo(dir: &Path) {
        git(dir, &["init", "-q", "."]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "test"]);
        // Local to this throwaway test fixture only — never touches the real repo's config or
        // any commit made outside `/tmp`. Without it these test commits inherit this sandbox's
        // global `commit.gpgsign=true` (signed via an MCP-backed tool), which is unrelated to
        // what's under test here and occasionally times out, flaking the suite.
        git(dir, &["config", "commit.gpgsign", "false"]);
    }

    #[test]
    fn repo_root_finds_the_toplevel_from_a_subdirectory() {
        let dir = tmp("repo-root");
        init_repo(&dir);
        fs::create_dir_all(dir.join("src/nested")).unwrap();
        let found = repo_root(&dir.join("src/nested")).unwrap();
        // Canonicalize both sides: on macOS /tmp is a symlink to /private/tmp, and git
        // resolves it while our own join()ed path doesn't.
        assert_eq!(found.canonicalize().unwrap(), dir.canonicalize().unwrap());
    }

    #[test]
    fn repo_root_errors_outside_any_repository() {
        let dir = tmp("no-repo");
        assert!(repo_root(&dir).is_err());
    }

    #[test]
    fn materialize_head_ignores_unstaged_and_staged_changes() {
        let dir = tmp("materialize-head");
        init_repo(&dir);
        fs::write(dir.join("a.txt"), "committed\n").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "init"]);

        fs::write(dir.join("b.txt"), "staged\n").unwrap();
        git(&dir, &["add", "b.txt"]);
        fs::write(dir.join("a.txt"), "unstaged edit\n").unwrap();

        let materialized = materialize(&dir, "HEAD").unwrap();
        assert_eq!(
            fs::read_to_string(materialized.path().join("a.txt")).unwrap(),
            "committed\n"
        );
        assert!(!materialized.path().join("b.txt").exists());
    }

    #[test]
    fn materialize_index_reflects_exactly_what_is_staged() {
        let dir = tmp("materialize-index");
        init_repo(&dir);
        fs::write(dir.join("a.txt"), "committed\n").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "init"]);

        fs::write(dir.join("b.txt"), "staged\n").unwrap();
        git(&dir, &["add", "b.txt"]);
        fs::write(dir.join("a.txt"), "unstaged edit\n").unwrap(); // must NOT appear below

        let index_tree = write_tree(&dir).unwrap();
        let materialized = materialize(&dir, &index_tree).unwrap();
        assert_eq!(
            fs::read_to_string(materialized.path().join("a.txt")).unwrap(),
            "committed\n"
        );
        assert_eq!(
            fs::read_to_string(materialized.path().join("b.txt")).unwrap(),
            "staged\n"
        );

        // The real repo's own index/working tree must be untouched by any of this.
        let status = StdCommand::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["status", "--porcelain"])
            .output()
            .unwrap();
        let status_text = String::from_utf8_lossy(&status.stdout);
        assert!(status_text.contains("A  b.txt") || status_text.contains("A b.txt"));
        assert!(status_text.contains("a.txt")); // unstaged modification still shows as dirty
    }

    #[test]
    fn merge_base_finds_the_common_ancestor() {
        let dir = tmp("merge-base");
        init_repo(&dir);
        fs::write(dir.join("a.txt"), "1\n").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "base"]);
        let base_sha = rev_parse(&dir, "HEAD").unwrap();

        git(&dir, &["checkout", "-q", "-b", "feature"]);
        fs::write(dir.join("b.txt"), "2\n").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "feature"]);

        let mb = merge_base(&dir, "feature", &base_sha).unwrap();
        assert_eq!(mb, base_sha);
    }

    #[test]
    fn merge_base_errors_on_an_unknown_ref() {
        let dir = tmp("merge-base-bad-ref");
        init_repo(&dir);
        fs::write(dir.join("a.txt"), "1\n").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "init"]);
        assert!(merge_base(&dir, "no-such-ref", "HEAD").is_err());
    }
}
