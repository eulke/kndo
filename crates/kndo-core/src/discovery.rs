//! Discovery — enumerate candidate files and their content hashes (RFC 0001 §4 step 1).
//!
//! Adapter-agnostic: this phase does not know which languages exist, only which files are
//! candidates. Two sources produce the same result shape (see [`TreeSource`]):
//!
//! - **A directory** — walked respecting `.gitignore`/`.ignore` (the `ignore` crate — the same
//!   engine ripgrep uses), every file's content hashed with blake3 (the fast path validated in
//!   spike 0001: 6 ms for a 37.7 MB repo).
//! - **A git tree-ish** — enumerated with `ls-tree` and read via one `cat-file --batch` pipe,
//!   never written to disk (diff modes' "before"/"staged" sides, RFC 0004 §6). The same
//!   exclusion rules the walk applies are reproduced against the tree's own content: hidden
//!   (dot-prefixed) entries skipped, `.kndo/` skipped, and `.gitignore`/`.ignore` files *from
//!   the tree* honored — a tracked-but-ignored file (like kndo's own conformance fixtures) is
//!   excluded from analysis on both sources identically. One known, deliberate divergence: git
//!   refuses to re-include a file whose parent *directory* is excluded (`dir/` + `!dir/f`);
//!   the tree matcher honors the re-include — erring toward analyzing more, never less.
//!
//! Parallel by default (RFC 0008 §2); output order is deterministic regardless of
//! walk/hash/thread-scheduling order (RFC 0008 §4) — results are sorted by path before
//! returning, so ids assigned from this list later are never scheduling-dependent.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::adapter::{Diagnostic, DiagnosticLevel, ProjectPath};
use crate::gitutil;

/// One discovered file: its project-relative path (`/`-separated) and content hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredFile {
    pub path: ProjectPath,
    pub content_hash: [u8; 32],
}

#[derive(Debug)]
pub enum DiscoveryError {
    Root(std::io::Error),
}

/// Discovery result: the files plus everything that could NOT be read — a file silently
/// disappearing from analysis is the worst failure mode a static analyzer has, so every
/// skipped path becomes a diagnostic instead of vanishing (RFC 0001 §6).
#[derive(Debug)]
pub struct Discovered {
    pub files: Vec<DiscoveredFile>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Where a tree's content comes from — assembly is source-blind (RFC 0004 §6: diff modes
/// assemble "the graph at two tree states"; *which bytes* back those states is this type's
/// whole job, and nothing downstream of discovery can tell the difference).
#[derive(Debug, Clone, Copy)]
pub enum TreeSource<'a> {
    /// A real directory on disk (full mode; `--diff`'s "after" side is the working tree).
    Directory(&'a Path),
    /// A git tree-ish read straight from the object database — nothing written to disk.
    GitTree {
        repo_root: &'a Path,
        treeish: &'a str,
        /// Repo-relative subdirectory the analysis is scoped to (`""` = whole repo): tree
        /// paths are filtered to it and returned *relative to it*, so ProjectPaths line up
        /// with a directory walk rooted at that same subdirectory — `kndo check --staged`
        /// from a monorepo package dir compares that package on both sides, same as full
        /// mode scopes to the cwd.
        prefix: &'a str,
    },
}

/// [`Discovered`] plus content access for the assembly phases that need bytes (extraction on a
/// facts-cache miss, manifest extraction). Directory sources read from disk on demand exactly
/// as before; git-tree sources serve the blob bytes already streamed during discovery (they
/// were needed for content hashing anyway — holding them beats a second subprocess round-trip,
/// and they live only for the one assembly call).
pub struct DiscoveredTree {
    pub files: Vec<DiscoveredFile>,
    pub diagnostics: Vec<Diagnostic>,
    reader: ContentReader,
}

enum ContentReader {
    Fs(PathBuf),
    Memory(HashMap<ProjectPath, Vec<u8>>),
}

impl DiscoveredTree {
    pub fn read(&self, path: &ProjectPath) -> std::io::Result<Vec<u8>> {
        match &self.reader {
            ContentReader::Fs(root) => std::fs::read(root.join(path.0.as_str())),
            ContentReader::Memory(map) => map.get(path).cloned().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("{} is not in the tree snapshot", path.0),
                )
            }),
        }
    }
}

