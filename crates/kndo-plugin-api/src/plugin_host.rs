//! The wasmtime host bridge for the `kndo:plugin` WASM ABI (`docs/contracts/wasm-abi.md` §5) —
//! bridges the four `kndo_core::plugin::Plugin` graph-mutation hooks. Unlike the adapter bridge
//! (`host.rs`), this world is bidirectional: the guest calls back into two host-provided query
//! functions (`list-files`, `symbols-in`) while computing its contributions.

use std::fmt;
use std::path::Path;
use std::sync::Mutex;

use kndo_core::adapter::ProjectPath;
use kndo_core::plugin::{
    AnnotationSink, EdgeSink, GraphView, Plugin, PluginDescriptor, PluginTarget, RootSink,
};
use kndo_core::vocab::{FileClass, FileOrigin, FileRole, RefKind, RootKind, SymbolKind};
use smol_str::SmolStr;

mod bindings {
    wasmtime::component::bindgen!({
        path: "wit/plugin.wit",
        world: "plugin",
    });
}

use bindings::kndo::plugin::types as w;
use bindings::Plugin as WitPluginBindings;

/// Same discipline as the adapter bridge's `FUEL_PER_CALL` (RFC 0003 §3): a trapped/exhausted
/// hook degrades to "contributed nothing" rather than aborting the run.
const FUEL_PER_CALL: u64 = 50_000_000;

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Instantiate(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "failed to read WASM component: {e}"),
            LoadError::Instantiate(msg) => write!(f, "failed to instantiate WASM plugin: {msg}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Owned snapshot of exactly what a WASM plugin's host-import queries can answer from — cloned
/// from the real `GraphView` once per graph-mutation hook round (not once per query), so
/// `list-files`/`symbols-in` never touch the filesystem or reach back into the caller's
/// borrowed graph. `wasmtime::Store`'s state must be `'static`; a live `&GraphView<'_>`
/// (borrowed for one `assemble_from_source` call) can't be, so this trades a bounded clone
/// for that requirement rather than reaching for unsafe raw-pointer plumbing — a WASM plugin
/// already forces a full graph rebuild every run (docs/contracts/wasm-abi.md §5), so one more
/// `O(files + symbols)` clone alongside that is proportionally small.
struct HostViewData {
    files: Vec<w::WasmFileInfo>,
    symbols_by_file: rustc_hash::FxHashMap<smol_str::SmolStr, Vec<w::WasmSymbolInfo>>,
}

impl HostViewData {
    fn empty() -> Self {
        HostViewData {
            files: Vec::new(),
            symbols_by_file: rustc_hash::FxHashMap::default(),
        }
    }

    fn from_view(graph: &GraphView<'_>) -> Self {
        let mut files = Vec::new();
        let mut symbols_by_file: rustc_hash::FxHashMap<SmolStr, Vec<w::WasmSymbolInfo>> =
            rustc_hash::FxHashMap::default();
        for file in graph.files() {
            let Some(class) = file.class else {
                continue; // unclaimed — nothing a plugin can usefully query about it
            };
            files.push(w::WasmFileInfo {
                path: file.path.0.to_string(),
                role: to_wit_role(class.role),
                origin: to_wit_origin(class.origin),
            });
            let symbols = graph
                .symbols_in(&file.path)
                .map(|s| w::WasmSymbolInfo {
                    name: s.name.to_string(),
                    kind: to_wit_symbol_kind(s.kind.clone()),
                    exported: s.exported,
                    member_of: s.member_of.as_ref().map(|m| m.to_string()),
                })
                .collect();
            symbols_by_file.insert(file.path.0.clone(), symbols);
        }
        HostViewData {
            files,
            symbols_by_file,
        }
    }
}

impl bindings::kndo::plugin::types::Host for HostViewData {}

impl bindings::PluginImports for HostViewData {
    fn list_files(&mut self) -> Vec<w::WasmFileInfo> {
        self.files.clone()
    }

    fn symbols_in(&mut self, path: String) -> Vec<w::WasmSymbolInfo> {
        self.symbols_by_file
            .get(path.as_str())
            .cloned()
            .unwrap_or_default()
    }
}

struct GuestState {
    store: wasmtime::Store<HostViewData>,
    bindings: WitPluginBindings,
}

/// A `kndo:plugin` WASM component, bridged to the native [`Plugin`] trait — indistinguishable
/// from a built-in plugin (e.g. `LcovPlugin`) from `Engine`'s perspective, same ADR 0003
/// "generated bridge" posture the adapter bridge already established.
pub struct WasmPlugin {
    engine: wasmtime::Engine,
    component: wasmtime::component::Component,
    descriptor: PluginDescriptor,
    // Re-instantiated per graph-mutation round (see `with_instance`) since each round needs a
    // freshly built `HostViewData` snapshot of *that* round's graph — the store's state isn't
    // reusable across rounds the way `WasmAdapter`'s state-free calls are.
    linker: wasmtime::component::Linker<HostViewData>,
    last_hooks: Mutex<Option<GuestState>>,
}

impl WasmPlugin {
    pub fn load(path: &Path) -> Result<WasmPlugin, LoadError> {
        let bytes = read_component_bytes(path)?;
        let (engine, component, linker) = build_runtime(&bytes)?;
        let descriptor = probe_descriptor(&engine, &component, &linker)?;

        Ok(WasmPlugin {
            engine,
            component,
            descriptor,
            linker,
            last_hooks: Mutex::new(None),
        })
    }

    /// (Re)instantiate the component against a fresh snapshot of `graph`, replacing whatever
    /// instance served the previous graph-mutation round.
    fn refresh_instance(&self, graph: &GraphView<'_>) -> Option<()> {
        let (store, bindings) = instantiate_with(
            &self.engine,
            &self.component,
            &self.linker,
            HostViewData::from_view(graph),
        )
        .ok()?;
        *self.last_hooks.lock().expect("wasm plugin store poisoned") =
            Some(GuestState { store, bindings });
        Some(())
    }
}

/// Reads, engine-configures, links, instantiates — split for the same reason as the adapter
/// bridge's own `instantiate` pipeline in `host.rs`: one fallible step per function keeps each
/// step's own complexity low instead of one long wall of `?`s.
fn read_component_bytes(path: &Path) -> Result<Vec<u8>, LoadError> {
    std::fs::read(path).map_err(LoadError::Io)
}

fn fuel_budgeted_engine() -> Result<wasmtime::Engine, LoadError> {
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true);
    wasmtime::Engine::new(&config).map_err(|e| LoadError::Instantiate(e.to_string()))
}

