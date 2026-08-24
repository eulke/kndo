//! Cargo.toml extraction. Parsed with the `toml` crate — the
//! format is full TOML, not a hand-parseable line format like go.mod.
//!
//! Root rules of record: bins (`src/main.rs`, autobins in `src/bin/`, `[[bin]] path`) are
//! production roots unconditionally — an entry point is an entry point; the lib entry is a
//! production root **only when the package is publishable** (library mode —
//! an unpublished crate's `pub` API must earn its keep through actual imports); `build.rs`
//! is a tooling root. `declares_surface` stays `false` by design: Cargo has no exports map,
//! a crate's surface IS its `pub` items, and `deep-import`'s gate is closed deliberately.

use kndo_core::adapter::{
    Diagnostic, DiagnosticLevel, ExecutableTarget, ManifestDependency, ManifestFacts, ManifestRoot,
    ProjectPath, ResolveCtx,
};
use kndo_core::vocab::{Confidence, DependencyScope, RootKind};
use smol_str::SmolStr;

pub(crate) fn extract(path: &str, content: &[u8], ctx: &ResolveCtx<'_>) -> ManifestFacts {
    let mut out = ManifestFacts::default();
    let Ok(text) = std::str::from_utf8(content) else {
        out.diagnostics.push(diag("Cargo.toml is not valid UTF-8"));
        return out;
    };
    let value: toml::Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            out.diagnostics
                .push(diag(&format!("Cargo.toml parse error: {e}")));
            return out;
        }
    };

    let dir = kndo_adapter_toolkit::paths::dirname(path);
    let join = |rel: &str| -> ProjectPath {
        if dir.is_empty() {
            ProjectPath(SmolStr::new(rel))
        } else {
            ProjectPath(SmolStr::new(format!("{dir}/{rel}")))
        }
    };

    // Identity. A pure [workspace] manifest (no [package]) contributes topology only.
    let package = value.get("package");
    out.package_name = package
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(SmolStr::new);
    // `publish = false` → app mode. (`publish = ["registry"]` restrictions stay publishable.)
    out.private = package
        .and_then(|p| p.get("publish"))
        .and_then(|v| v.as_bool())
        .is_some_and(|publish| !publish);

    // Workspace topology (globs; exclude is honored by NOT expanding here — the core expands
    // members, and cargo's exclude only prunes glob expansion).
    if let Some(members) = value
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
    {
        out.workspace_members = members
            .iter()
            .filter_map(|m| m.as_str())
            .map(SmolStr::new)
            .collect();
    }

    // `[workspace.dependencies]` — the shared version pool `{ workspace = true }` deps in
    // member manifests point at. Captured here (root/virtual-workspace manifest only, in
    // practice) so graph assembly can resolve `inherited` deps against a real version before
    // any analysis (version-skew, in particular) ever sees them.
    if let Some(table) = value
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(|d| d.as_table())
    {
        out.workspace_dependencies = table
            .iter()
            .map(|(key, spec)| {
                let (version_req, _) = version_req_of(spec);
                ManifestDependency {
                    name: SmolStr::new(key.as_str()),
                    version_req,
                    scope: DependencyScope::Prod, // unused for resolution; the table has no scope concept
                    inherited: false,             // this entry IS the pool, not a heir of it
                }
            })
            .collect();
    }

    // Dependencies, three scopes + target-conditional tables flattened.
    collect_deps(value.get("dependencies"), DependencyScope::Prod, &mut out);
    collect_deps(
        value.get("dev-dependencies"),
        DependencyScope::Dev,
        &mut out,
    );
    collect_deps(
        value.get("build-dependencies"),
        DependencyScope::Build,
        &mut out,
    );
    if let Some(targets) = value.get("target").and_then(|t| t.as_table()) {
        for cfg in targets.values() {
            collect_deps(cfg.get("dependencies"), DependencyScope::Prod, &mut out);
            collect_deps(cfg.get("dev-dependencies"), DependencyScope::Dev, &mut out);
            collect_deps(
                cfg.get("build-dependencies"),
                DependencyScope::Build,
                &mut out,
            );
        }
    }

    // Only packages have targets; a virtual workspace manifest stops here.
    if package.is_none() {
        return out;
    }

    // --- bins: production roots unconditionally *as a claim* — "this is an entry point"
    // is the language fact this adapter states; assembly caps the root's KIND by the
    // file's role (a bin under a tooling dir like xtask/ becomes a Tooling root,
    // graph phase 2.58). Each lands under its cargo-assigned name — the identity
    // `env!("CARGO_BIN_EXE_<name>")` invokes it by (the invoked-program rule) ---
    let mut bins: Vec<(Option<SmolStr>, ProjectPath)> = Vec::new();
    let main_rs = join("src/main.rs");
    if ctx.contains(&main_rs) {
        // Cargo names the default bin after the package itself.
        bins.push((out.package_name.clone(), main_rs));
    }
    // Autobins: every .rs directly under src/bin/, named by file stem (sorted —
    // deterministic).
    let bin_dir = if dir.is_empty() {
        "src/bin".to_string()
    } else {
        format!("{dir}/src/bin")
    };
    let mut autobins: Vec<ProjectPath> = ctx
        .files_in_dir(&bin_dir)
        .filter(|p| p.0.ends_with(".rs"))
        .cloned()
        .collect();
    autobins.sort();
    bins.extend(autobins.into_iter().map(|p| (bin_stem(&p), p)));
    // Explicit [[bin]] entries with a path (name is cargo-required; a pathless entry names
    // an autobin already collected above).
    if let Some(entries) = value.get("bin").and_then(|b| b.as_array()) {
        for bin in entries {
            if let Some(p) = bin.get("path").and_then(|p| p.as_str()) {
                let target = join(p);
                if ctx.contains(&target) {
                    let name = bin.get("name").and_then(|n| n.as_str()).map(SmolStr::new);
                    bins.push((name, target));
                }
            }
        }
    }
    for (name, target) in bins {
        if let Some(name) = name {
            out.executables.push(ExecutableTarget {
                name,
                entry: target.clone(),
            });
        }
        out.entry_points.push(target.0.clone());
        out.roots.push(ManifestRoot {
            kind: RootKind::Production,
            target,
            confidence: Confidence::Certain,
        });
    }

    // --- lib: the import entry always; a production root only in library mode ---
    let lib_path = value
        .get("lib")
        .and_then(|l| l.get("path"))
        .and_then(|p| p.as_str())
        .map(&join)
        .unwrap_or_else(|| join("src/lib.rs"));
    if ctx.contains(&lib_path) {
        out.entry_points.push(lib_path.0.clone());
        out.resolved_entries
            .push((lib_path.clone(), Confidence::Certain));
        if !out.private {
            out.roots.push(ManifestRoot {
                kind: RootKind::Production,
                target: lib_path,
                confidence: Confidence::Certain,
            });
        }
    }

    // --- build.rs: tooling root (invoked by cargo, not imported) ---
    let build_rs = value
        .get("package")
        .and_then(|p| p.get("build"))
        .and_then(|b| b.as_str())
        .map(&join)
        .unwrap_or_else(|| join("build.rs"));
    if ctx.contains(&build_rs) {
        out.roots.push(ManifestRoot {
            kind: RootKind::Tooling,
            target: build_rs,
            confidence: Confidence::Certain,
        });
    }

    out
}

