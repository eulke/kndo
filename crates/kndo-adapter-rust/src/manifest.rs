//! What one `Cargo.toml` teaches the engine: every target cargo builds is a UNIT
//! — the lib, each bin, each test, bench and example, and the build script —
//! with the file cargo enters it through, the directory that file lives in, and
//! whether the registry ever sees it (`publish = false`). Declared paths and the
//! auto-discovered conventions alike are real cargo semantics, so both are
//! stated the same way. The `[package]` name becomes a workspace package other
//! crates import by path dependency, and the dependency tables become the
//! declarations the hygiene analyses read. Unparseable or dangling entries
//! degrade to absence: a unit whose entry does not exist anchors nothing.
//!
//! Cargo's targets share directories — `src/lib.rs` and `src/main.rs` are two
//! crates in one folder — so a directory cannot say which target compiles a
//! file. The entry can: the engine walks each module tree down from the file
//! its unit names, and every file the tree holds belongs to that unit.

use kndo_contract::adapter::{
    DependencyDeclaration, DependencyScope, PackageEntry, ResolveContext, SourceFile,
};
use kndo_contract::manifest::{ManifestSink, Publication, Unit, UnitKind};
use kndo_contract::vocab::ProjectPath;
use smol_str::SmolStr;

/// Everything one `Cargo.toml` states, in one read.
pub fn structure(manifest: &SourceFile<'_>, cx: &ResolveContext<'_>, out: &mut ManifestSink) {
    let Some(toml) = parse(manifest.content) else {
        return;
    };
    // A virtual workspace manifest declares no target and no package, and its
    // `[workspace.dependencies]` pool is still a declaration its members
    // inherit from.
    for declaration in dependencies(manifest) {
        out.dependency(declaration);
    }
    let Some(package_name) = toml
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
    else {
        return;
    };
    for package in packages(manifest, cx) {
        out.package(package);
    }
    let dir = parent_dir(manifest.path);
    // `publish = false` (or an empty allow-list) is cargo's own word for "no
    // registry sees this": the lib's exports are the project's business alone.
    let publication = match toml.get("package").and_then(|p| p.get("publish")) {
        Some(toml::Value::Boolean(false)) => Publication::Unpublished,
        Some(toml::Value::Array(a)) if a.is_empty() => Publication::Unpublished,
        _ => Publication::Unstated,
    };
    let mut declared: Vec<SmolStr> = dependencies(manifest).into_iter().map(|d| d.name).collect();
    declared.sort();
    declared.dedup();
    for (name, kind, entry) in targets(&toml, &dir, package_name, cx) {
        let mut depends_on = declared.clone();
        if kind != UnitKind::Library {
            // Every other target compiles against the crate's own library.
            depends_on.push(SmolStr::new(package_name));
        }
        out.unit(Unit {
            name,
            kind,
            roots: vec![SmolStr::new(parent_dir(&entry))],
            excludes: Vec::new(),
            entries: vec![entry],
            depends_on,
            // Cargo says nothing of the sort: an integration test is a
            // separate crate that sees only what the library exports.
            friend_of: Vec::new(),
            publication: if kind == UnitKind::Library {
                publication
            } else {
                Publication::Unstated
            },
        });
    }
}