fn load_component(
    engine: &wasmtime::Engine,
    bytes: &[u8],
) -> Result<wasmtime::component::Component, LoadError> {
    wasmtime::component::Component::from_binary(engine, bytes)
        .map_err(|e| LoadError::Instantiate(e.to_string()))
}

fn build_linker(
    engine: &wasmtime::Engine,
) -> Result<wasmtime::component::Linker<HostViewData>, LoadError> {
    let mut linker = wasmtime::component::Linker::new(engine);
    WitPluginBindings::add_to_linker(&mut linker, |state: &mut HostViewData| state)
        .map_err(|e| LoadError::Instantiate(format!("linking host imports failed: {e}")))?;
    Ok(linker)
}

/// Engine, component, and linker in one step — keeps `load`'s own `?` count (and so its
/// cyclomatic complexity) low; each piece is still its own single-purpose function underneath.
#[allow(clippy::type_complexity)]
fn build_runtime(
    bytes: &[u8],
) -> Result<
    (
        wasmtime::Engine,
        wasmtime::component::Component,
        wasmtime::component::Linker<HostViewData>,
    ),
    LoadError,
> {
    let engine = fuel_budgeted_engine()?;
    let component = load_component(&engine, bytes)?;
    let linker = build_linker(&engine)?;
    Ok((engine, component, linker))
}

/// Instantiate against a caller-supplied view — shared by `load`'s throwaway descriptor probe,
/// `refresh_instance`'s per-round instance, and `classify_file`'s view-less instance, so the
/// fuel-budgeting/instantiate sequence is written exactly once.
fn instantiate_with(
    engine: &wasmtime::Engine,
    component: &wasmtime::component::Component,
    linker: &wasmtime::component::Linker<HostViewData>,
    view: HostViewData,
) -> Result<(wasmtime::Store<HostViewData>, WitPluginBindings), LoadError> {
    let mut store = wasmtime::Store::new(engine, view);
    store
        .set_fuel(FUEL_PER_CALL)
        .map_err(|e| LoadError::Instantiate(e.to_string()))?;
    let bindings = WitPluginBindings::instantiate(&mut store, component, linker)
        .map_err(|e| LoadError::Instantiate(e.to_string()))?;
    Ok((store, bindings))
}

