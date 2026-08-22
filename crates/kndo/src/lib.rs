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

/// `kndo plugin install/list/remove` (RFC 0015 §4) — the registry-less installer over the
/// global plugin directory.
#[cfg(feature = "plugin-install")]
pub mod plugin_install;

/// `kndo plugin verify <component.wasm>` (RFC 0017 §7) — the author kit's local compliance
/// run over one component file.
#[cfg(feature = "external-adapters")]
pub mod verify;

/// Every first-party adapter this build includes — the product's language registry, defined
/// exactly once. Adding a language: one dependency + feature in this crate's manifest, one
/// entry here. Nothing else in the workspace changes.
pub fn default_adapters() -> Vec<Box<dyn LanguageAdapter>> {
    // One cfg-gated push per adapter, deliberately: with eight feature-gated languages this
    // stays the clearest composition shape (clippy's vec![] suggestion doesn't). `unused_mut`
    // is a false positive in the RFC 0016 §7 shell configuration (every language feature off):
    // none of the pushes below compile in, so nothing here needs `mut` in that build alone.
    #[allow(clippy::vec_init_then_push, unused_mut)]
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
/// workspace changes. These are *candidates*: each is still gated by its own
/// `PluginDescriptor.activation` rules in [`compose_plugins`] (RFC 0003 §4), so a convention
/// plugin never runs — or costs the graph-cache bypass — on a project it doesn't match. The
/// rest of RFC 0003 §3's stated launch set (react, spring, jest/vitest…) is tracked in the
/// ROADMAP, not silently implied by this function's name.
pub fn default_plugins() -> Vec<Box<dyn Plugin>> {
    // Same cfg-gated-push shape as `default_adapters`, same clippy reasoning.
    #[allow(clippy::vec_init_then_push)]
    {
        let mut plugins: Vec<Box<dyn Plugin>> = Vec::new();
        plugins.push(Box::new(kndo_core::plugin::LcovPlugin));
        #[cfg(feature = "plugin-nextjs")]
        plugins.push(Box::new(kndo_plugin_nextjs::NextjsPlugin));
        #[cfg(feature = "plugin-express")]
        plugins.push(Box::new(kndo_plugin_express::ExpressPlugin));
        plugins
    }
}

/// The one-line entry point frontends use: an [`Engine`] over the full default product plus
/// whatever third-party WASM adapters this project has installed. Frontends needing a custom
/// adapter or plugin set (embedders, tests) still have [`Engine::open`]/[`Engine::open_with_plugins`]
/// directly.
pub fn open(root: &Path, overrides: ConfigOverrides) -> Result<Engine, EngineError> {
    let (adapters, _resolution) = compose_adapters(root);
    let (plugins, _resolution) = compose_plugins(root);
    Engine::open_with_plugins(root, overrides, adapters, plugins)
}

/// Where a resolved plugin came from (RFC 0015 §2's three tiers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginSource {
    Builtin,
    ProjectLocal,
    Global,
}

/// Why a plugin is active for this project — the doctor-visible answer to "why is this
/// running?" (RFC 0003 §4's introspectability requirement, extended to RFC 0015 §3's
/// implication chains).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationReason {
    /// Dropped in `.kndo/plugins/` — presence is the opt-in.
    ProjectLocal,
    /// A built-in with no activation rules — always on.
    BuiltinAlwaysOn,
    /// One of its own `activation` rules matched the project.
    RuleMatched,
    /// Activated because the named (active) plugin lists it in `dependencies`.
    ImpliedBy(String),
}

/// One plugin the composition layer considered — active or not — with everything `kndo doctor`
/// needs to explain the outcome.
#[derive(Debug, Clone)]
pub struct ResolvedPlugin {
    pub id: String,
    pub version: String,
    pub source: PluginSource,
    pub activation: Vec<String>,
    pub dependencies: Vec<String>,
    /// `None` = present but inactive (a global candidate whose rules didn't fire and that
    /// nothing active depends on).
    pub active: Option<ActivationReason>,
}

