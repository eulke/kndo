//! Import resolution, package-shaped: an import path maps to a DIRECTORY through
//! the longest module-path prefix a `go.mod` declares, and the resolution is every
//! non-test `.go` file in it — [`Resolution::Files`], the engine drawing one edge
//! per file. The synthetic `"."` specifier is this file's own package. Anything
//! outside the project's modules is `Unresolved` — keep-alive, never an accusation.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;

pub fn resolve(from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
    if specifier == "." {
        return package_files(&parent_dir(from.as_str()), cx, Some(from));
    }
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
            return package_files(&dir, cx, None);
        }
        match specifier[..prefix_end].rfind('/') {
            Some(i) => prefix_end = i,
            None => return Resolution::Unresolved,
        }
    }
}

/// Every non-test `.go` directly in `dir` — the files that ARE the package.
/// `_test.go` siblings stay out: importing a package never pulls its tests, and
/// the synthetic self-edge must not paint them production-reachable.
fn package_files(dir: &str, cx: &ResolveContext<'_>, exclude: Option<&ProjectPath>) -> Resolution {
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let files: Vec<ProjectPath> = cx
        .files_with_prefix(&prefix)
        .filter(|p| {
            let rest = &p.as_str()[prefix.len()..];
            !rest.contains('/')
                && rest.ends_with(".go")
                && !rest.ends_with("_test.go")
                && Some(*p) != exclude
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