fn probe_descriptor(
    engine: &wasmtime::Engine,
    component: &wasmtime::component::Component,
    linker: &wasmtime::component::Linker<HostViewData>,
) -> Result<PluginDescriptor, LoadError> {
    let (mut store, bindings) = instantiate_with(engine, component, linker, HostViewData::empty())?;
    let raw = bindings
        .call_descriptor(&mut store)
        .map_err(|e| LoadError::Instantiate(format!("descriptor() failed: {e}")))?;
    Ok(PluginDescriptor {
        id: SmolStr::new(&raw.id),
        version: SmolStr::new(&raw.version),
        detection: raw.detection.iter().map(SmolStr::new).collect(),
        requested_file_access: raw.requested_file_access.iter().map(SmolStr::new).collect(),
        activation: raw
            .activation
            .into_iter()
            .map(from_wit_activation_rule)
            .collect(),
    })
}

fn from_wit_activation_rule(rule: w::ActivationRule) -> kndo_core::plugin::ActivationRule {
    match rule {
        w::ActivationRule::FileExists(glob) => {
            kndo_core::plugin::ActivationRule::FileExists(SmolStr::new(&glob))
        }
        w::ActivationRule::ManifestDependency(name) => {
            kndo_core::plugin::ActivationRule::ManifestDependency(SmolStr::new(&name))
        }
    }
}

impl Plugin for WasmPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        self.descriptor.clone()
    }

    fn classify_file(&self, path: &ProjectPath, current: FileClass) -> Option<FileClass> {
        // classify_file runs before contribute_roots/contribute_edges/annotate_symbols in the
        // assembly pipeline (phase 2 vs. after phase 3b) and needs no graph queries of its own
        // (RFC 0003 §2's own hook contract — it only ever sees the one file it's asked about),
        // so it gets a lightweight, view-less instance rather than forcing a premature
        // `refresh_instance` — the graph isn't even fully built yet at this point.
        let (mut store, bindings) = instantiate_with(
            &self.engine,
            &self.component,
            &self.linker,
            HostViewData::empty(),
        )
        .ok()?;
        let result = bindings
            .call_classify_file(
                &mut store,
                path.0.as_str(),
                w::FileClass {
                    role: to_wit_role(current.role),
                    origin: to_wit_origin(current.origin),
                },
            )
            .ok()?;
        result.map(|c| FileClass {
            role: from_wit_role(c.role),
            origin: from_wit_origin(c.origin),
        })
    }

    fn contribute_roots(&self, graph: &GraphView<'_>, out: &mut RootSink) {
        if self.refresh_instance(graph).is_none() {
            return;
        }
        let mut guard = self.last_hooks.lock().expect("wasm plugin store poisoned");
        let Some(GuestState { store, bindings }) = guard.as_mut() else {
            return;
        };
        let Ok(roots) = bindings.call_contribute_roots(&mut *store) else {
            return;
        };
        for r in roots {
            out.add(
                from_wit_target(r.target),
                from_wit_root_kind(r.kind),
                from_wit_confidence(r.confidence),
            );
        }
    }

    fn contribute_edges(&self, graph: &GraphView<'_>, out: &mut EdgeSink) {
        if self.refresh_instance(graph).is_none() {
            return;
        }
        let mut guard = self.last_hooks.lock().expect("wasm plugin store poisoned");
        let Some(GuestState { store, bindings }) = guard.as_mut() else {
            return;
        };
        let Ok(edges) = bindings.call_contribute_edges(&mut *store) else {
            return;
        };
        for e in edges {
            out.add(
                from_wit_target(e.from),
                from_wit_target(e.to),
                from_wit_ref_kind(e.kind),
                from_wit_confidence(e.confidence),
            );
        }
    }

    fn annotate_symbols(&self, graph: &GraphView<'_>, out: &mut AnnotationSink) {
        if self.refresh_instance(graph).is_none() {
            return;
        }
        let mut guard = self.last_hooks.lock().expect("wasm plugin store poisoned");
        let Some(GuestState { store, bindings }) = guard.as_mut() else {
            return;
        };
        let Ok(targets) = bindings.call_annotate_symbols(&mut *store) else {
            return;
        };
        for t in targets {
            if let Some(symbol) = t.symbol {
                out.mark_externally_consumed(ProjectPath(SmolStr::new(&t.path)), symbol);
            }
        }
    }
}

fn to_wit_role(role: FileRole) -> w::FileRole {
    match role {
        FileRole::Production => w::FileRole::Production,
        FileRole::Test => w::FileRole::Test,
        FileRole::Tooling => w::FileRole::Tooling,
    }
}

fn from_wit_role(role: w::FileRole) -> FileRole {
    match role {
        w::FileRole::Production => FileRole::Production,
        w::FileRole::Test => FileRole::Test,
        w::FileRole::Tooling => FileRole::Tooling,
    }
}