/// A `dependencies` coordinate an *active* plugin names that no present plugin carries as its
/// id (RFC 0015 §3): never a runtime error — the depending plugin still runs — but reported so
/// the gap is visible instead of silent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MissingDependency {
    pub required_by: String,
    pub coordinate: String,
}

/// The full outcome of plugin composition for a project — what [`open`] registered and why,
/// plus what it *couldn't* satisfy. Recomputed on demand for `kndo doctor` (occasional command,
/// not the hot path), same posture as [`global_plugin_candidates`].
#[derive(Debug, Clone, Default)]
pub struct PluginResolution {
    pub plugins: Vec<ResolvedPlugin>,
    pub missing_dependencies: Vec<MissingDependency>,
}

/// [`compose_plugins`]' report half, for frontends (`kndo doctor`).
pub fn plugin_resolution(root: &Path) -> PluginResolution {
    compose_plugins(root).1
}

/// Assemble the full plugin set for `root` (RFC 0015 §3): built-ins, project-local
/// `.kndo/plugins/*.wasm`, and global candidates, seeded by their own activation rules and
/// closed over `dependencies` implication as a fixpoint. Built-ins with non-empty `activation`
/// are gated exactly like global candidates — a built-in convention plugin must never run (or
/// cost the cache bypass) on a project that doesn't match it.
fn compose_plugins(root: &Path) -> (Vec<Box<dyn Plugin>>, PluginResolution) {
    #[cfg(feature = "plugin-activation")]
    {
        let candidates = collect_candidates(root);
        let descriptors: Vec<kndo_core::plugin::PluginDescriptor> =
            candidates.iter().map(|(p, _)| p.descriptor()).collect();
        let sources: Vec<PluginSource> = candidates.iter().map(|(_, s)| *s).collect();
        let (active, missing_dependencies) = activation::resolve(&descriptors, &sources, root);

        let resolution = PluginResolution {
            plugins: descriptors
                .iter()
                .zip(&sources)
                .zip(&active)
                .map(|((d, source), reason)| resolved_plugin(d, *source, reason.clone()))
                .collect(),
            missing_dependencies,
        };
        let plugins = candidates
            .into_iter()
            .zip(active)
            .filter_map(|((plugin, _), reason)| reason.map(|_| plugin))
            .collect();
        (plugins, resolution)
    }
    #[cfg(not(feature = "plugin-activation"))]
    {
        let _ = root;
        // Minimal embedder build: no rule evaluation (the activation module's glob/manifest
        // machinery is feature-gated with it) — built-ins are unconditional, exactly the
        // pre-RFC-0015 behavior. Safe only because every gated built-in's feature
        // (`plugin-nextjs`/`plugin-express`) implies `plugin-activation`, so the sole
        // built-in that can land here is the lcov ingester (always-on is its original
        // contract, and it declares `mutates_graph() == false`).
        let plugins = default_plugins();
        let resolution = PluginResolution {
            plugins: plugins
                .iter()
                .map(|p| {
                    resolved_plugin(
                        &p.descriptor(),
                        PluginSource::Builtin,
                        Some(ActivationReason::BuiltinAlwaysOn),
                    )
                })
                .collect(),
            missing_dependencies: Vec::new(),
        };
        (plugins, resolution)
    }
}

fn resolved_plugin(
    d: &kndo_core::plugin::PluginDescriptor,
    source: PluginSource,
    active: Option<ActivationReason>,
) -> ResolvedPlugin {
    ResolvedPlugin {
        id: d.id.to_string(),
        version: d.version.to_string(),
        source,
        activation: d.activation.iter().map(|r| r.describe()).collect(),
        dependencies: d.dependencies.iter().map(|c| c.to_string()).collect(),
        active,
    }
}

