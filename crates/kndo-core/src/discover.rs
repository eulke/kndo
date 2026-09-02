//! Deterministic project discovery: a gitignore-aware walk (`.gitignore` and
//! `.ignore`, the same pair git and the ignore crate honor), paths normalized to
//! `/`-separated project-relative form, results sorted by path so everything
//! downstream consumes ordered input.

use kndo_contract::vocab::ProjectPath;
use std::collections::BTreeSet;
use std::path::Path;

pub struct DiscoveredFile {
    pub path: ProjectPath,
    pub content: Vec<u8>,
    pub hash: [u8; 32],
}

/// The dot-named entries the walk enters. A hidden entry is not the project's
/// content — tool state, caches, the VCS — except where an extension's manifest
/// glob names one (`.github` in `**/.github/workflows/*.yml`): such a directory
/// carries project facts, and naming it in a glob is what opts it in. No
/// hosting convention is spelled in the engine; the extensions that read one
/// declare it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HiddenOptIn(BTreeSet<String>);

impl HiddenOptIn {
    /// Every dot-named LITERAL segment of the globs (`.github`; never a
    /// pattern such as `.*`).
    pub fn from_manifest_globs<'a>(globs: impl Iterator<Item = &'a str>) -> Self {
        let mut names = BTreeSet::new();
        for glob in globs {
            for segment in glob.split('/') {
                if segment.len() > 1
                    && segment.starts_with('.')
                    && !segment.contains(['*', '?', '[', ']', '{', '}'])
                {
                    names.insert(segment.to_string());
                }
            }
        }
        HiddenOptIn(names)
    }

    fn admits(&self, name: &str) -> bool {
        self.0.contains(name)
    }
}

pub fn discover(root: &Path, hidden: &HiddenOptIn) -> Vec<DiscoveredFile> {
    let mut files = Vec::new();
    // Determinism closes over the TREE: the committed `.gitignore`/`.ignore` files
    // inside it. The walker's other default sources — the user's global excludes,
    // the uncommitted `.git/info/exclude`, and ignore files in parent directories —
    // are machine state, and honoring them lets two checkouts of one tree discover
    // different file sets.
    let admitted = hidden.clone();
    let walker = ignore::WalkBuilder::new(root)
        // Hidden entries are pruned here rather than by the walker's own filter
        // so the opted-in ones can pass; the root itself may be dot-named.
        .hidden(false)
        .filter_entry(move |entry| {
            entry.depth() == 0
                || entry
                    .file_name()
                    .to_str()
                    .is_none_or(|name| !name.starts_with('.') || admitted.admits(name))
        })
        .follow_links(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .build();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let Ok(content) = std::fs::read(entry.path()) else {
            // Unreadable files degrade to absent; the engine never fails a run on one.
            continue;
        };
        let hash = *blake3::hash(&content).as_bytes();
        files.push(DiscoveredFile {
            path: ProjectPath::new(rel),
            content,
            hash,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_entries_are_skipped_unless_a_manifest_glob_names_them() {
        let dir = tempfile::tempdir().expect("tempdir");
        for (path, content) in [
            (".github/workflows/ci.yml", "jobs: {}\n"),
            (".github/actions/report/render.mjs", "export {};\n"),
            (".git/HEAD", "ref: refs/heads/main\n"),
            (".cache/build.js", "x\n"),
            (".eslintrc.js", "x\n"),
            ("src/a.js", "x\n"),
        ] {
            let full = dir.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, content).unwrap();
        }
        let paths = |hidden: &HiddenOptIn| -> Vec<String> {
            discover(dir.path(), hidden)
                .into_iter()
                .map(|f| f.path.as_str().to_string())
                .collect()
        };
        assert_eq!(paths(&HiddenOptIn::default()), ["src/a.js"]);
        let opted = HiddenOptIn::from_manifest_globs(
            ["**/package.json", "**/.github/workflows/*.yml", "**/.*rc"].into_iter(),
        );
        assert_eq!(
            paths(&opted),
            [
                ".github/actions/report/render.mjs",
                ".github/workflows/ci.yml",
                "src/a.js"
            ],
            "the named directory enters whole; a pattern names nothing"
        );
    }
}
