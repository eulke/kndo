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
//! Trees are read **in memory, never materialized to disk**: [`ls_tree`] enumerates a
//! tree-ish's paths + blob ids without touching the filesystem, and [`cat_blobs`] streams the
//! blob contents needed over one `cat-file --batch` pipe. An earlier implementation checked
//! whole trees out into temp directories (`read-tree` + `checkout-index`) so the existing
//! directory-walking pipeline could run on them unchanged — measured at ~0.5 s of pure
//! file-creation syscalls per tree at 5k files (×2 trees for `--staged`, plus cleanup), it was
//! the dominant cost of diff mode and blew RFC 0008's warm budget; enumerating + streaming the
//! same tree costs milliseconds. Everything here is read-only against the repository — the
//! only object-database *write* diff mode ever performs is `write-tree` (staged mode's
//! "after"), which adds a tree object without touching the index or working tree.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

/// One entry of a recursively-listed tree: repo-relative path (raw bytes — git paths aren't
/// guaranteed UTF-8; the caller decides how to degrade), blob id, and file mode (`100644`
/// regular, `100755` executable, `120000` symlink, `160000` submodule).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: Vec<u8>,
    pub sha: String,
    pub mode: u32,
}

/// `git ls-tree -r -z <treeish>` — every blob in the tree, recursively, without touching the
/// filesystem. `-z` (NUL-delimited records, no path quoting) so unusual filenames survive
/// byte-exact.
pub fn ls_tree(repo_root: &Path, treeish: &str) -> Result<Vec<TreeEntry>, GitError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["ls-tree", "-r", "-z", treeish])
        .output()
        .map_err(|e| GitError(format!("failed to run `git ls-tree`: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(GitError(if stderr.is_empty() {
            format!("`git ls-tree {treeish}` failed")
        } else {
            stderr
        }));
    }

    // Record shape: `<mode> <type> <sha>\t<path>` terminated by NUL.
    let mut entries = Vec::new();
    for record in output.stdout.split(|&b| b == 0) {
        if record.is_empty() {
            continue;
        }
        let Some(tab) = record.iter().position(|&b| b == b'\t') else {
            return Err(GitError(
                "malformed `git ls-tree -z` record (no tab)".into(),
            ));
        };
        let (meta, path) = (&record[..tab], &record[tab + 1..]);
        let meta = std::str::from_utf8(meta)
            .map_err(|_| GitError("malformed `git ls-tree -z` record (non-UTF-8 header)".into()))?;
        let mut fields = meta.split(' ');
        let (Some(mode), Some(kind), Some(sha)) = (fields.next(), fields.next(), fields.next())
        else {
            return Err(GitError(format!(
                "malformed `git ls-tree -z` record: {meta}"
            )));
        };
        if kind != "blob" {
            continue; // submodules (`commit` entries) have no content to analyze
        }
        let mode: u32 = mode
            .parse()
            .map_err(|_| GitError(format!("malformed ls-tree mode: {mode}")))?;
        entries.push(TreeEntry {
            path: path.to_vec(),
            sha: sha.to_string(),
            mode,
        });
    }
    Ok(entries)
}

/// Streams the contents of `shas` over one `git cat-file --batch` pipe, returned in request
/// order — `None` for an id git reports missing (the caller degrades it to a diagnostic, the
/// same contract as an unreadable file in a directory walk). One subprocess for the whole
/// batch: per-blob `git show` spawns would cost a process each, and the batch pipe is what
/// makes tree reading ~5× cheaper than materializing the tree to disk ever was.
pub fn cat_blobs(repo_root: &Path, shas: &[String]) -> Result<Vec<Option<Vec<u8>>>, GitError> {
    if shas.is_empty() {
        return Ok(Vec::new());
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| GitError(format!("failed to run `git cat-file --batch`: {e}")))?;

    // Feed requests from a separate thread: writing every request before reading any response
    // deadlocks once either pipe's buffer fills (git blocks writing responses we aren't
    // reading; we block writing requests git isn't consuming).
    let mut stdin = child.stdin.take().expect("stdin was piped");
    let request: String = shas.iter().map(|s| format!("{s}\n")).collect();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(request.as_bytes());
        // stdin drops here, closing the pipe — cat-file exits after the last response.
    });

    let mut reader = BufReader::new(child.stdout.take().expect("stdout was piped"));
    let mut results = Vec::with_capacity(shas.len());
    for _ in shas {
        // Response header: `<sha> <type> <size>\n`, or `<sha> missing\n`.
        let mut header = String::new();
        reader
            .read_line(&mut header)
            .map_err(|e| GitError(format!("reading `git cat-file --batch` output: {e}")))?;
        let header = header.trim_end();
        if header.ends_with(" missing") || header.is_empty() {
            results.push(None);
            continue;
        }
        let size: usize = header
            .rsplit(' ')
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| GitError(format!("malformed cat-file header: {header}")))?;
        let mut content = vec![0u8; size];
        reader
            .read_exact(&mut content)
            .map_err(|e| GitError(format!("reading blob content: {e}")))?;
        let mut newline = [0u8; 1];
        reader
            .read_exact(&mut newline)
            .map_err(|e| GitError(format!("reading blob terminator: {e}")))?;
        results.push(Some(content));
    }

    let _ = writer.join();
    let _ = child.wait();
    Ok(results)
}