/// The three candidate tiers, in deterministic order: built-ins, project-local `.kndo/plugins/`,
/// global directory — each `.wasm` load failure skipped, never fatal (RFC 0003 §3). The two
/// WASM tiers only exist with the WASM runtime (`external-adapters`); a plugins-only build
/// (`plugin-activation` via `plugin-nextjs`/`plugin-express`) still gates its built-ins here.
#[cfg(feature = "plugin-activation")]
fn collect_candidates(root: &Path) -> Vec<(Box<dyn Plugin>, PluginSource)> {
    let mut candidates: Vec<(Box<dyn Plugin>, PluginSource)> = Vec::new();
    for plugin in default_plugins() {
        candidates.push((plugin, PluginSource::Builtin));
    }
    #[cfg(feature = "external-adapters")]
    {
        candidates.extend(load_wasm_plugins(
            &project_plugin_dir(root),
            PluginSource::ProjectLocal,
        ));
        if let Some(global_dir) = activation::global_plugin_dir() {
            candidates.extend(load_wasm_plugins(&global_dir, PluginSource::Global));
        }
    }
    #[cfg(not(feature = "external-adapters"))]
    let _ = root;
    candidates
}

#[cfg(feature = "external-adapters")]
fn load_wasm_plugins(dir: &Path, source: PluginSource) -> Vec<(Box<dyn Plugin>, PluginSource)> {
    wasm_components(dir)
        .into_iter()
        .filter_map(|path| kndo_plugin_api::WasmPlugin::load(&path).ok())
        .map(|plugin| (Box::new(plugin) as Box<dyn Plugin>, source))
        .collect()
}

/// Where a resolved adapter came from — mirrors [`PluginSource`] (RFC 0016 §4 brings adapters
/// to the same three-tier shape plugins already had).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterSource {
    Builtin,
    ProjectLocal,
    Global,
}

/// One adapter the composition layer considered, active or not — the doctor-visible half of
/// RFC 0016 §4, matching [`ResolvedPlugin`]'s shape field for field since RFC 0017 §6 landed
/// the adapter-side `dependencies` fixpoint (the wrapper-adapter case: a `.vue`-style
/// superset language whose own extraction degrades without its base language's adapter
/// present). `active: None` means "present but inactive" — a global candidate whose rules
/// didn't fire and that nothing active depends on.
#[derive(Debug, Clone)]
pub struct ResolvedAdapter {
    pub id: String,
    pub source: AdapterSource,
    pub activation: Vec<String>,
    pub dependencies: Vec<String>,
    pub active: Option<ActivationReason>,
}

/// The full outcome of adapter composition for a project (RFC 0016 §4, RFC 0017 §6) — what
/// [`open`] registered, in claim-priority order, plus every global candidate that didn't
/// activate, plus every coordinate an active adapter depends on that nothing present carries.
#[derive(Debug, Clone, Default)]
pub struct AdapterResolution {
    pub adapters: Vec<ResolvedAdapter>,
    pub missing_dependencies: Vec<MissingDependency>,
}

/// [`compose_adapters`]'s report half, for frontends (`kndo doctor`) — mirrors
/// [`plugin_resolution`].
pub fn adapter_resolution(root: &Path) -> AdapterResolution {
    compose_adapters(root).1
}

