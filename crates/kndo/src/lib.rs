//! kndo — the composed product, as a library (RFC 0001 §2 layering).
//!
//! This is the **distribution layer**: it owns the composition "kndo = core + these
//! languages (+ these built-in plugins, when they land)". That knowledge belongs to the
//! product, not to any presentation layer — a frontend (CLI, `kndo serve`/MCP, LSP, GUI, CI
//! action) depends on this crate alone and can never accidentally ship a kndo missing
//! languages, nor does adding a language ever touch a frontend.
//!
//! The ignorance rule survives intact: `kndo-core` still never depends on an adapter — this
//! crate sits *above* both and points downward at each.
//!
//! Embedders wanting a subset build with `--no-default-features --features js,go,…`
//! (ADR 0006 feature-gated builds).

pub use kndo_core::{
    adapter, analysis, cache, coverage, discovery, engine, graph, plugin, query, query_envelope,
    vocab,
};

use kndo_core::adapter::LanguageAdapter;
use kndo_core::engine::{ConfigOverrides, Engine, EngineError};
use kndo_core::plugin::Plugin;
use std::path::Path;

/// Every first-party adapter this build includes — the product's language registry, defined
/// exactly once. Adding a language: one dependency + feature in this crate's manifest, one
/// entry here. Nothing else in the workspace changes.
pub fn default_adapters() -> Vec<Box<dyn LanguageAdapter>> {
    // One cfg-gated push per adapter, deliberately: with eight feature-gated languages this
    // stays the clearest composition shape (clippy's vec![] suggestion doesn't).
    #[allow(clippy::vec_init_then_push)]
    {
        let mut adapters: Vec<Box<dyn LanguageAdapter>> = Vec::new();
        #[cfg(feature = "js")]
        adapters.push(Box::new(kndo_adapter_js::JsTsAdapter));
        #[cfg(feature = "go")]
        adapters.push(Box::new(kndo_adapter_go::GoAdapter));
        #[cfg(feature = "rust")]
        adapters.push(Box::new(kndo_adapter_rust::RustAdapter));
        #[cfg(feature = "java")]
        adapters.push(Box::new(kndo_adapter_java::JavaAdapter));
        #[cfg(feature = "kotlin")]
        adapters.push(Box::new(kndo_adapter_kotlin::KotlinAdapter));
        #[cfg(feature = "swift")]
        adapters.push(Box::new(kndo_adapter_swift::SwiftAdapter));
        #[cfg(feature = "json")]
        adapters.push(Box::new(kndo_adapter_json::JsonAdapter));
        #[cfg(feature = "css")]
        adapters.push(Box::new(kndo_adapter_css::CssAdapter));
        adapters
    }
}

/// Every first-party plugin this build includes (RFC 0003) — the product's ecosystem registry,
/// same shape and same reasoning as [`default_adapters`]: one entry here, nothing else in the
/// workspace changes. Just the built-in lcov coverage ingester today; framework-convention
/// plugins (react, nextjs, spring…) are RFC 0003 §3's stated launch set, not yet built —
/// tracked in the ROADMAP, not silently implied by this function's name.
pub fn default_plugins() -> Vec<Box<dyn Plugin>> {
    vec![Box::new(kndo_core::plugin::LcovPlugin)]
}

/// The one-line entry point frontends use: an [`Engine`] over the full default product plus
/// whatever third-party WASM adapters this project has installed. Frontends needing a custom
/// adapter or plugin set (embedders, tests) still have [`Engine::open`]/[`Engine::open_with_plugins`]
/// directly.
pub fn open(root: &Path, overrides: ConfigOverrides) -> Result<Engine, EngineError> {
    let mut adapters = default_adapters();
    adapters.extend(external_adapters(root));
    let mut plugins = default_plugins();
    plugins.extend(external_plugins(root));
    Engine::open_with_plugins(root, overrides, adapters, plugins)
}