fn collect_deps(table: Option<&toml::Value>, scope: DependencyScope, out: &mut ManifestFacts) {
    let Some(table) = table.and_then(|t| t.as_table()) else {
        return;
    };
    for (key, spec) in table {
        // `foo = { package = "real-name" }` renames: the KEY is what source code writes
        // (`use foo::…`), so the key is the declared identity kndo matches against.
        let (version_req, inherited) = version_req_of(spec);
        out.dependencies.push(ManifestDependency {
            name: SmolStr::new(key.as_str()),
            version_req,
            scope,
            inherited,
        });
    }
}

/// A single `toml::Value` dependency spec's version requirement, and whether it's inherited
/// from `[workspace.dependencies]` rather than a literal of its own.
fn version_req_of(spec: &toml::Value) -> (SmolStr, bool) {
    match spec {
        toml::Value::String(v) => (SmolStr::new(v.as_str()), false),
        toml::Value::Table(t) => {
            if t.get("workspace").and_then(|w| w.as_bool()) == Some(true) {
                // Workspace-inherited: a readable placeholder, not a real value to compare —
                // graph assembly resolves it against ManifestFacts::workspace_dependencies.
                (SmolStr::new("workspace"), true)
            } else {
                let version_req = t
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(SmolStr::new)
                    // Path/git deps without a version: any.
                    .unwrap_or_else(|| SmolStr::new("*"));
                (version_req, false)
            }
        }
        _ => (SmolStr::new("*"), false),
    }
}

/// An autobin's cargo-assigned name: the file stem (`src/bin/foo.rs` → `foo`).
fn bin_stem(path: &ProjectPath) -> Option<SmolStr> {
    path.0
        .rsplit('/')
        .next()
        .and_then(|f| f.strip_suffix(".rs"))
        .map(SmolStr::new)
}