/// Discover from either source, yielding the identical result shape (same paths, same hashes,
/// same ordering) — the property the parity test in this module pins down.
pub fn discover_source(source: &TreeSource<'_>) -> Result<DiscoveredTree, DiscoveryError> {
    match source {
        TreeSource::Directory(root) => {
            let discovered = discover(root)?;
            Ok(DiscoveredTree {
                files: discovered.files,
                diagnostics: discovered.diagnostics,
                reader: ContentReader::Fs(root.to_path_buf()),
            })
        }
        TreeSource::GitTree {
            repo_root,
            treeish,
            prefix,
        } => discover_git_tree(repo_root, treeish, prefix),
    }
}

/// Walk `root` respecting ignore files, hash every regular file, and return the result sorted
/// by path — deterministic regardless of filesystem or thread-scheduling order.
pub fn discover(root: &Path) -> Result<Discovered, DiscoveryError> {
    if !root.is_dir() {
        return Err(DiscoveryError::Root(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("not a directory: {}", root.display()),
        )));
    }

    let paths: Vec<std::path::PathBuf> = ignore::WalkBuilder::new(root)
        // Honor .gitignore by content, not by the presence of an actual .git directory —
        // kndo analyzes the tree it's given, git repo or not.
        .require_git(false)
        // `.kndo/` is kndo's own cache (ADR 0004), never project content — excluded
        // unconditionally rather than relying on the project's own `.gitignore` (which a
        // fresh clone may not have updated yet, and which `kndo init` — not this — owns).
        .filter_entry(|e| e.file_name() != std::ffi::OsStr::new(".kndo"))
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| e.into_path())
        .collect();

    let results: Vec<Result<DiscoveredFile, Diagnostic>> = paths
        .par_iter()
        .map(|abs| {
            // No normalized ProjectPath exists yet for these two failures — the absolute
            // path stays in the message instead (still informative; both cases are
            // pathological, not part of the normal skip path below).
            let skip_raw = |why: &str| Diagnostic {
                level: DiagnosticLevel::Warn,
                path: None,
                message: format!("skipped {}: {why}", abs.display()),
                span: None,
            };
            let rel = abs
                .strip_prefix(root)
                .map_err(|_| skip_raw("outside project root"))?;
            // Normalize to `/` — the only path form that crosses the adapter boundary
            // (contracts §2, ProjectPath).
            let rel_str = rel
                .to_str()
                .ok_or_else(|| skip_raw("path is not valid UTF-8"))?
                .replace('\\', "/");
            let path = ProjectPath(rel_str.into());
            let content = std::fs::read(abs).map_err(|e| Diagnostic {
                level: DiagnosticLevel::Warn,
                path: Some(path.clone()),
                message: format!("unreadable ({e})"),
                span: None,
            })?;
            let hash = blake3::hash(&content);
            Ok(DiscoveredFile {
                path,
                content_hash: *hash.as_bytes(),
            })
        })
        .collect();

    let mut files = Vec::with_capacity(results.len());
    let mut diagnostics = Vec::new();
    for r in results {
        match r {
            Ok(f) => files.push(f),
            Err(d) => diagnostics.push(d),
        }
    }
    files.sort_by(|a, b| a.path.0.cmp(&b.path.0));
    diagnostics.sort_by(|a, b| a.message.cmp(&b.message)); // deterministic order here too
    Ok(Discovered { files, diagnostics })
}

fn git_error(e: gitutil::GitError) -> DiscoveryError {
    DiscoveryError::Root(std::io::Error::other(e.0))
}