/// Third-party adapters as WASM components (ADR 0003, `docs/contracts/wasm-abi.md`),
/// auto-discovered from `.kndo/plugins/*.wasm` (RFC 0003 §3's stated convention) — no
/// `kndo.toml` entry needed, the same "drop a file in, it's live" default every other
/// zero-config surface in this product follows. A component that fails to load (not a real
/// component binary, a version mismatch, an instantiation error) is skipped rather than
/// failing the whole run: one broken extension must not take every other language down with
/// it. There is no diagnostic surfaced for a *load*-time failure yet (unlike a per-call
/// fuel/budget trip inside `WasmAdapter::extract`, which does produce one) — an honest gap,
/// not an omission papered over; `kndo doctor`'s adapter list is the way to confirm a plugin
/// actually loaded until one lands.
///
/// Adapters are project-local only — [`activation`] (RFC 0003 §4) gates the *global* XDG
/// install path, and that path only exists for `Plugin`s today (see [`external_plugins`]);
/// extending it to `LanguageAdapter` is a natural follow-up, not done here.
fn external_adapters(root: &Path) -> Vec<Box<dyn LanguageAdapter>> {
    #[cfg(feature = "external-adapters")]
    {
        wasm_components(&project_plugin_dir(root))
            .into_iter()
            .filter_map(|path| kndo_plugin_api::WasmAdapter::load(&path).ok())
            .map(|adapter| Box::new(adapter) as Box<dyn LanguageAdapter>)
            .collect()
    }
    #[cfg(not(feature = "external-adapters"))]
    {
        let _ = root;
        Vec::new()
    }
}

/// Third-party `Plugin`s as WASM components (`docs/contracts/wasm-abi.md` §5) from two sources:
///
/// - **Project-local** `.kndo/plugins/*.wasm` (RFC 0003 §3) — unconditional, same zero-config
///   discovery [`external_adapters`] uses. A file's presence there already is the opt-in.
/// - **Global** [`activation::global_plugin_dir`] (RFC 0003 §4) — installed once, shared across
///   every project on the machine, so presence alone can't be the opt-in signal. Each candidate
///   is filtered through [`activation::activates`] against `root`, evaluating its
///   `PluginDescriptor.activation` rules; a plugin with none never self-activates from here.
///
/// A `.wasm` file only ever implements one of the two ABIs (`kndo:adapter` or `kndo:plugin`) —
/// its world's exports say which, so nothing here has to *ask*: [`WasmAdapter::load`] and
/// [`WasmPlugin::load`] both simply fail to instantiate against a component compiled for the
/// other world's exports, and each loader silently skips what it can't load. A single directory
/// scan feeding both loaders (per source) is deliberate, not an accident of how
/// [`external_adapters`] already existed — a plugin author never has to name their file
/// `*.adapter.wasm` vs `*.plugin.wasm` or sort it into a subdirectory to say which ABI it
/// targets.
fn external_plugins(root: &Path) -> Vec<Box<dyn Plugin>> {
    #[cfg(feature = "external-adapters")]
    {
        let mut plugins: Vec<Box<dyn Plugin>> = wasm_components(&project_plugin_dir(root))
            .into_iter()
            .filter_map(|path| kndo_plugin_api::WasmPlugin::load(&path).ok())
            .map(|plugin| Box::new(plugin) as Box<dyn Plugin>)
            .collect();
        if let Some(global_dir) = activation::global_plugin_dir() {
            plugins.extend(
                wasm_components(&global_dir)
                    .into_iter()
                    .filter_map(|path| kndo_plugin_api::WasmPlugin::load(&path).ok())
                    .filter(|plugin| activation::activates(&plugin.descriptor().activation, root))
                    .map(|plugin| Box::new(plugin) as Box<dyn Plugin>),
            );
        }
        plugins
    }
    #[cfg(not(feature = "external-adapters"))]
    {
        let _ = root;
        Vec::new()
    }
}

#[cfg(feature = "external-adapters")]
fn project_plugin_dir(root: &Path) -> std::path::PathBuf {
    root.join(".kndo").join("plugins")
}

#[cfg(feature = "external-adapters")]
fn wasm_components(dir: &Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("wasm"))
        .collect()
}

/// RFC 0003 §4: where globally installed `Plugin`s live, and whether one activates for a given
/// project — the machine-checkable counterpart to `PluginDescriptor.detection`'s prose.
#[cfg(feature = "external-adapters")]
mod activation {
    use kndo_core::plugin::ActivationRule;
    use std::path::{Path, PathBuf};

