//! The author kit's scaffold and build halves (RFC 0017 §7, second pass): `kndo plugin new`
//! writes a compilable component crate with the ABI already vendored, `kndo plugin build`
//! turns it into a componentized `.wasm` with no wasm tooling knowledge required, and
//! `kndo plugin wit` prints the WIT world this binary was built against. Together with
//! `kndo plugin verify` (verify.rs) the idea→working-component loop is four commands, none
//! of which require reading kndo's repository.
//!
//! The templates are held to the same bar as the examples: the e2e author-kit test scaffolds
//! both kinds, builds them with a real cargo invocation, componentizes through [`build`], and
//! loads the result through the discovery loaders — a template that stops compiling breaks
//! this workspace's own tests, not a third party's afternoon.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Which WIT world to scaffold or print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentKind {
    Plugin,
    Adapter,
}

impl ComponentKind {
    /// The vendored WIT text for this kind — byte-identical to what this binary's host
    /// bridge was compiled against.
    pub fn wit(self) -> &'static str {
        match self {
            ComponentKind::Plugin => kndo_plugin_api::PLUGIN_WIT,
            ComponentKind::Adapter => kndo_plugin_api::ADAPTER_WIT,
        }
    }

    fn wit_file(self) -> &'static str {
        match self {
            ComponentKind::Plugin => "plugin.wit",
            ComponentKind::Adapter => "adapter.wit",
        }
    }

    fn lib_template(self) -> &'static str {
        match self {
            ComponentKind::Plugin => PLUGIN_LIB_TEMPLATE,
            ComponentKind::Adapter => ADAPTER_LIB_TEMPLATE,
        }
    }
}

/// Scaffold a new component crate at `dir` (created; must not already contain files). The
/// crate name is `dir`'s final path segment. Returns the created files, project-relative.
pub fn scaffold(dir: &Path, kind: ComponentKind) -> Result<Vec<String>, String> {
    let name = crate_name(dir)?;
    ensure_fresh_dir(dir)?;
    let mut created = Vec::new();
    for (rel, content) in scaffold_files(kind, &name) {
        std::fs::write(dir.join(&rel), content).map_err(|e| format!("writing {rel}: {e}"))?;
        created.push(rel);
    }
    Ok(created)
}

/// Refuse to scaffold over anything that already exists; create the crate's subdirectories.
fn ensure_fresh_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() && std::fs::read_dir(dir).map_err(|e| e.to_string())?.count() > 0 {
        return Err(format!(
            "{} already exists and is not empty — scaffold into a fresh directory",
            dir.display()
        ));
    }
    std::fs::create_dir_all(dir.join("src")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("wit")).map_err(|e| e.to_string())
}

/// Every file the scaffold writes, as `(relative path, content)`. The WIT is vendored from
/// this binary — not copied by hand from a possibly mismatched git checkout (authoring.md
/// §3's instruction, now automated).
fn scaffold_files(kind: ComponentKind, name: &str) -> Vec<(String, String)> {
    let wit_rel = format!("wit/{}", kind.wit_file());
    vec![
        (
            "Cargo.toml".to_string(),
            CARGO_TOML_TEMPLATE.replace("__NAME__", name),
        ),
        (
            "src/lib.rs".to_string(),
            kind.lib_template()
                .replace("__NAME__", name)
                .replace("__WIT_FILE__", kind.wit_file()),
        ),
        (wit_rel, kind.wit().to_string()),
        (".gitignore".to_string(), "/target\n".to_string()),
        (
            "README.md".to_string(),
            README_TEMPLATE.replace("__NAME__", name),
        ),
    ]
}

/// Build the component crate at `crate_dir` and componentize the result: one cargo
/// invocation (`--release --target wasm32-unknown-unknown`) plus the same
/// `wit_component::ComponentEncoder` call kndo's own compliance suites make. Writes
/// `<crate-name>.wasm` (the release-asset shape authoring.md §9 distributes) into
/// `crate_dir` and returns its path.
pub fn build(crate_dir: &Path) -> Result<PathBuf, String> {
    let name = package_name(crate_dir)?;
    run_cargo_build(crate_dir)?;
    let component = componentize(crate_dir, &name)?;
    let out = crate_dir.join(format!("{name}.wasm"));
    std::fs::write(&out, component).map_err(|e| format!("writing {}: {e}", out.display()))?;
    Ok(out)
}

/// Read cargo's core module output and encode it as a component.
fn componentize(crate_dir: &Path, name: &str) -> Result<Vec<u8>, String> {
    let artifact = wasm_target_dir(crate_dir)
        .join("wasm32-unknown-unknown/release")
        .join(format!("{}.wasm", name.replace('-', "_")));
    let core_wasm = std::fs::read(&artifact)
        .map_err(|e| format!("reading the built module {}: {e}", artifact.display()))?;
    wit_component::ComponentEncoder::default()
        .module(&core_wasm)
        .map_err(|e| format!("attaching the module to the component encoder: {e}"))?
        .encode()
        .map_err(|e| format!("encoding the component: {e}"))
}