fn diag(message: &str) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Warn,
        path: None,
        message: message.to_string(),
        span: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet;

    fn ctx_with(files: &[&str]) -> (FxHashSet<ProjectPath>, ()) {
        (
            files
                .iter()
                .map(|f| ProjectPath(SmolStr::new(*f)))
                .collect(),
            (),
        )
    }

    fn facts(manifest_path: &str, content: &str, files: &[&str]) -> ManifestFacts {
        let (known, _) = ctx_with(files);
        let ctx = ResolveCtx::new(&known);
        extract(manifest_path, content.as_bytes(), &ctx)
    }

    #[test]
    fn identity_scopes_and_rename_key() {
        let f = facts(
            "Cargo.toml",
            r#"
[package]
name = "demo"
publish = false

[dependencies]
serde = { version = "1", features = ["derive"] }
renamed = { package = "actual-crate", version = "2" }

[dev-dependencies]
insta = "1"

[build-dependencies]
cc = "1"
"#,
            &["src/lib.rs"],
        );
        assert_eq!(f.package_name.as_deref(), Some("demo"));
        assert!(f.private);
        let dep = |n: &str| f.dependencies.iter().find(|d| d.name == n).unwrap();
        assert_eq!(dep("serde").scope, DependencyScope::Prod);
        assert_eq!(dep("serde").version_req.as_str(), "1");
        assert_eq!(dep("renamed").version_req.as_str(), "2"); // key, not package field
        assert_eq!(dep("insta").scope, DependencyScope::Dev);
        assert_eq!(dep("cc").scope, DependencyScope::Build);
    }

    #[test]
    fn bins_root_unconditionally_lib_only_when_publishable() {
        let f = facts(
            "Cargo.toml",
            "[package]\nname = \"app\"\npublish = false\n",
            &["src/main.rs", "src/lib.rs", "src/bin/extra.rs"],
        );
        let production: Vec<&str> = f
            .roots
            .iter()
            .filter(|r| r.kind == RootKind::Production)
            .map(|r| r.target.0.as_str())
            .collect();
        assert!(production.contains(&"src/main.rs"));
        assert!(production.contains(&"src/bin/extra.rs"));
        assert!(
            !production.contains(&"src/lib.rs"),
            "unpublished lib API must earn its keep through imports"
        );
        // …but the lib is still the sibling-import entry.
        assert_eq!(f.resolved_entries[0].0 .0.as_str(), "src/lib.rs");

        let published = facts("Cargo.toml", "[package]\nname = \"lib\"\n", &["src/lib.rs"]);
        assert!(published
            .roots
            .iter()
            .any(|r| r.kind == RootKind::Production && r.target.0 == "src/lib.rs"));
    }

    #[test]
    fn build_rs_is_a_tooling_root_and_subdir_manifests_join_paths() {
        let f = facts(
            "crates/x/Cargo.toml",
            "[package]\nname = \"x\"\n",
            &["crates/x/src/lib.rs", "crates/x/build.rs"],
        );
        assert!(f
            .roots
            .iter()
            .any(|r| r.kind == RootKind::Tooling && r.target.0 == "crates/x/build.rs"));
        assert!(f.roots.iter().any(|r| r.target.0 == "crates/x/src/lib.rs"));
    }

    #[test]
    fn virtual_workspace_contributes_topology_only() {
        let f = facts(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\", \"xtask\"]\n",
            &[],
        );
        assert!(f.package_name.is_none());
        assert_eq!(f.workspace_members, vec!["crates/*", "xtask"]);
        assert!(f.roots.is_empty());
    }

    #[test]
    fn bins_carry_their_cargo_assigned_names() {
        let f = facts(
            "crates/x/Cargo.toml",
            "[package]\nname = \"app\"\n\n[[bin]]\nname = \"custom\"\npath = \"tools/custom.rs\"\n",
            &[
                "crates/x/src/main.rs",
                "crates/x/src/bin/extra.rs",
                "crates/x/tools/custom.rs",
            ],
        );
        let exe = |n: &str| {
            f.executables
                .iter()
                .find(|e| e.name == n)
                .map(|e| e.entry.0.as_str())
        };
        assert_eq!(
            exe("app"),
            Some("crates/x/src/main.rs"),
            "default bin = package name"
        );
        assert_eq!(
            exe("extra"),
            Some("crates/x/src/bin/extra.rs"),
            "autobin = file stem"
        );
        assert_eq!(
            exe("custom"),
            Some("crates/x/tools/custom.rs"),
            "[[bin]] = declared name"
        );
    }

    #[test]
    fn target_conditional_dependencies_flatten() {
        let f = facts(
            "Cargo.toml",
            "[package]\nname = \"t\"\n[target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n",
            &[],
        );
        assert!(f
            .dependencies
            .iter()
            .any(|d| d.name == "libc" && d.scope == DependencyScope::Prod));
    }

    #[test]
    fn workspace_dependencies_are_captured_and_inherited_deps_are_flagged() {
        let f = facts(
            "Cargo.toml",
            r#"
[workspace]
members = ["crates/*"]

[workspace.dependencies]
foo = "1.2"
bar = { version = "3", features = ["derive"] }
"#,
            &[],
        );
        let pooled = |n: &str| f.workspace_dependencies.iter().find(|d| d.name == n);
        assert_eq!(pooled("foo").unwrap().version_req.as_str(), "1.2");
        assert!(!pooled("foo").unwrap().inherited);
        assert_eq!(pooled("bar").unwrap().version_req.as_str(), "3");

        let member = facts(
            "crates/x/Cargo.toml",
            "[package]\nname = \"x\"\n[dependencies]\nfoo = { workspace = true }\n",
            &[],
        );
        let dep = member
            .dependencies
            .iter()
            .find(|d| d.name == "foo")
            .unwrap();
        assert!(dep.inherited);
        assert_eq!(dep.version_req.as_str(), "workspace"); // placeholder, resolved during assembly
    }
}
