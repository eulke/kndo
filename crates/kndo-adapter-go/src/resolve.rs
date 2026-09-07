//! Import resolution, package-shaped: an import path maps to a DIRECTORY through
//! the longest module-path prefix a `go.mod` declares, and the resolution is every
//! non-test `.go` file in it — [`Resolution::Files`], the engine drawing one edge
//! per file. Which files co-compile is NOT here: that is the namespace node's,
//! read off the scope forest. Anything outside the project's modules is
//! `Unresolved` — keep-alive, never an accusation.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;
use kndo_toolkit as tk;

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
            let prefix = if dir.is_empty() {
                String::new()
            } else {
                format!("{dir}/")
            };
            let files: Vec<ProjectPath> = cx
                .files_with_prefix(&prefix)
                .filter(|p| {
                    let rest = &p.as_str()[prefix.len()..];
                    rest.ends_with(".go") && !rest.contains('/') && !rest.ends_with("_test.go")
                })
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