/// Assemble the full adapter set for `root` (RFC 0016 §4) in **claim-priority order**: a
/// project-local `.kndo/plugins/*.wasm` adapter always wins a contested file extension over a
/// globally installed one, which always wins over a compiled-in one — the same "presence is
/// the strongest opt-in signal" reasoning `Plugin` composition already applies (RFC 0003 §3),
/// now written down as policy rather than left as compiled-in-happens-to-be-first accident.
/// Within one tier, adapters are ordered by id for deterministic claim resolution independent
/// of filesystem enumeration. Global adapters are gated by their own `activation` exactly like
/// global plugins (RFC 0003 §4): empty rules never self-activate from the global tier; a
/// project-local file is unconditional either way.
fn compose_adapters(root: &Path) -> (Vec<Box<dyn LanguageAdapter>>, AdapterResolution) {
    #[cfg(feature = "external-adapters")]
    {
        let mut project_local = load_wasm_adapters(&project_plugin_dir(root));
        sort_by_id(&mut project_local);
        let mut global_all = activation::global_plugin_dir()
            .map(|dir| load_wasm_adapters(&dir))
            .unwrap_or_default();
        sort_by_id(&mut global_all);
        let builtins = default_adapters();

        // Candidates in claim-priority order, then RFC 0017 §6's activation resolution: seed
        // each tier (project-local unconditional; global by its own rules; compiled-in
        // unconditional — a language adapter must run, `activation` only ever gates the
        // global tier), close over `dependencies` implication with the SAME fixpoint the
        // plugin side uses (`activation::imply_fixpoint` over kind-neutral identities), and
        // collect what active adapters require that nothing present provides.
        let descriptors: Vec<kndo_core::adapter::AdapterDescriptor> = project_local
            .iter()
            .map(|a| a.descriptor())
            .chain(global_all.iter().map(|a| a.descriptor()))
            .chain(builtins.iter().map(|a| a.descriptor()))
            .collect();
        let sources: Vec<AdapterSource> =
            std::iter::repeat_n(AdapterSource::ProjectLocal, project_local.len())
                .chain(std::iter::repeat_n(AdapterSource::Global, global_all.len()))
                .chain(std::iter::repeat_n(AdapterSource::Builtin, builtins.len()))
                .collect();
        let identities: Vec<activation::CandidateIdentity> = descriptors
            .iter()
            .map(|d| activation::CandidateIdentity {
                id: d.id.to_string(),
                dependencies: d.dependencies.iter().map(|c| c.to_string()).collect(),
            })
            .collect();
        let mut active: Vec<Option<ActivationReason>> = descriptors
            .iter()
            .zip(&sources)
            .map(|(d, source)| match source {
                AdapterSource::ProjectLocal => Some(ActivationReason::ProjectLocal),
                AdapterSource::Builtin => Some(ActivationReason::BuiltinAlwaysOn),
                AdapterSource::Global => activation::activates(&d.activation, root)
                    .then_some(ActivationReason::RuleMatched),
            })
            .collect();
        activation::imply_fixpoint(&identities, &mut active);
        let missing_dependencies = activation::collect_missing(&identities, &active);

        let resolved: Vec<ResolvedAdapter> = descriptors
            .iter()
            .zip(&sources)
            .zip(&active)
            .map(|((d, source), reason)| resolved_adapter(d, *source, reason.clone()))
            .collect();

        // Global-tier reasons sit right after the project-local ones in `active`; `zip`
        // stops at the global adapters, before the builtin tail.
        let global_active: Vec<_> = global_all
            .into_iter()
            .zip(&active[project_local.len()..])
            .filter_map(|(adapter, reason)| reason.as_ref().map(|_| adapter))
            .collect();

        let mut adapters: Vec<Box<dyn LanguageAdapter>> = Vec::new();
        adapters.extend(boxed(project_local));
        adapters.extend(boxed(global_active));
        adapters.extend(builtins);
        (
            adapters,
            AdapterResolution {
                adapters: resolved,
                missing_dependencies,
            },
        )
    }
    #[cfg(not(feature = "external-adapters"))]
    {
        let _ = root;
        let adapters = default_adapters();
        let resolved = adapters
            .iter()
            .map(|a| {
                resolved_adapter(
                    &a.descriptor(),
                    AdapterSource::Builtin,
                    Some(ActivationReason::BuiltinAlwaysOn),
                )
            })
            .collect();
        (
            adapters,
            AdapterResolution {
                adapters: resolved,
                missing_dependencies: Vec::new(),
            },
        )
    }
}

fn resolved_adapter(
    d: &kndo_core::adapter::AdapterDescriptor,
    source: AdapterSource,
    active: Option<ActivationReason>,
) -> ResolvedAdapter {
    ResolvedAdapter {
        id: d.id.to_string(),
        source,
        activation: d.activation.iter().map(|r| r.describe()).collect(),
        dependencies: d.dependencies.iter().map(|c| c.to_string()).collect(),
        active,
    }
}

