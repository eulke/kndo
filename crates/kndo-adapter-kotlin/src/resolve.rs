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
