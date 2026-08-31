//! Resolution by the package/directory convention Kotlin RECOMMENDS but does
//! not enforce: path suffix over the discovered set, with two fallbacks Java
//! does not need — the `.java` extension (mixed projects share one namespace)
//! and the package DIRECTORY (Kotlin lets `import a.b.Foo` live in any
//! `a/b/*.kt`, file names free). Third-party packages stay `Unresolved`,
//! deliberately. Nearest-module preference as in Java: sibling modules
//! declaring the same package resolve toward the importer's own tree.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;
use kndo_toolkit as tk;

pub fn resolve(from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
    let path = specifier.replace('.', "/");

    for ext in [".kt", ".java"] {
        if let Some(file) = tk::nearest_suffix_match(&format!("{path}{ext}"), from, cx) {
            return Resolution::File(file);
        }
    }
    // The named thing may be a wildcard's package (the EXACT directory — tried
    // first, or `import a.b.*` would grab package `a` whenever `a` holds any
    // source), a top-level function or property in ANY file of its package (the
    // parent directory), or a nested type whose outer class is the file (the
    // peel step, Java's chain) — keep-alive over precision throughout.
    let dir_members = package_dir_files(&path, cx);
    if !dir_members.is_empty() {
        return Resolution::Files(dir_members);
    }
    if let Some((parent, _)) = path.rsplit_once('/') {
        let dir_members = package_dir_files(parent, cx);
        if !dir_members.is_empty() {
            return Resolution::Files(dir_members);
        }
        for ext in [".kt", ".java"] {
            if let Some(file) = tk::nearest_suffix_match(&format!("{parent}{ext}"), from, cx) {
                return Resolution::File(file);
            }
        }
    }
    Resolution::Unresolved
}

/// Every direct source child of any directory whose path ends in the package's
/// segments — `.kt` and `.java` alike, all modules' matches.
fn package_dir_files(dir_suffix: &str, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
    if dir_suffix.is_empty() {
        return Vec::new();
    }
    let want = format!("/{dir_suffix}/");
    let mut out: Vec<ProjectPath> = cx
        .known_files()
        .filter(|p| {
            let s = p.as_str();
            if !(s.ends_with(".kt") || s.ends_with(".java")) {
                return false;
            }
            let Some(dir_end) = s.rfind('/') else {
                return false;
            };
            let dir = &s[..dir_end + 1];
            dir.ends_with(&want) || dir == &want[1..]
        })
        .cloned()
        .collect();
    out.sort();
    out
}

/// The unit is the directory — `.kt` and `.java` siblings share one namespace
/// at compile — plus the standard layout's test→main mirror across BOTH
/// source-set spellings: `src/test/kotlin/<pkg>` sees `src/main/kotlin/<pkg>`
/// AND `src/main/java/<pkg>`, one direction only.
pub fn unit_mates(path: &ProjectPath, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
    let p = path.as_str();
    let dir = match p.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "",
    };
    let main_views: Vec<String> = mirrored_main_dirs(dir);

    let mut mates: Vec<ProjectPath> = cx
        .known_files()
        .filter(|f| {
            let s = f.as_str();
            if s == p || !(s.ends_with(".kt") || s.ends_with(".java")) {
                return false;
            }
            let fd = match s.rsplit_once('/') {
                Some((d, _)) => d,
                None => "",
            };
            fd == dir || main_views.iter().any(|m| m == fd)
        })
        .cloned()
        .collect();
    mates.sort();
    mates.dedup();
    mates
}

/// For a test-set directory (either standard spelling — Kotlin sources live
/// under `src/test/java` in plenty of mixed projects), the main-set
/// directories it shares a package with; for a MAIN-set directory, the sibling
/// main spelling — joint compilation makes `src/main/kotlin/<pkg>` and
/// `src/main/java/<pkg>` one namespace; empty for anything else.
fn mirrored_main_dirs(dir: &str) -> Vec<String> {
    for marker in ["src/test/kotlin", "src/test/java"] {
        if let Some((head, tail)) = split_on_set(dir, marker) {
            return vec![
                format!("{head}src/main/kotlin{tail}"),
                format!("{head}src/main/java{tail}"),
            ];
        }
    }
    if let Some((head, tail)) = split_on_set(dir, "src/main/kotlin") {
        return vec![format!("{head}src/main/java{tail}")];
    }
    if let Some((head, tail)) = split_on_set(dir, "src/main/java") {
        return vec![format!("{head}src/main/kotlin{tail}")];
    }
    Vec::new()
}

/// `head` and `tail` around a `/`-anchored source-set marker, or None.
fn split_on_set<'a>(dir: &'a str, marker: &str) -> Option<(&'a str, &'a str)> {
    let ix = dir.find(marker)?;
    if ix != 0 && dir.as_bytes()[ix - 1] != b'/' {
        return None;
    }
    Some((&dir[..ix], &dir[ix + marker.len()..]))
}
