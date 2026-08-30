//! What one `go.mod` teaches the engine: the module path becomes an entry-less
//! package — a Go module maps import prefixes to directories, there is no file a
//! bare import resolves to — and the directory is what subpath imports resolve
//! against. Targets need no manifest here: `package main` + `func main` and
//! `_test.go` are extraction's to see.

use kndo_contract::adapter::{PackageEntry, ResolveContext, SourceFile};
use smol_str::SmolStr;

pub fn packages(manifest: &SourceFile<'_>, _cx: &ResolveContext<'_>) -> Vec<PackageEntry> {
    let Ok(text) = std::str::from_utf8(manifest.content) else {
        return Vec::new();
    };
    let Some(module) = text.lines().find_map(|line| {
        let line = line.trim();
        line.strip_prefix("module")
            .filter(|r| r.starts_with([' ', '\t']))
            .map(|r| r.trim().trim_matches('"').to_string())
    }) else {
        return Vec::new();
    };
    if module.is_empty() {
        return Vec::new();
    }
    let dir = match manifest.path.as_str().rfind('/') {
        Some(i) => &manifest.path.as_str()[..i],
        None => "",
    };
    vec![PackageEntry {
        name: SmolStr::new(module),
        entry: None,
        dir: SmolStr::new(dir),
    }]
}

/// The module paths this `go.mod` requires — single-line and block form alike,
/// `// indirect` included (an indirect dependency is still in the build).
/// Activation evidence for plugin `ManifestDependency` rules.
pub fn dependencies(manifest: &SourceFile<'_>) -> Vec<SmolStr> {
    let Ok(text) = std::str::from_utf8(manifest.content) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut in_block = false;
    for line in text.lines() {
        let line = line.trim();
        if in_block {
            if line.starts_with(')') {
                in_block = false;
            } else if let Some(module) = line.split_whitespace().next()
                && !module.starts_with("//")
            {
                out.push(SmolStr::new(module.trim_matches('"')));
            }
            continue;
        }
        let Some(rest) = line.strip_prefix("require") else {
            continue;
        };
        if !rest.starts_with([' ', '\t', '(']) {
            continue;
        }
        let rest = rest.trim_start();
        if rest.starts_with('(') {
            in_block = true;
        } else if let Some(module) = rest.split_whitespace().next() {
            out.push(SmolStr::new(module.trim_matches('"')));
        }
    }
    out
}