fn to_wit_origin(origin: FileOrigin) -> w::FileOrigin {
    match origin {
        FileOrigin::Authored => w::FileOrigin::Authored,
        FileOrigin::Generated => w::FileOrigin::Generated,
        FileOrigin::Vendored => w::FileOrigin::Vendored,
    }
}

fn from_wit_origin(origin: w::FileOrigin) -> FileOrigin {
    match origin {
        w::FileOrigin::Authored => FileOrigin::Authored,
        w::FileOrigin::Generated => FileOrigin::Generated,
        w::FileOrigin::Vendored => FileOrigin::Vendored,
    }
}

// Table-driven rather than a match-per-variant, same shape `host.rs` settled on for its own
// conversion tables: a flat match this wide reads as more cyclomatic risk than a straight 1:1
// enum mirror actually carries, and `crap` has no way to tell the difference without a
// coverage report. Keyed by the WIT (Copy) side so the table itself is a plain `const` slice —
// `SymbolKind::Other(SmolStr)` carries data and isn't `Copy`, so the lookup runs the other
// direction (linear search by `PartialEq`) instead of an indexed table.
const SYMBOL_KIND_TABLE: &[(w::SymbolKind, SymbolKind)] = &[
    (w::SymbolKind::Function, SymbolKind::Function),
    (w::SymbolKind::Method, SymbolKind::Method),
    (w::SymbolKind::Class, SymbolKind::Class),
    (w::SymbolKind::Interface, SymbolKind::Interface),
    (w::SymbolKind::Struct, SymbolKind::Struct),
    (w::SymbolKind::Enum, SymbolKind::Enum),
    (w::SymbolKind::EnumMember, SymbolKind::EnumMember),
    (w::SymbolKind::TypeAlias, SymbolKind::TypeAlias),
    (w::SymbolKind::Const, SymbolKind::Const),
    (w::SymbolKind::Static, SymbolKind::Static),
    (w::SymbolKind::Variable, SymbolKind::Variable),
    (w::SymbolKind::Field, SymbolKind::Field),
    (w::SymbolKind::Module, SymbolKind::Module),
];

fn to_wit_symbol_kind(kind: SymbolKind) -> w::SymbolKind {
    // v1 has no wire representation for an adapter-specific facet (`Other`/CSS-only kinds) —
    // falling through to the table's default is the same conservative-mapping posture the
    // visibility ladder uses elsewhere (over-approximate, never fabricate).
    SYMBOL_KIND_TABLE
        .iter()
        .find(|(_, native)| *native == kind)
        .map_or(w::SymbolKind::Variable, |(wit, _)| *wit)
}

fn from_wit_root_kind(kind: w::RootKind) -> RootKind {
    match kind {
        w::RootKind::Production => RootKind::Production,
        w::RootKind::Test => RootKind::Test,
        w::RootKind::Tooling => RootKind::Tooling,
    }
}

const REF_KIND_TABLE: &[(w::RefKind, RefKind)] = &[
    (w::RefKind::Call, RefKind::Call),
    (w::RefKind::Read, RefKind::Read),
    (w::RefKind::Write, RefKind::Write),
    (w::RefKind::Extend, RefKind::Extend),
    (w::RefKind::Implement, RefKind::Implement),
    (w::RefKind::Override, RefKind::Override),
    (w::RefKind::TypeUse, RefKind::TypeUse),
];

/// Every row above is exhaustive by construction (one per WIT enum variant) — a miss here can
/// only mean this file and `wit/plugin.wit` have drifted, not something a well-formed
/// component could trigger at runtime.
fn from_wit_ref_kind(kind: w::RefKind) -> RefKind {
    REF_KIND_TABLE
        .iter()
        .find(|(wit, _)| *wit == kind)
        .unwrap_or_else(|| {
            panic!("kndo-plugin-api: missing RefKind table row for a WIT enum variant")
        })
        .1
}

fn from_wit_confidence(confidence: w::Confidence) -> kndo_core::vocab::Confidence {
    match confidence {
        w::Confidence::Certain => kndo_core::vocab::Confidence::Certain,
        w::Confidence::Probable => kndo_core::vocab::Confidence::Probable,
        w::Confidence::Possible => kndo_core::vocab::Confidence::Possible,
    }
}

fn from_wit_target(target: w::PluginTarget) -> PluginTarget {
    match target.symbol {
        Some(name) => PluginTarget::symbol(ProjectPath(SmolStr::new(&target.path)), name),
        None => PluginTarget::file(ProjectPath(SmolStr::new(&target.path))),
    }
}