    /// Overridable via `KNDO_PLUGIN_DIR` (tests, and any user who wants a non-default location);
    /// otherwise `<XDG data dir>/kndo/plugins` — `~/.local/share/kndo/plugins` on Linux,
    /// `~/Library/Application Support/kndo/plugins` on macOS, `%APPDATA%\kndo\plugins` on
    /// Windows. `None` when neither the override nor the platform data dir can be determined
    /// (e.g. `$HOME` unset) — global discovery is then simply skipped, not an error: a project's
    /// own `.kndo/plugins/` keeps working regardless.
    pub(crate) fn global_plugin_dir() -> Option<PathBuf> {
        if let Ok(dir) = std::env::var("KNDO_PLUGIN_DIR") {
            return Some(PathBuf::from(dir));
        }
        dirs::data_dir().map(|d| d.join("kndo").join("plugins"))
    }

    /// A plugin with no activation rules never self-activates from the global directory — an
    /// empty list means "no known structural signal," not "always on" (zero-false-positive
    /// discipline: silence over a guess). Otherwise, any single matching rule is enough.
    pub(crate) fn activates(rules: &[ActivationRule], root: &Path) -> bool {
        !rules.is_empty() && rules.iter().any(|rule| matches(rule, root))
    }

    fn matches(rule: &ActivationRule, root: &Path) -> bool {
        match rule {
            ActivationRule::FileExists(pattern) => file_exists(root, pattern),
            ActivationRule::ManifestDependency(name) => manifest_declares(root, name),
        }
    }

    fn file_exists(root: &Path, pattern: &str) -> bool {
        let full_pattern = root.join(pattern);
        let Some(full_pattern) = full_pattern.to_str() else {
            return false;
        };
        glob::glob(full_pattern).is_ok_and(|mut paths| paths.any(|p| p.is_ok()))
    }

    /// Every `package.json`/`Cargo.toml` anywhere under the project root — not just the root's
    /// own — using kndo-core's own gitignore-aware walker (`node_modules`, `.kndo/`, etc.
    /// excluded exactly like every other analysis in this product; a second, hand-rolled walker
    /// here would risk drifting from that). A monorepo where only one package depends on `react`
    /// must still activate a `react` plugin — restricting this to the root manifest would have
    /// made every monorepo a false negative, and kndo's monorepo support is not speculative
    /// (RFC 0012 §8/§10 already resolve per-package topology for real).
    fn manifest_declares(root: &Path, name: &str) -> bool {
        kndo_core::discovery::find_files_named(root, &["package.json", "Cargo.toml"])
            .into_iter()
            .any(|path| match path.file_name().and_then(|n| n.to_str()) {
                Some("package.json") => package_json_declares(&path, name),
                Some("Cargo.toml") => cargo_toml_declares(&path, name),
                _ => false,
            })
    }

    fn package_json_declares(manifest_path: &Path, name: &str) -> bool {
        const SECTIONS: [&str; 4] = [
            "dependencies",
            "devDependencies",
            "peerDependencies",
            "optionalDependencies",
        ];
        let Ok(content) = std::fs::read_to_string(manifest_path) else {
            return false;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            return false;
        };
        SECTIONS.iter().any(|section| {
            value
                .get(section)
                .and_then(|v| v.as_object())
                .is_some_and(|deps| deps.contains_key(name))
        })
    }