/// Every target cargo builds from this manifest, as (unit name, kind, entry):
/// the declared ones and the ones cargo discovers by convention, each named
/// the way cargo names it and prefixed by its kind, so a bin and a test that
/// share a file stem stay two units.
fn targets(
    toml: &toml::Value,
    dir: &str,
    package_name: &str,
    cx: &ResolveContext<'_>,
) -> Vec<(SmolStr, UnitKind, ProjectPath)> {
    let mut out: Vec<(SmolStr, UnitKind, ProjectPath)> = Vec::new();
    let mut push = |name: String, kind: UnitKind, file: ProjectPath| {
        if cx.contains(&file) && !out.iter().any(|(_, _, e)| *e == file) {
            out.push((SmolStr::new(name), kind, file));
        }
    };

    if let Some(entry) = lib_entry(toml, dir) {
        push(package_name.to_string(), UnitKind::Library, entry);
    }
    push(
        format!("bin:{package_name}"),
        UnitKind::Executable,
        join(dir, "src/main.rs"),
    );
    push(
        "build".to_string(),
        UnitKind::Tooling,
        join(dir, "build.rs"),
    );
    if let Some(build) = toml
        .get("package")
        .and_then(|p| p.get("build"))
        .and_then(|b| b.as_str())
    {
        push("build".to_string(), UnitKind::Tooling, join(dir, build));
    }

    // Declared targets with explicit paths.
    for (section, kind) in [
        ("bin", UnitKind::Executable),
        ("test", UnitKind::Test),
        ("bench", UnitKind::Bench),
        ("example", UnitKind::Example),
    ] {
        for target in toml
            .get(section)
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(path) = target.get("path").and_then(|p| p.as_str()) {
                let file = join(dir, path);
                let name = target
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| stem(&file));
                push(format!("{section}:{name}"), kind, file);
            }
        }
    }

    // Auto-discovered target conventions: every single-file crate cargo builds.
    for (subdir, section, kind) in [
        ("src/bin", "bin", UnitKind::Executable),
        ("tests", "test", UnitKind::Test),
        ("benches", "bench", UnitKind::Bench),
        ("examples", "example", UnitKind::Example),
    ] {
        let prefix = format!("{}/", join(dir, subdir).as_str());
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
            let name = match file.as_str().strip_suffix("/main.rs") {
                Some(rest) => stem_of(rest),
                None => stem(&file),
            };
            push(format!("{section}:{name}"), kind, file);
        }
    }

    out
}

/// A file's name without its extension (`tests/it.rs` → `it`).
fn stem(file: &ProjectPath) -> String {
    stem_of(file.as_str().strip_suffix(".rs").unwrap_or(file.as_str()))
}

/// The last segment of a `/`-separated path.
fn stem_of(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// The package this manifest declares, entry-optional: a bin-only crate has no lib
/// to import, but its directory still tells `package_of` which crate a file belongs
/// to — `crate::` resolution needs that even where nothing imports the package.
fn packages(manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<PackageEntry> {
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

/// The dependency names this manifest declares — every dependency table Cargo
/// reads: the three top-level sections, `[workspace.dependencies]`, and the same
/// sections under each `[target.…]`. Names are the table keys (what the project's
/// code refers to). Activation evidence for plugin `ManifestDependency` rules.
fn dependencies(manifest: &SourceFile<'_>) -> Vec<DependencyDeclaration> {
    const SECTIONS: [(&str, DependencyScope); 3] = [
        ("dependencies", DependencyScope::Prod),
        ("dev-dependencies", DependencyScope::Dev),
        ("build-dependencies", DependencyScope::Build),
    ];
    let Some(toml) = parse(manifest.content) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut collect = |table: Option<&toml::Value>, scope: Option<DependencyScope>| {
        if let Some(toml::Value::Table(map)) = table {
            for (name, spec) in map {
                // `optional = true` is a feature gate: the dependency is in
                // the build only when a feature asks — never a usage claim.
                let optional = spec
                    .get("optional")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false);
                out.push(DependencyDeclaration {
                    name: SmolStr::new(name),
                    scope: if optional {
                        Some(DependencyScope::Optional)
                    } else {
                        scope
                    },
                    version_req: comparable_req(spec),
                });
            }
        }
    };
    for (section, scope) in SECTIONS {
        collect(toml.get(section), Some(scope));
    }
    // The workspace pool: a real declaring site version-skew compares against a
    // member's own requirement, but not a usage scope — members opt in per name.
    collect(
        toml.get("workspace").and_then(|w| w.get("dependencies")),
        None,
    );
    if let Some(toml::Value::Table(targets)) = toml.get("target") {
        for target in targets.values() {
            for (section, scope) in SECTIONS {
                collect(target.get(section), Some(scope));
            }
        }
    }
    out
}

/// The requirement one dependency spec states, when it states one at all: the
/// bare-string form, or a table's `version` key. A path/git/workspace-inherited
/// spec names a resolution mechanism, not a version — a comparison the manifest
/// does not enable must stay silent instead of diverging from every real one.
fn comparable_req(spec: &toml::Value) -> Option<SmolStr> {
    match spec {
        toml::Value::String(req) => Some(SmolStr::new(req)),
        toml::Value::Table(t) => t.get("version").and_then(|v| v.as_str()).map(SmolStr::new),
        _ => None,
    }
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