fn crate_name(dir: &Path) -> Result<String, String> {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{} has no usable final path segment", dir.display()))?;
    if !is_valid_crate_name(name) {
        return Err(format!(
            "`{name}` is not a valid crate name (ascii letters, digits, `-`, `_`)"
        ));
    }
    Ok(name.to_string())
}

fn is_valid_crate_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The `package.name` from the crate's own manifest — the one fact `build` needs from it.
fn package_name(crate_dir: &Path) -> Result<String, String> {
    let manifest_path = crate_dir.join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("reading {}: {e}", manifest_path.display()))?;
    let table: toml::Table = manifest
        .parse()
        .map_err(|e| format!("parsing {}: {e}", manifest_path.display()))?;
    table
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("{} has no [package].name", manifest_path.display()))
}

fn run_cargo_build(crate_dir: &Path) -> Result<(), String> {
    let status = Command::new("cargo")
        .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
        .current_dir(crate_dir)
        .status()
        .map_err(|e| format!("invoking cargo: {e}"))?;
    if !status.success() {
        return Err("cargo build failed (is the wasm target installed? \
             `rustup target add wasm32-unknown-unknown`)"
            .to_string());
    }
    Ok(())
}

/// Where cargo put the artifact — the crate's own `target/` unless the caller's environment
/// redirects it (`CARGO_TARGET_DIR` is how kndo's own tests isolate concurrent builds).
fn wasm_target_dir(crate_dir: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate_dir.join("target"))
}

const CARGO_TOML_TEMPLATE: &str = r#"[package]
name = "__NAME__"
version = "0.1.0"
edition = "2021"
publish = false           # ships as a .wasm release asset, not a crate

# Standalone even when scaffolded inside another repository — a parent cargo workspace must
# not adopt (and then fail over) this wasm-only crate.
[workspace]

[lib]
crate-type = ["cdylib"]   # required: a linkable wasm module, not an rlib

[dependencies]
wit-bindgen = "0.57"

[profile.release]
opt-level = "s"           # size over speed — the binary is a distribution artifact
lto = true
"#;

const PLUGIN_LIB_TEMPLATE: &str = r#"//! __NAME__ — a kndo plugin (kndo:plugin ABI).
//!
//! Authoring guide: https://github.com/eulke/kndo — docs/plugins/authoring.md.
//! Inner loop: `kndo plugin build` then `kndo plugin verify __NAME__.wasm`.

// Marks the dependency used — the macro below is a fully-qualified path with no `use`.
use wit_bindgen as _;

wit_bindgen::generate!({
    // The ABI contract, vendored by `kndo plugin new` from the kndo you ran. To retarget a
    // newer kndo: `kndo plugin wit plugin > wit/plugin.wit` and rebuild. Do not edit it.
    path: "wit/__WIT_FILE__",
    // The findings-capable world (RFC 0018) — the full surface. Target `plugin` instead if
    // you only mutate the graph and want the smallest possible export set.
    world: "plugin-findings",
});

use crate::kndo::plugin::types::*;

struct Component;

impl Guest for Component {
    fn descriptor() -> PluginDescriptor {
        PluginDescriptor {
            // Your id IS your coordinate (authoring guide §4): the GitHub repo this plugin
            // can be fetched from. A plain name works for hand-dropped .kndo/plugins/ files
            // but can never be installed or depended on.
            id: "github.com/you/__NAME__".to_string(),
            version: "0.1.0".to_string(),
            detection: vec!["TODO: one line on when this plugin applies".to_string()],
            // Files outside the language graph you need to read (configs, templates) —
            // globs, served through the host's content channel. Empty = no reads.
            requested_file_access: Vec::new(),
            // When a globally installed copy of this plugin turns on (guide §5). An empty
            // list NEVER self-activates globally — declare a real rule.
            activation: vec![ActivationRule::ManifestDependency(
                "TODO-your-framework-package".to_string(),
            )],
            dependencies: Vec::new(),
        }
    }

    /// Correct a file's classification (role/origin). `None` = no opinion (the common case).
    fn classify_file(_path: String, _current: FileClass) -> Option<FileClass> {
        None
    }

    /// Framework entry points invisible to import analysis: routes, DI beans, handlers.
    /// Query the graph via the host imports (list-files, symbols-in, importers-of,
    /// call-sites-in, packages, …) and return targets to root.
    fn contribute_roots() -> Vec<ContributedRoot> {
        let mut roots = Vec::new();
        for file in list_files() {
            // Example convention — replace with yours:
            if file.path.starts_with("routes/") {
                roots.push(ContributedRoot {
                    target: PluginTarget {
                        path: file.path.clone(),
                        symbol: None,
                    },
                    kind: RootKind::Production,
                    // Only `Certain` when the framework GUARANTEES invocation — a root keeps
                    // code alive forever, and kndo's zero-false-positive bar applies to you.
                    confidence: Confidence::Probable,
                });
            }
        }
        roots
    }