#[cfg(feature = "external-adapters")]
fn sort_by_id(adapters: &mut [kndo_plugin_api::WasmAdapter]) {
    adapters.sort_by(|a, b| a.descriptor().id.cmp(&b.descriptor().id));
}

#[cfg(feature = "external-adapters")]
fn boxed(
    adapters: Vec<kndo_plugin_api::WasmAdapter>,
) -> impl Iterator<Item = Box<dyn LanguageAdapter>> {
    adapters
        .into_iter()
        .map(|a| Box::new(a) as Box<dyn LanguageAdapter>)
}

/// Every `.wasm` `LanguageAdapter` in `dir` — component-load failures skipped, never fatal
/// (RFC 0003 §3), same posture the plugin tier's loader already has.
#[cfg(feature = "external-adapters")]
fn load_wasm_adapters(dir: &Path) -> Vec<kndo_plugin_api::WasmAdapter> {
    wasm_components(dir)
        .into_iter()
        .filter_map(|path| kndo_plugin_api::WasmAdapter::load(&path).ok())
        .collect()
}

/// One globally installed [`LanguageAdapter`] candidate, as `kndo doctor` reports it — mirrors
/// [`GlobalPluginCandidate`], but carries the full [`ActivationReason`] (RFC 0017 §6): a global
/// adapter can now activate via another component's `dependencies`, and "active (dependency of
/// X)" is exactly the fact a rule-only bool would erase.
pub struct GlobalAdapterCandidate {
    pub id: String,
    pub activation: Vec<String>,
    pub active: Option<ActivationReason>,
}

/// Every `.wasm` `LanguageAdapter` found in the global directory for `root`, activated or not —
/// mirrors [`global_plugin_candidates`].
pub fn global_adapter_candidates(root: &Path) -> Vec<GlobalAdapterCandidate> {
    adapter_resolution(root)
        .adapters
        .into_iter()
        .filter(|a| a.source == AdapterSource::Global)
        .map(|a| GlobalAdapterCandidate {
            id: a.id,
            activation: a.activation,
            active: a.active,
        })
        .collect()
}

/// One globally installed `Plugin` candidate (RFC 0003 §4), as `kndo doctor` reports it —
/// unlike [`kndo_core::engine::DoctorPluginInfo`] (which only ever sees plugins that already
/// made it into composition), this covers *every* `.wasm` file the global directory holds,
/// skipped ones included, so a plugin whose `activation` rule doesn't match isn't invisible —
/// it shows up here with `activated: false` and the exact rule that didn't fire. A thin,
/// global-tier view over [`plugin_resolution`] — `activated` includes RFC 0015 §3 implication,
/// not just the candidate's own rules.
pub struct GlobalPluginCandidate {
    pub id: String,
    pub version: String,
    pub activation: Vec<String>,
    pub activated: bool,
}

/// Every `.wasm` `Plugin` found in the global directory for `root`, activated or not — the CLI's
/// `doctor` command calls this directly (not through `Engine`, which never sees a candidate that
/// didn't activate). Loads each component fresh, same cost profile as `kndo::open` paying it
/// once per run; `kndo doctor` is a standalone, occasional command, not the hot path.
pub fn global_plugin_candidates(root: &Path) -> Vec<GlobalPluginCandidate> {
    plugin_resolution(root)
        .plugins
        .into_iter()
        .filter(|p| p.source == PluginSource::Global)
        .map(|p| GlobalPluginCandidate {
            id: p.id,
            version: p.version,
            activation: p.activation,
            activated: p.active.is_some(),
        })
        .collect()
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
    let mut paths: Vec<std::path::PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("wasm"))
        .collect();
    // read_dir order is filesystem-dependent — sort so candidate order (and with it every
    // downstream report) is deterministic (RFC 0008 §4's discipline applied here too).
    paths.sort();
    paths
}