/// The walk's exclusion rules, reproduced against tree content — see the module doc for the
/// parity contract and its one deliberate divergence.
fn discover_git_tree(
    repo_root: &Path,
    treeish: &str,
    prefix: &str,
) -> Result<DiscoveredTree, DiscoveryError> {
    let entries = gitutil::ls_tree(repo_root, treeish).map_err(git_error)?;
    let mut diagnostics = Vec::new();

    // Split entries into UTF-8-pathed regular files (candidates + ignore-rule files) and
    // everything degraded: non-UTF-8 paths (same skip the walk applies) and symlinks (the walk
    // yields symlinks as non-file entries and never reads them).
    let mut candidates: Vec<(String, String)> = Vec::new(); // (repo-relative path, sha)
    let mut ignore_files: Vec<(String, String)> = Vec::new(); // (containing dir, sha)
    for entry in &entries {
        let Ok(path) = std::str::from_utf8(&entry.path) else {
            diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Warn,
                path: None,
                message: format!(
                    "skipped {}: path is not valid UTF-8",
                    String::from_utf8_lossy(&entry.path)
                ),
                span: None,
            });
            continue;
        };
        if entry.mode == 120000 {
            continue; // symlink — never a regular file on the walk side either
        }
        let basename = path.rsplit('/').next().unwrap_or(path);
        if basename == ".gitignore" || basename == ".ignore" {
            let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            ignore_files.push((dir.to_string(), entry.sha.clone()));
            continue; // rule files drive filtering but are never analyzed (hidden, below)
        }
        // Hidden-entry parity: the walk skips dot-prefixed files and directories by default.
        if path.split('/').any(|c| c.starts_with('.')) {
            continue; // also covers `.kndo/` — kndo's own cache, excluded unconditionally
        }
        candidates.push((path.to_string(), entry.sha.clone()));
    }

    // Build one matcher per directory that carries ignore rules, from the tree's own file
    // contents. Within a directory, `.gitignore` lines load before `.ignore` lines so a tie
    // resolves to `.ignore` (last match wins — the walk's precedence). Across directories,
    // the deepest decisive answer wins, mirroring git.
    ignore_files.sort(); // deterministic build order; also groups same-dir files together
    let ignore_shas: Vec<String> = ignore_files.iter().map(|(_, sha)| sha.clone()).collect();
    let ignore_contents = gitutil::cat_blobs(repo_root, &ignore_shas).map_err(git_error)?;
    let mut matchers: Vec<(String, ignore::gitignore::Gitignore)> = Vec::new();
    {
        let mut by_dir: HashMap<&str, Vec<&str>> = HashMap::new();
        let texts: Vec<Option<String>> = ignore_contents
            .iter()
            .map(|c| c.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()))
            .collect();
        for ((dir, _), text) in ignore_files.iter().zip(&texts) {
            if let Some(text) = text {
                by_dir.entry(dir.as_str()).or_default().push(text.as_str());
            }
        }
        for (dir, texts) in by_dir {
            let mut builder = ignore::gitignore::GitignoreBuilder::new(dir);
            for text in texts {
                for line in text.lines() {
                    let _ = builder.add_line(None, line);
                }
            }
            if let Ok(matcher) = builder.build() {
                matchers.push((dir.to_string(), matcher));
            }
        }
    }
    // Deepest directory first — its verdict shadows every ancestor's.
    matchers.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
    let (global_matcher, _) = ignore::gitignore::Gitignore::global();

    let is_ignored = |path: &str| -> bool {
        for (dir, matcher) in &matchers {
            let applies = dir.is_empty()
                || (path.starts_with(dir.as_str())
                    && path.as_bytes().get(dir.len()) == Some(&b'/'));
            if !applies {
                continue;
            }
            match matcher.matched_path_or_any_parents(path, false) {
                ignore::Match::Ignore(_) => return true,
                ignore::Match::Whitelist(_) => return false,
                ignore::Match::None => {}
            }
        }
        matches!(
            global_matcher.matched_path_or_any_parents(path, false),
            ignore::Match::Ignore(_)
        )
    };

    let kept: Vec<(String, String)> = candidates
        .into_iter()
        .filter(|(path, _)| !is_ignored(path))
        .filter_map(|(path, sha)| {
            // Scope + re-relativize to `prefix` so both diff sides and full mode agree on
            // project-relative paths (see `TreeSource::GitTree::prefix`).
            if prefix.is_empty() {
                return Some((path, sha));
            }
            let rest = path.strip_prefix(prefix)?.strip_prefix('/')?;
            Some((rest.to_string(), sha))
        })
        .collect();

    let shas: Vec<String> = kept.iter().map(|(_, sha)| sha.clone()).collect();
    let contents = gitutil::cat_blobs(repo_root, &shas).map_err(git_error)?;

    let mut files = Vec::with_capacity(kept.len());
    let mut blobs: HashMap<ProjectPath, Vec<u8>> = HashMap::with_capacity(kept.len());
    let hashed: Vec<Option<[u8; 32]>> = contents
        .par_iter()
        .map(|c| c.as_ref().map(|bytes| *blake3::hash(bytes).as_bytes()))
        .collect();
    for (((path, _), content), hash) in kept.into_iter().zip(contents).zip(hashed) {
        let path = ProjectPath(path.into());
        match (content, hash) {
            (Some(content), Some(hash)) => {
                files.push(DiscoveredFile {
                    path: path.clone(),
                    content_hash: hash,
                });
                blobs.insert(path, content);
            }
            _ => diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Warn,
                path: Some(path),
                message: "unreadable (blob missing from the object database)".to_string(),
                span: None,
            }),
        }
    }

    files.sort_by(|a, b| a.path.0.cmp(&b.path.0));
    diagnostics.sort_by(|a, b| a.message.cmp(&b.message));
    Ok(DiscoveredTree {
        files,
        diagnostics,
        reader: ContentReader::Memory(blobs),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("kndo-discovery-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn discovers_files_sorted_and_respects_gitignore() {
        let dir = tmp("basic");
        fs::write(dir.join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.join("b.ts"), "export const b = 1;").unwrap();
        fs::write(dir.join("a.ts"), "export const a = 1;").unwrap();
        fs::write(dir.join("ignored.txt"), "should not appear").unwrap();

        let files = discover(&dir).unwrap().files;
        let paths: Vec<&str> = files.iter().map(|f| f.path.0.as_str()).collect();

        assert_eq!(paths, vec!["a.ts", "b.ts"]);
    }

    #[test]
    fn identical_content_hashes_identically() {
        let dir = tmp("hash");
        fs::write(dir.join("x.ts"), "same").unwrap();
        fs::write(dir.join("y.ts"), "same").unwrap();

        let files = discover(&dir).unwrap().files;
        assert_eq!(files[0].content_hash, files[1].content_hash);
        assert_ne!(files[0].content_hash, [0u8; 32]);
    }

    #[test]
    fn order_is_deterministic_across_runs() {
        let dir = tmp("determinism");
        for i in 0..20 {
            fs::write(dir.join(format!("f{i:02}.ts")), format!("{i}")).unwrap();
        }
        let run1: Vec<_> = discover(&dir)
            .unwrap()
            .files
            .into_iter()
            .map(|f| f.path.0)
            .collect();
        let run2: Vec<_> = discover(&dir)
            .unwrap()
            .files
            .into_iter()
            .map(|f| f.path.0)
            .collect();
        assert_eq!(run1, run2);
    }

    #[test]
    fn rejects_non_directory_root() {
        let dir = tmp("not-a-dir");
        let file = dir.join("f.ts");
        fs::write(&file, "x").unwrap();
        assert!(discover(&file).is_err());
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_and_commit_all(dir: &Path) {
        git(dir, &["init", "-q", "."]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "test"]);
        // Local to this throwaway fixture only — see gitutil's test helper for why.
        git(dir, &["config", "commit.gpgsign", "false"]);
        git(dir, &["add", "-A", "-f"]); // -f: tracked-but-ignored files are part of the point
        git(dir, &["commit", "-q", "-m", "fixture"]);
    }

    fn tree_files(source: &TreeSource<'_>) -> Vec<DiscoveredFile> {
        discover_source(source).unwrap().files
    }

    /// The parity contract, pinned: a committed tree discovered via git must yield the exact
    /// same file set and hashes as walking the identical working tree — including the cases
    /// that make it hard: tracked-but-ignored files (kndo's own fixture corpus shape), nested
    /// `.gitignore`s, whitelists, hidden entries, and `.kndo/`.
    #[test]
    fn git_tree_discovery_matches_directory_discovery() {
        let dir = tmp("git-parity");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::create_dir_all(dir.join("gen")).unwrap();
        fs::create_dir_all(dir.join(".hidden-dir")).unwrap();
        fs::create_dir_all(dir.join(".kndo/cache")).unwrap();
        fs::write(dir.join(".gitignore"), "gen/*\n!gen/keep.ts\n").unwrap();
        fs::write(dir.join("a.ts"), "export const a = 1;").unwrap();
        fs::write(dir.join("b.ts"), "export const b = 2;").unwrap();
        fs::write(dir.join("sub/.gitignore"), "local-ignored.ts\n").unwrap();
        fs::write(dir.join("sub/kept.ts"), "export const kept = 3;").unwrap();
        fs::write(dir.join("sub/local-ignored.ts"), "tracked but ignored").unwrap();
        fs::write(dir.join("gen/drop.ts"), "generated, ignored").unwrap();
        fs::write(dir.join("gen/keep.ts"), "whitelisted back in").unwrap();
        fs::write(dir.join(".hidden-dir/h.ts"), "hidden dir content").unwrap();
        fs::write(dir.join(".dotfile.ts"), "hidden file").unwrap();
        fs::write(dir.join(".kndo/cache/junk"), "never analyzed").unwrap();
        init_and_commit_all(&dir);

        let from_walk = tree_files(&TreeSource::Directory(&dir));
        let from_tree = tree_files(&TreeSource::GitTree {
            repo_root: &dir,
            treeish: "HEAD",
            prefix: "",
        });

        let walk_paths: Vec<&str> = from_walk.iter().map(|f| f.path.0.as_str()).collect();
        assert!(walk_paths.contains(&"gen/keep.ts"), "{walk_paths:?}");
        assert!(
            !walk_paths.contains(&"sub/local-ignored.ts"),
            "{walk_paths:?}"
        );
        assert_eq!(from_walk, from_tree);
    }

    #[test]
    fn git_tree_prefix_scopes_and_relativizes_paths() {
        let dir = tmp("git-prefix");
        fs::create_dir_all(dir.join("pkg/src")).unwrap();
        fs::write(dir.join("outside.ts"), "export const o = 1;").unwrap();
        fs::write(dir.join("pkg/src/inner.ts"), "export const i = 2;").unwrap();
        init_and_commit_all(&dir);

        let files = tree_files(&TreeSource::GitTree {
            repo_root: &dir,
            treeish: "HEAD",
            prefix: "pkg",
        });
        let paths: Vec<&str> = files.iter().map(|f| f.path.0.as_str()).collect();
        assert_eq!(paths, vec!["src/inner.ts"]);
        // …and those paths line up with a walk rooted at the same subdirectory.
        let from_walk = tree_files(&TreeSource::Directory(&dir.join("pkg")));
        assert_eq!(files, from_walk);
    }

    #[test]
    fn git_tree_root_gitignore_applies_to_prefixed_subdirectory_files() {
        // A repo-root .gitignore ignoring something under the prefix must still apply after
        // scoping — the walk's `parents(true)` default does the same from a subdir root.
        let dir = tmp("git-prefix-parent-ignore");
        fs::create_dir_all(dir.join("pkg")).unwrap();
        fs::write(dir.join(".gitignore"), "pkg/generated.ts\n").unwrap();
        fs::write(dir.join("pkg/real.ts"), "export const r = 1;").unwrap();
        fs::write(dir.join("pkg/generated.ts"), "tracked but ignored").unwrap();
        init_and_commit_all(&dir);

        let files = tree_files(&TreeSource::GitTree {
            repo_root: &dir,
            treeish: "HEAD",
            prefix: "pkg",
        });
        let paths: Vec<&str> = files.iter().map(|f| f.path.0.as_str()).collect();
        assert_eq!(paths, vec!["real.ts"]);
    }

    #[test]
    fn git_tree_content_reads_serve_the_blob_bytes() {
        let dir = tmp("git-content");
        fs::write(dir.join("f.ts"), "export const x = 42;").unwrap();
        init_and_commit_all(&dir);
        // Change the working tree AFTER committing — reads must come from the tree, not disk.
        fs::write(dir.join("f.ts"), "changed on disk").unwrap();

        let tree = discover_source(&TreeSource::GitTree {
            repo_root: &dir,
            treeish: "HEAD",
            prefix: "",
        })
        .unwrap();
        let content = tree.read(&ProjectPath("f.ts".into())).unwrap();
        assert_eq!(content, b"export const x = 42;");
    }

    #[test]
    fn git_tree_unknown_treeish_is_an_error_not_a_panic() {
        let dir = tmp("git-bad-treeish");
        fs::write(dir.join("f.ts"), "x").unwrap();
        init_and_commit_all(&dir);
        assert!(discover_source(&TreeSource::GitTree {
            repo_root: &dir,
            treeish: "no-such-ref",
            prefix: "",
        })
        .is_err());
    }
}