/// A persistent `cat-file --batch` child for on-demand, one-blob-at-a-time reads — the lazy
/// fallback behind `discovery`'s in-memory tree reader, for the rare case where assembly asks
/// for a blob whose content discovery skipped fetching (its hash was already known via the
/// blob-hash sidecar, and content is only needed again on a facts-cache miss). One request in
/// flight at a time; the caller serializes access (a `Mutex` in the reader). Worst case —
/// every file missing facts — degrades to one pipe round-trip per file (~100 µs each), never
/// one process spawn per file.
pub struct BlobFetcher {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl BlobFetcher {
    pub fn spawn(repo_root: &Path) -> Result<BlobFetcher, GitError> {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| GitError(format!("failed to run `git cat-file --batch`: {e}")))?;
        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = BufReader::new(child.stdout.take().expect("stdout was piped"));
        Ok(BlobFetcher {
            child,
            stdin,
            stdout,
        })
    }

    /// `Ok(None)` for an id git reports missing; `Err` only for a broken pipe/protocol (the
    /// caller degrades either to its own not-found error).
    pub fn fetch(&mut self, sha: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.stdin.write_all(sha.as_bytes())?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;

        let mut header = String::new();
        self.stdout.read_line(&mut header)?;
        let header = header.trim_end();
        if header.ends_with(" missing") || header.is_empty() {
            return Ok(None);
        }
        let size: usize = header
            .rsplit(' ')
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| std::io::Error::other(format!("malformed cat-file header: {header}")))?;
        let mut content = vec![0u8; size];
        self.stdout.read_exact(&mut content)?;
        let mut newline = [0u8; 1];
        self.stdout.read_exact(&mut newline)?;
        Ok(Some(content))
    }
}

impl Drop for BlobFetcher {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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

    /// Reads one path's content out of a tree via ls_tree + cat_blobs — the composed operation
    /// diff mode performs, minus discovery's filtering.
    fn tree_content(dir: &Path, treeish: &str, path: &str) -> Option<Vec<u8>> {
        let entries = ls_tree(dir, treeish).unwrap();
        let entry = entries.iter().find(|e| e.path == path.as_bytes())?;
        cat_blobs(dir, std::slice::from_ref(&entry.sha))
            .unwrap()
            .pop()
            .flatten()
    }

    #[test]
    fn head_tree_ignores_unstaged_and_staged_changes() {
        let dir = tmp("tree-head");
        init_repo(&dir);
        fs::write(dir.join("a.txt"), "committed\n").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "init"]);

        fs::write(dir.join("b.txt"), "staged\n").unwrap();
        git(&dir, &["add", "b.txt"]);
        fs::write(dir.join("a.txt"), "unstaged edit\n").unwrap();

        assert_eq!(tree_content(&dir, "HEAD", "a.txt").unwrap(), b"committed\n");
        assert_eq!(tree_content(&dir, "HEAD", "b.txt"), None);
    }

    #[test]
    fn index_tree_reflects_exactly_what_is_staged() {
        let dir = tmp("tree-index");
        init_repo(&dir);
        fs::write(dir.join("a.txt"), "committed\n").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "init"]);

        fs::write(dir.join("b.txt"), "staged\n").unwrap();
        git(&dir, &["add", "b.txt"]);
        fs::write(dir.join("a.txt"), "unstaged edit\n").unwrap(); // must NOT appear below

        let index_tree = write_tree(&dir).unwrap();
        assert_eq!(
            tree_content(&dir, &index_tree, "a.txt").unwrap(),
            b"committed\n"
        );
        assert_eq!(
            tree_content(&dir, &index_tree, "b.txt").unwrap(),
            b"staged\n"
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
    fn ls_tree_reports_modes_and_recurses_into_subdirectories() {
        let dir = tmp("ls-tree-modes");
        init_repo(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("plain.txt"), "x\n").unwrap();
        fs::write(dir.join("sub/nested.txt"), "y\n").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["update-index", "--chmod=+x", "plain.txt"]);
        git(&dir, &["commit", "-q", "-m", "init"]);

        let entries = ls_tree(&dir, "HEAD").unwrap();
        let by_path = |p: &str| entries.iter().find(|e| e.path == p.as_bytes()).unwrap();
        assert_eq!(by_path("plain.txt").mode, 100755);
        assert_eq!(by_path("sub/nested.txt").mode, 100644);
    }

    #[test]
    fn cat_blobs_returns_contents_in_request_order_and_none_for_missing() {
        let dir = tmp("cat-blobs");
        init_repo(&dir);
        fs::write(dir.join("a.txt"), "alpha\n").unwrap();
        fs::write(dir.join("b.txt"), "beta\n").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "init"]);

        let entries = ls_tree(&dir, "HEAD").unwrap();
        let sha = |p: &str| {
            entries
                .iter()
                .find(|e| e.path == p.as_bytes())
                .unwrap()
                .sha
                .clone()
        };
        let bogus = "0000000000000000000000000000000000000000".to_string();
        let got = cat_blobs(&dir, &[sha("b.txt"), bogus, sha("a.txt")]).unwrap();
        assert_eq!(got[0].as_deref(), Some(b"beta\n".as_slice()));
        assert_eq!(got[1], None);
        assert_eq!(got[2].as_deref(), Some(b"alpha\n".as_slice()));
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
