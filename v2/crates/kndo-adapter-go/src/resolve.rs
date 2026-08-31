//! Import resolution, package-shaped: an import path maps to a DIRECTORY through
//! the longest module-path prefix a `go.mod` declares, and the resolution is every
//! non-test `.go` file in it — [`Resolution::Files`], the engine drawing one edge
//! per file. The same directory fact answers [`sees`]: what a file sees with
//! no import at all. Anything outside the project's modules is `Unresolved` —
//! keep-alive, never an accusation.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;

/// The rest of this file's package — what its names see without an import. A
/// production file sees its non-test siblings only; a test file sees the whole
/// package, test siblings included (internal test files share the package scope,
/// and external `_test`-package files over-keep in the same safe direction). The
/// asymmetry is the point: tests consume the package, the package never consumes
/// its tests, so the production color cannot leak through a test file.
pub fn sees(path: &ProjectPath, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
    let dir = parent_dir(path.as_str());
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let from_test = path.as_str().ends_with("_test.go");
    cx.files_with_prefix(&prefix)
        .filter(|p| {
            let rest = &p.as_str()[prefix.len()..];
            rest.ends_with(".go")
                && !rest.contains('/')
                && *p != path
                && (from_test || !rest.ends_with("_test.go"))
        })
        .cloned()
        .collect()
}

pub fn resolve(_from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
    // Longest declared module prefix wins: `example.com/mod/sub/pkg` tries the full
    // path, then each `/` boundary shorter, against the go.mod-declared names.
    let mut prefix_end = specifier.len();
    loop {
        let prefix = &specifier[..prefix_end];
        if let Some(pkg) = cx.package(prefix) {
            let rest = &specifier[prefix_end..];
            let dir = if rest.is_empty() {
                pkg.dir.to_string()
            } else {
                join(&pkg.dir, rest.trim_start_matches('/'))
            };
            return package_files(&dir, cx);
        }
        match specifier[..prefix_end].rfind('/') {
            Some(i) => prefix_end = i,
            None => return Resolution::Unresolved,
        }
    }
}

/// Every non-test `.go` directly in `dir` — the files that ARE the package as its
/// importers see it: importing a package never pulls its tests.
fn package_files(dir: &str, cx: &ResolveContext<'_>) -> Resolution {
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let files: Vec<ProjectPath> = cx
        .files_with_prefix(&prefix)
        .filter(|p| {
            let rest = &p.as_str()[prefix.len()..];
            !rest.contains('/') && rest.ends_with(".go") && !rest.ends_with("_test.go")
        })
        .cloned()
        .collect();
    if files.is_empty() {
        Resolution::Unresolved
    } else {
        Resolution::Files(files)
    }
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

fn join(dir: &str, rest: &str) -> String {
    if dir.is_empty() {
        rest.to_string()
    } else {
        format!("{dir}/{rest}")
    }
}