    fn cargo_toml_declares(manifest_path: &Path, name: &str) -> bool {
        const SECTIONS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
        let Ok(content) = std::fs::read_to_string(manifest_path) else {
            return false;
        };
        let Ok(value) = content.parse::<toml::Table>() else {
            return false;
        };
        // Cargo treats `-`/`_` as interchangeable in a crate name (spec §3's own idiom
        // elsewhere in this codebase) — a plugin author shouldn't have to guess which spelling
        // a project used.
        let hyphenated = name.replace('_', "-");
        let underscored = name.replace('-', "_");
        SECTIONS.iter().any(|section| {
            value
                .get(*section)
                .and_then(|v| v.as_table())
                .is_some_and(|deps| {
                    deps.contains_key(name)
                        || deps.contains_key(&hyphenated)
                        || deps.contains_key(&underscored)
                })
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use smol_str::SmolStr;

        #[test]
        fn no_rules_never_activates() {
            let dir = tempfile::tempdir().unwrap();
            assert!(!activates(&[], dir.path()));
        }

        #[test]
        fn file_glob_rule_matches_a_present_file() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("next.config.js"), "").unwrap();
            let rules = vec![ActivationRule::FileExists(SmolStr::new("next.config.*"))];
            assert!(activates(&rules, dir.path()));
        }

        #[test]
        fn file_glob_rule_does_not_match_when_absent() {
            let dir = tempfile::tempdir().unwrap();
            let rules = vec![ActivationRule::FileExists(SmolStr::new("next.config.*"))];
            assert!(!activates(&rules, dir.path()));
        }

        #[test]
        fn manifest_dependency_rule_matches_package_json() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(
                dir.path().join("package.json"),
                r#"{"dependencies": {"react": "^18.0.0"}}"#,
            )
            .unwrap();
            let rules = vec![ActivationRule::ManifestDependency(SmolStr::new("react"))];
            assert!(activates(&rules, dir.path()));
        }

        #[test]
        fn manifest_dependency_rule_matches_cargo_toml_across_hyphen_underscore() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(
                dir.path().join("Cargo.toml"),
                "[dependencies]\nserde_json = \"1\"\n",
            )
            .unwrap();
            let rules = vec![ActivationRule::ManifestDependency(SmolStr::new(
                "serde-json",
            ))];
            assert!(activates(&rules, dir.path()));
        }

        #[test]
        fn manifest_dependency_rule_does_not_match_when_absent() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("package.json"), r#"{"dependencies": {}}"#).unwrap();
            let rules = vec![ActivationRule::ManifestDependency(SmolStr::new("react"))];
            assert!(!activates(&rules, dir.path()));
        }

        #[test]
        fn manifest_dependency_rule_matches_a_nested_monorepo_package() {
            // A root manifest that itself declares nothing must not shadow a real dependency
            // three levels down — restricting this to the root only would make every monorepo
            // a false negative, and kndo's monorepo support isn't speculative anywhere else.
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("package.json"), r#"{"dependencies": {}}"#).unwrap();
            let web_pkg = dir.path().join("packages").join("web");
            std::fs::create_dir_all(&web_pkg).unwrap();
            std::fs::write(
                web_pkg.join("package.json"),
                r#"{"dependencies": {"react": "^18.0.0"}}"#,
            )
            .unwrap();
            let rules = vec![ActivationRule::ManifestDependency(SmolStr::new("react"))];
            assert!(activates(&rules, dir.path()));
        }

        #[test]
        fn manifest_dependency_rule_skips_gitignored_packages() {
            // node_modules is exactly why this reuses discovery's own walker instead of a
            // fresh one: a transitive dependency's own package.json declaring "react" deep in
            // node_modules must never activate a plugin the project itself doesn't use.
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(".gitignore"), "node_modules/\n").unwrap();
            std::fs::write(dir.path().join("package.json"), r#"{"dependencies": {}}"#).unwrap();
            let nested = dir.path().join("node_modules").join("some-lib");
            std::fs::create_dir_all(&nested).unwrap();
            std::fs::write(
                nested.join("package.json"),
                r#"{"dependencies": {"react": "^18.0.0"}}"#,
            )
            .unwrap();
            let rules = vec![ActivationRule::ManifestDependency(SmolStr::new("react"))];
            assert!(!activates(&rules, dir.path()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_core::engine::{CheckRequest, RunMode};

    #[test]
    fn default_build_registers_at_least_one_language() {
        assert!(!default_adapters().is_empty());
    }

    #[test]
    fn open_composes_the_full_product() {
        let dir = std::env::temp_dir().join("kndo-dist-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.ts"), "export function f() { return 1; }").unwrap();

        let mut engine = open(&dir, ConfigOverrides::default()).unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Full,
        });
        // The js adapter came from the distribution layer, not from this test.
        assert_eq!(result.files_claimed, 1);
        assert_eq!(result.symbols, 1);
    }
}