/// RFC 0003 §4: whether a plugin activates for a given project — the machine-checkable
/// counterpart to `PluginDescriptor.detection`'s prose — plus (with the WASM tier) where
/// globally installed `Plugin`s live. Gated built-in convention plugins share this exact
/// machinery, which is why it sits behind `plugin-activation` rather than `external-adapters`.
#[cfg(feature = "plugin-activation")]
mod activation {
    use kndo_core::plugin::ActivationRule;
    use std::path::Path;

    /// Overridable via `KNDO_PLUGIN_DIR` (tests, and any user who wants a non-default location);
    /// otherwise `<XDG data dir>/kndo/plugins` — `~/.local/share/kndo/plugins` on Linux,
    /// `~/Library/Application Support/kndo/plugins` on macOS, `%APPDATA%\kndo\plugins` on
    /// Windows. `None` when neither the override nor the platform data dir can be determined
    /// (e.g. `$HOME` unset) — global discovery is then simply skipped, not an error: a project's
    /// own `.kndo/plugins/` keeps working regardless.
    #[cfg(feature = "external-adapters")]
    pub(crate) fn global_plugin_dir() -> Option<std::path::PathBuf> {
        if let Ok(dir) = std::env::var("KNDO_PLUGIN_DIR") {
            return Some(std::path::PathBuf::from(dir));
        }
        dirs::data_dir().map(|d| d.join("kndo").join("plugins"))
    }

    /// A plugin with no activation rules never self-activates from the global directory — an
    /// empty list means "no known structural signal," not "always on" (zero-false-positive
    /// discipline: silence over a guess). Otherwise, any single matching rule is enough.
    pub(crate) fn activates(rules: &[ActivationRule], root: &Path) -> bool {
        !rules.is_empty() && rules.iter().any(|rule| matches(rule, root))
    }

    /// RFC 0015 §3's activation resolution: seed each candidate from its source and its own
    /// rules, then close over `dependencies` implication as a fixpoint. Returns, per candidate,
    /// `Some(reason)` (active) or `None` (present but inactive), plus every coordinate an
    /// active plugin depends on that no present candidate carries as its id.
    ///
    /// Seeding: project-local candidates are unconditional (presence is the opt-in); built-ins
    /// with an *empty* rule list are always-on (`LcovPlugin`'s original contract predates
    /// rules; today it has rules and is gated like everything else); anything with rules
    /// activates iff one matches. The fixpoint then activates any present candidate an active
    /// plugin names in `dependencies`, transitively — the wrapper-chain case
    /// (`company-framework → other-framework → kndo:express`) composes to arbitrary depth from
    /// a single manifest match. Cycles terminate trivially: activation is monotone over a
    /// finite set. Missing coordinates are collected only from *active* requirers (an inactive
    /// candidate's dependencies are moot) and never fail the run.
    pub(crate) fn resolve(
        descriptors: &[kndo_core::plugin::PluginDescriptor],
        sources: &[crate::PluginSource],
        root: &Path,
    ) -> (
        Vec<Option<crate::ActivationReason>>,
        Vec<crate::MissingDependency>,
    ) {
        let identities: Vec<CandidateIdentity> = descriptors
            .iter()
            .map(|d| CandidateIdentity {
                id: d.id.to_string(),
                dependencies: d.dependencies.iter().map(|c| c.to_string()).collect(),
            })
            .collect();
        let mut active = seed(descriptors, sources, root);
        imply_fixpoint(&identities, &mut active);
        let missing = collect_missing(&identities, &active);
        (active, missing)
    }

    /// The two fields the implication fixpoint and missing-dependency collection actually
    /// read — kind-neutral on purpose (RFC 0017 §6): `PluginDescriptor` and
    /// `AdapterDescriptor` both map into it, so plugins and adapters share one fixpoint
    /// implementation instead of two that could drift.
    pub(crate) struct CandidateIdentity {
        pub id: String,
        pub dependencies: Vec<String>,
    }

