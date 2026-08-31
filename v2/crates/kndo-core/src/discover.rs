//! Deterministic project discovery: a gitignore-aware walk (`.gitignore` and
//! `.ignore`, the same pair git and the ignore crate honor), paths normalized to
//! `/`-separated project-relative form, results sorted by path so everything
//! downstream consumes ordered input.

use kndo_contract::vocab::ProjectPath;
use std::path::Path;

pub struct DiscoveredFile {
    pub path: ProjectPath,
    pub content: Vec<u8>,
    pub hash: [u8; 32],
}

pub fn discover(root: &Path) -> Vec<DiscoveredFile> {
    let mut files = Vec::new();
    // Determinism closes over the TREE: the committed `.gitignore`/`.ignore` files
    // inside it. The walker's other default sources — the user's global excludes,
    // the uncommitted `.git/info/exclude`, and ignore files in parent directories —
    // are machine state, and honoring them lets two checkouts of one tree discover
    // different file sets.
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
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
