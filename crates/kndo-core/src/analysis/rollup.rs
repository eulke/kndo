//! Directory rollup (the taxonomy rule: "a directory whose every file carries the
//! same verdict rolls up once more — the widest uniform node gets one finding, not fifty").
//! Shared by every file-granularity analysis with a verdict that can span a whole directory
//! (`unused`, `test-only`, …) — one mechanism, not one reimplementation per verdict: what
//! qualifies as "the same verdict" is each caller's own business (an eligibility map), but
//! *how* eligible files fold into the widest non-overlapping directories is identical
//! regardless of which verdict is asking.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::graph::{self, ProjectGraph};
use crate::vocab::{FileId, PackageId};

// kndo:allow internal-only read through inferred-typed locals at the call sites, field accesses the graph cannot attribute (internal/detection-gaps.md §3)
pub(crate) struct DirRollup<'a> {
    /// The widest directories where every file underneath is eligible, deepest-independent
    /// (never both a directory and one of its own ancestors).
    pub dirs: Vec<DirGroup<'a>>,
    /// Every file path folded into one of `dirs` — excluded from the caller's per-file list.
    pub covered: HashSet<&'a str>,
}

pub(crate) struct DirGroup<'a> {
    pub path: &'a str,
    pub files: Vec<&'a str>,
    pub package: PackageId,
}

/// `eligible` maps a file's project-relative path to `(FileId, PackageId)` for every file the
/// caller's verdict currently holds true for — a non-eligible file anywhere in a directory's
/// subtree (including files merely out of scope for the verdict, e.g. unclaimed/generated)
/// blocks the rollup for every one of its ancestors.
pub(crate) fn directory_rollups<'a>(
    graph: &'a ProjectGraph,
    eligible: &HashMap<&'a str, (FileId, PackageId)>,
) -> DirRollup<'a> {
    // Every directory that owns at least one file in the *whole project* (not just the
    // eligible ones) — an ineligible file here is exactly what should block its ancestors from
    // rolling up, so it has to be in this index too.
    let mut dir_files: HashMap<&'a str, Vec<&'a str>> = HashMap::default();
    for file in &graph.files {
        let path = file.path.0.as_str();
        for ancestor in ancestors(path) {
            dir_files.entry(ancestor).or_default().push(path);
        }
    }

    let mut fully_eligible: Vec<&str> = dir_files
        .iter()
        // `files.len() >= 2`: a one-file "directory" rollup is worse than the plain file
        // finding it would replace ("delete/this-whole-folder-applies-to" about a single file
        // is just a roundabout way of saying "this file") — same floor `duplicate` applies to
        // its own grouping, for the same reason: a group of one isn't a group.
        .filter(|(_, files)| files.len() >= 2 && files.iter().all(|f| eligible.contains_key(f)))
        .map(|(&dir, _)| dir)
        .collect();
    // Widest first (fewest path segments) so a directory is skipped once its parent already
    // qualifies — the "widest uniform node" the taxonomy rule asks for, not every level.
    // Depth by segment *count*, not slash count: "" (root, depth 0) and "src" (depth 1) both
    // contain zero '/' characters, so `matches('/').count()` alone ties them — and a tie here
    // is exactly the bug, since the two are not remotely the same width.
    fully_eligible.sort_by_key(|d| depth(d));

    let mut dirs = Vec::new();
    let mut covered = HashSet::default();
    for dir in fully_eligible {
        if dirs
            .iter()
            .any(|g: &DirGroup| graph::package_owns(g.path, dir))
        {
            continue; // already covered by a wider rollup already accepted
        }
        let files = dir_files.remove(dir).unwrap_or_default();
        // All files here are eligible (verified above) and therefore claimed (`eligible` only
        // ever holds claimed files), so every one shares a package — a nested package boundary
        // would have introduced an unclaimed manifest and blocked the rollup already.
        let package = files
            .first()
            .and_then(|f| eligible.get(f))
            .map(|&(_, p)| p)
            .unwrap_or(PackageId(0));
        covered.extend(files.iter().copied());
        dirs.push(DirGroup {
            path: dir,
            files,
            package,
        });
    }
    DirRollup { dirs, covered }
}

fn depth(dir: &str) -> usize {
    if dir.is_empty() {
        0
    } else {
        dir.matches('/').count() + 1
    }
}

fn ancestors(path: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut current = graph::core_dirname(path);
    loop {
        out.push(current);
        if current.is_empty() {
            break;
        }
        current = graph::core_dirname(current);
    }
    out
}