    fn seed(
        descriptors: &[kndo_core::plugin::PluginDescriptor],
        sources: &[crate::PluginSource],
        root: &Path,
    ) -> Vec<Option<crate::ActivationReason>> {
        use crate::{ActivationReason, PluginSource};
        descriptors
            .iter()
            .zip(sources)
            .map(|(d, source)| match source {
                PluginSource::ProjectLocal => Some(ActivationReason::ProjectLocal),
                PluginSource::Builtin if d.activation.is_empty() => {
                    Some(ActivationReason::BuiltinAlwaysOn)
                }
                PluginSource::Builtin | PluginSource::Global => {
                    activates(&d.activation, root).then_some(ActivationReason::RuleMatched)
                }
            })
            .collect()
    }

    /// Monotone over a finite set, so it terminates on any input, cycles included.
    pub(crate) fn imply_fixpoint(
        descriptors: &[CandidateIdentity],
        active: &mut [Option<crate::ActivationReason>],
    ) {
        let mut changed = true;
        while changed {
            changed = false;
            for (target, requirer_id) in pending_implications(descriptors, active) {
                active[target] = Some(crate::ActivationReason::ImpliedBy(requirer_id));
                changed = true;
            }
        }
    }

    /// One implication round: `(inactive target index, active requirer id)` pairs — recomputed
    /// per round rather than incrementally, since the candidate set is tiny (a handful of
    /// plugins, not a dependency universe).
    fn pending_implications(
        descriptors: &[CandidateIdentity],
        active: &[Option<crate::ActivationReason>],
    ) -> Vec<(usize, String)> {
        descriptors
            .iter()
            .enumerate()
            .filter(|(i, _)| active[*i].is_some())
            .flat_map(|(_, d)| {
                d.dependencies
                    .iter()
                    .map(move |dep| (d.id.to_string(), dep))
            })
            .filter_map(|(requirer, dep)| {
                descriptors
                    .iter()
                    .position(|o| o.id == *dep)
                    .map(|j| (j, requirer))
            })
            .filter(|(j, _)| active[*j].is_none())
            .collect()
    }

