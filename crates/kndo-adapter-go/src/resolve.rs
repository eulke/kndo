//! Import resolution, package-shaped: an import path maps to a DIRECTORY through
//! the longest module-path prefix a `go.mod` declares, and the resolution is every
//! non-test `.go` file in it — [`Resolution::Files`], the engine drawing one edge
//! per file. The same directory fact answers [`sees`]: what a file sees with
//! no import at all. Anything outside the project's modules is `Unresolved` —
//! keep-alive, never an accusation.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;
use kndo_toolkit as tk;

/// Whether the caller wants the package as its importers compile it or as its
/// own test binary does — the only axis on which "the `.go` files of one
/// directory" has two answers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tests {
    Included,
    Excluded,
}

/// The `.go` files directly in `dir`, in path order.
fn dir_files<'a>(dir: &str, tests: Tests, cx: &'a ResolveContext<'_>) -> Vec<&'a ProjectPath> {
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    cx.files_with_prefix(&prefix)
        .filter(|p| {
            let rest = &p.as_str()[prefix.len()..];
            rest.ends_with(".go")
                && !rest.contains('/')
                && (tests == Tests::Included || !rest.ends_with("_test.go"))
        })
        .collect()
}

/// The rest of this file's package — what its names see without an import. A
/// production file sees its non-test siblings only; a test file sees the whole
/// package, test siblings included (internal test files share the package scope,
/// and external `_test`-package files over-keep in the same safe direction). The
/// asymmetry is the point: tests consume the package, the package never consumes
/// its tests, so the production color cannot leak through a test file.
pub fn sees(path: &ProjectPath, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
    let tests = if path.as_str().ends_with("_test.go") {
        Tests::Included
    } else {
        Tests::Excluded
    };
    dir_files(tk::parent_dir(path.as_str()), tests, cx)
        .into_iter()
        .filter(|p| *p != path)
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
            let Some(dir) = tk::join_relative(&pkg.dir, rest.trim_start_matches('/')) else {
                return Resolution::Unresolved;
            };
            // Every non-test `.go` directly in the directory — the files that
            // ARE the package as its importers see it: importing a package
            // never pulls its tests.
            let files: Vec<ProjectPath> = dir_files(&dir, Tests::Excluded, cx)
                .into_iter()
                .cloned()
                .collect();
            return if files.is_empty() {
                Resolution::Unresolved
            } else {
                Resolution::Files(files)
            };
        }
        match specifier[..prefix_end].rfind('/') {
            Some(i) => prefix_end = i,
            None => return Resolution::Unresolved,
        }
    }
}
