//! What one `Cargo.toml` teaches the engine: the targets cargo builds become roots
//! (declared `[[bin]]`/`[lib]` paths and the auto-discovered conventions alike —
//! both are real cargo semantics, so both are `Certain`), and the `[package]` name
//! becomes a workspace package other crates can import by path dependency.
//! Unparseable or dangling entries degrade to absence: a root that anchors nothing
//! accuses nothing.

use kndo_contract::adapter::{PackageEntry, ProjectRoot, ResolveContext, SourceFile};
use kndo_contract::evidence::RootKind;
use kndo_contract::vocab::{Confidence, ProjectPath};
use smol_str::SmolStr;

pub fn roots(manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<ProjectRoot> {
    let Some(toml) = parse(manifest.content) else {
        return Vec::new();
    };
    if toml.get("package").is_none() {
        // A virtual workspace manifest: its members carry their own.
        return Vec::new();
    }
    let dir = parent_dir(manifest.path);
    let mut out = Vec::new();
    let mut push = |file: ProjectPath, kind: RootKind| {
        if cx.contains(&file) && !out.iter().any(|r: &ProjectRoot| r.file == file) {
            out.push(ProjectRoot {
                file,
                kind,
                confidence: Confidence::Certain,
            });
        }
    };

    if let Some(entry) = lib_entry(&toml, &dir) {
        push(entry, RootKind::Production);
    }
    push(join(&dir, "src/main.rs"), RootKind::Production);
    push(join(&dir, "build.rs"), RootKind::Tooling);
    if let Some(build) = toml
        .get("package")
        .and_then(|p| p.get("build"))
        .and_then(|b| b.as_str())
    {
        push(join(&dir, build), RootKind::Tooling);
    }

    // Declared targets with explicit paths.
    for (section, kind) in [
        ("bin", RootKind::Production),
        ("test", RootKind::Test),
        ("bench", RootKind::Test),
        ("example", RootKind::Tooling),
    ] {
        for target in toml
            .get(section)
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(path) = target.get("path").and_then(|p| p.as_str()) {
                push(join(&dir, path), kind);
            }
        }
    }

    // Auto-discovered target conventions: every single-file crate cargo would build.
    for (subdir, kind) in [
        ("src/bin", RootKind::Production),
        ("tests", RootKind::Test),
        ("benches", RootKind::Test),
        ("examples", RootKind::Tooling),
    ] {
        let prefix = format!("{}/", join(&dir, subdir).as_str());
        let discovered: Vec<ProjectPath> = cx
            .files_with_prefix(&prefix)
            .filter(|p| {
                let rest = &p.as_str()[prefix.len()..];
                let direct_file = !rest.contains('/') && rest.ends_with(".rs");
                let dir_main = rest.ends_with("/main.rs") && rest.matches('/').count() == 1;
                direct_file || dir_main
            })
            .cloned()
            .collect();
        for file in discovered {
            push(file, kind);
        }
    }

    out
}

/// The package this manifest declares, entry-optional: a bin-only crate has no lib
/// to import, but its directory still tells `package_of` which crate a file belongs
/// to — `crate::` resolution needs that even where nothing imports the package.
pub fn packages(manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<PackageEntry> {
    let Some(toml) = parse(manifest.content) else {
        return Vec::new();
    };
    let Some(package_name) = toml
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
    else {
        return Vec::new();
    };
    let dir = parent_dir(manifest.path);
    // `[lib] name` overrides the import name; either way the ecosystem imports the
    // underscored form.
    let import_name = toml
        .get("lib")
        .and_then(|l| l.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or(package_name)
        .replace('-', "_");
    let entry = lib_entry(&toml, &dir).filter(|e| cx.contains(e));
    vec![PackageEntry {
        name: SmolStr::new(import_name),
        entry,
        dir: SmolStr::new(dir),
    }]
}

fn lib_entry(toml: &toml::Value, dir: &str) -> Option<ProjectPath> {
    let declared = toml
        .get("lib")
        .and_then(|l| l.get("path"))
        .and_then(|p| p.as_str());
    Some(match declared {
        Some(path) => join(dir, path),
        None => join(dir, "src/lib.rs"),
    })
}

fn parse(content: &[u8]) -> Option<toml::Value> {
    std::str::from_utf8(content)
        .ok()?
        .parse::<toml::Value>()
        .ok()
}

fn parent_dir(path: &ProjectPath) -> String {
    match path.as_str().rfind('/') {
        Some(i) => path.as_str()[..i].to_string(),
        None => String::new(),
    }
}

fn join(dir: &str, rest: &str) -> ProjectPath {
    if dir.is_empty() {
        ProjectPath::new(rest)
    } else {
        ProjectPath::new(format!("{dir}/{rest}"))
    }
}