    /// Coordinates *active* plugins depend on that no present candidate carries as its id —
    /// inactive requirers contribute nothing (their dependencies are moot). Sorted + deduped:
    /// deterministic output regardless of candidate order (RFC 0008 §4).
    pub(crate) fn collect_missing(
        descriptors: &[CandidateIdentity],
        active: &[Option<crate::ActivationReason>],
    ) -> Vec<crate::MissingDependency> {
        let mut missing: Vec<crate::MissingDependency> = descriptors
            .iter()
            .enumerate()
            .filter(|(i, _)| active[*i].is_some())
            .flat_map(|(_, d)| {
                d.dependencies
                    .iter()
                    .map(move |dep| (d.id.to_string(), dep))
            })
            .filter(|(_, dep): &(String, &String)| {
                !descriptors.iter().any(|other| other.id == **dep)
            })
            .map(|(required_by, dep)| crate::MissingDependency {
                required_by,
                coordinate: dep.to_string(),
            })
            .collect();
        missing.sort();
        missing.dedup();
        missing
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

        fn descriptor(
            id: &str,
            activation: Vec<ActivationRule>,
            dependencies: &[&str],
        ) -> kndo_core::plugin::PluginDescriptor {
            kndo_core::plugin::PluginDescriptor {
                id: SmolStr::new(id),
                version: SmolStr::new("1"),
                detection: vec![],
                requested_file_access: vec![],
                activation,
                dependencies: dependencies.iter().map(SmolStr::new).collect(),
            }
        }

        #[test]
        fn dependency_chain_activates_transitively() {
            // The wrapper case end to end (RFC 0015 §1/§3): the project matches only the
            // company plugin's rule; nextjs and express activate purely through the chain.
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(
                dir.path().join("package.json"),
                r#"{"dependencies": {"@company/framework": "1.0.0"}}"#,
            )
            .unwrap();
            let descriptors = vec![
                descriptor(
                    "github.com/company/framework-plugin",
                    vec![ActivationRule::ManifestDependency(SmolStr::new(
                        "@company/framework",
                    ))],
                    &["kndo:nextjs"],
                ),
                descriptor(
                    "kndo:nextjs",
                    vec![ActivationRule::ManifestDependency(SmolStr::new("next"))],
                    &["kndo:express"],
                ),
                descriptor(
                    "kndo:express",
                    vec![ActivationRule::ManifestDependency(SmolStr::new("express"))],
                    &[],
                ),
            ];
            let sources = vec![
                crate::PluginSource::Global,
                crate::PluginSource::Builtin,
                crate::PluginSource::Builtin,
            ];
            let (active, missing) = resolve(&descriptors, &sources, dir.path());
            assert_eq!(active[0], Some(crate::ActivationReason::RuleMatched));
            assert_eq!(
                active[1],
                Some(crate::ActivationReason::ImpliedBy(
                    "github.com/company/framework-plugin".to_string()
                ))
            );
            assert_eq!(
                active[2],
                Some(crate::ActivationReason::ImpliedBy(
                    "kndo:nextjs".to_string()
                ))
            );
            assert!(missing.is_empty());
        }

        #[test]
        fn dependency_cycles_terminate_and_activate_both() {
            let dir = tempfile::tempdir().unwrap();
            let descriptors = vec![
                descriptor("a", vec![], &["b"]),
                descriptor("b", vec![], &["a"]),
            ];
            // `a` is project-local (unconditional); `b` only reachable through the cycle.
            let sources = vec![
                crate::PluginSource::ProjectLocal,
                crate::PluginSource::Global,
            ];
            let (active, missing) = resolve(&descriptors, &sources, dir.path());
            assert_eq!(active[0], Some(crate::ActivationReason::ProjectLocal));
            assert_eq!(
                active[1],
                Some(crate::ActivationReason::ImpliedBy("a".to_string()))
            );
            assert!(missing.is_empty());
        }

        #[test]
        fn missing_dependency_is_reported_never_fatal() {
            let dir = tempfile::tempdir().unwrap();
            let descriptors = vec![descriptor("a", vec![], &["github.com/x/not-installed"])];
            let sources = vec![crate::PluginSource::ProjectLocal];
            let (active, missing) = resolve(&descriptors, &sources, dir.path());
            assert!(active[0].is_some(), "the requirer itself still runs");
            assert_eq!(
                missing,
                vec![crate::MissingDependency {
                    required_by: "a".to_string(),
                    coordinate: "github.com/x/not-installed".to_string(),
                }]
            );
        }

        #[test]
        fn inactive_requirers_do_not_report_missing_or_imply() {
            // An inactive global candidate's dependencies are moot: no implication from it,
            // no missing-coordinate noise for it.
            let dir = tempfile::tempdir().unwrap();
            let descriptors = vec![
                descriptor(
                    "inactive",
                    vec![ActivationRule::FileExists(SmolStr::new("nope.marker"))],
                    &["also-present", "github.com/x/absent"],
                ),
                descriptor("also-present", vec![], &[]),
            ];
            let sources = vec![crate::PluginSource::Global, crate::PluginSource::Global];
            let (active, missing) = resolve(&descriptors, &sources, dir.path());
            assert!(active[0].is_none());
            assert!(
                active[1].is_none(),
                "nothing active implies it, and empty rules never self-activate globally"
            );
            assert!(missing.is_empty());
        }

        #[test]
        fn builtin_with_rules_is_gated_and_builtin_without_is_not() {
            let dir = tempfile::tempdir().unwrap();
            let descriptors = vec![
                descriptor(
                    "kndo:gated",
                    vec![ActivationRule::FileExists(SmolStr::new("nope.marker"))],
                    &[],
                ),
                descriptor("kndo:always", vec![], &[]),
            ];
            let sources = vec![crate::PluginSource::Builtin, crate::PluginSource::Builtin];
            let (active, _) = resolve(&descriptors, &sources, dir.path());
            assert!(active[0].is_none());
            assert_eq!(active[1], Some(crate::ActivationReason::BuiltinAlwaysOn));
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