    /// Edges the language cannot see (route-string → handler, template → class).
    fn contribute_edges() -> Vec<ContributedEdge> {
        Vec::new()
    }

    /// Symbols consumed from outside the graph (FFI, serialization, a public SDK surface) —
    /// exempts them from internal-only/private-type-leak, never from unused.
    fn annotate_symbols() -> Vec<PluginTarget> {
        Vec::new()
    }

    /// Rules you may emit findings under (RFC 0018) — declare them here or they are dropped.
    /// A finding's severity IS its rule's declared severity; without a user's explicit
    /// [plugins.gate] opt-in your findings are advisory (shown, never gating).
    fn rules() -> Vec<RuleDescriptor> {
        Vec::new()
        // Example:
        // vec![RuleDescriptor {
        //     name: "deprecated-v1-api".to_string(),
        //     description: "calls to the v1 API are deprecated".to_string(),
        //     severity: FindingSeverity::Warning,
        // }]
    }

    /// Emit findings for the declared rules — verdicts, not graph facts. Same read surface
    /// as the other hooks (call-sites-in, references-to, read-file, …).
    fn contribute_findings() -> Vec<ContributedFinding> {
        Vec::new()
    }
}

export!(Component);
"#;

const ADAPTER_LIB_TEMPLATE: &str = r#"//! __NAME__ — a kndo language adapter (kndo:adapter ABI).
//!
//! Authoring guide: https://github.com/eulke/kndo — docs/plugins/authoring.md (§1 explains
//! when to write an adapter vs a plugin).
//! Inner loop: `kndo plugin build` then `kndo plugin verify __NAME__.wasm`.

// Marks the dependency used — the macro below is a fully-qualified path with no `use`.
use wit_bindgen as _;

wit_bindgen::generate!({
    // The ABI contract, vendored by `kndo plugin new` from the kndo you ran. To retarget a
    // newer kndo: `kndo plugin wit adapter > wit/adapter.wit` and rebuild. Do not edit it.
    path: "wit/__WIT_FILE__",
    world: "adapter",
});

use crate::kndo::adapter::types::*;

struct Component;

impl Guest for Component {
    fn descriptor() -> AdapterDescriptor {
        AdapterDescriptor {
            id: "__NAME__".to_string(),
            // Bump whenever extract()'s OUTPUT changes shape or meaning — it keys kndo's
            // facts cache, so a stale bump ships stale analysis.
            facts_schema_version: 1,
            file_globs: vec!["**/*.__NAME__".to_string()], // TODO: your real extension
            grammar_version: "0.1.0".to_string(),
            // When a globally installed copy activates. Empty NEVER self-activates globally.
            activation: vec![ActivationRule::FileExists("*.TODO-marker".to_string())],
            // Adapters this one builds on (a wrapper/superset language naming its base) —
            // activating this adapter co-activates them (RFC 0017 §6).
            dependencies: Vec::new(),
        }
    }

    /// Claim the files this adapter understands. `None` = not mine.
    fn claim(path: String) -> Option<FileClaim> {
        path.ends_with(".__NAME__").then_some(FileClaim {
            class: FileClass {
                role: FileRole::Production,
                origin: FileOrigin::Authored,
            },
        })
    }

    /// Parse one claimed file into facts: declarations, references, imports, roots.
    /// Called once per changed file — results are content-hash cached by the host.
    fn extract(_path: String, _content: String) -> FileFacts {
        FileFacts {
            declarations: Vec::new(),
            references: Vec::new(),
            roots: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

export!(Component);
"#;

const README_TEMPLATE: &str = r#"# __NAME__

A kndo component, scaffolded by `kndo plugin new`.

## Build & test

```bash
kndo plugin build              # cargo build + componentize -> __NAME__.wasm
kndo plugin verify __NAME__.wasm
```

`verify` reports what your descriptor declares, lints the common mistakes, and drives every
hook against a fixture project. Point it at your own fixture with
`kndo plugin verify __NAME__.wasm --project path/to/fixture`.

To try it in a real project: copy `__NAME__.wasm` into that project's `.kndo/plugins/`
directory (project-local components always run) and check `kndo doctor` there.

## Distribute

Publish a GitHub release tagged `vX.Y.Z` carrying `__NAME__.wasm` plus a `checksums.txt`
(`sha256sum` format) on the repository your descriptor `id` names — users then install it
with `kndo plugin install <your-coordinate>`. Full guide: docs/plugins/authoring.md in the
kndo repository.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_names_are_validated() {
        assert!(crate_name(Path::new("out/my-plugin")).is_ok());
        assert!(crate_name(Path::new("out/has space")).is_err());
        assert_eq!(crate_name(Path::new("x_1")).unwrap(), "x_1");
    }

    #[test]
    fn the_vendored_wit_is_the_hosts_own() {
        assert!(ComponentKind::Plugin.wit().contains("world plugin"));
        assert!(ComponentKind::Adapter.wit().contains("world adapter"));
    }
}
