//! Discovery — enumerate candidate files and their content hashes (RFC 0001 §4 step 1).
//!
//! Adapter-agnostic: this phase does not know which languages exist, only which files are
//! candidates. It walks the project respecting `.gitignore`/`.ignore` (the `ignore` crate —
//! the same engine ripgrep uses) and hashes every file's content with blake3, the fast path
//! validated in spike 0001 (6 ms for a 37.7 MB repo).
//!
//! Parallel by default (RFC 0008 §2); output order is deterministic regardless of
//! walk/hash/thread-scheduling order (RFC 0008 §4) — results are sorted by path before
//! returning, so ids assigned from this list later are never scheduling-dependent.

use std::path::Path;

use rayon::prelude::*;

use crate::adapter::{Diagnostic, DiagnosticLevel, ProjectPath};

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
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| e.into_path())
        .collect();

    let results: Vec<Result<DiscoveredFile, Diagnostic>> = paths
        .par_iter()
        .map(|abs| {
            let skip = |why: String| Diagnostic {
                level: DiagnosticLevel::Warn,
                message: format!("skipped {}: {why}", abs.display()),
                span: None,
            };
            let rel = abs
                .strip_prefix(root)
                .map_err(|_| skip("outside project root".into()))?;
            // Normalize to `/` — the only path form that crosses the adapter boundary
            // (contracts §2, ProjectPath).
            let rel_str = rel
                .to_str()
                .ok_or_else(|| skip("path is not valid UTF-8".into()))?
                .replace('\\', "/");
            let content = std::fs::read(abs).map_err(|e| skip(format!("unreadable ({e})")))?;
            let hash = blake3::hash(&content);
            Ok(DiscoveredFile {
                path: ProjectPath(rel_str.into()),
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
}
