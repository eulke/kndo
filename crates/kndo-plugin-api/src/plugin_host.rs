//! The wasmtime host bridge for the `kndo:plugin` WASM ABI (`docs/contracts/wasm-abi.md` §5) —
//! bridges the four `kndo_core::plugin::Plugin` graph-mutation hooks. Unlike the adapter bridge
//! (`host.rs`), this world is bidirectional: the guest calls back into two host-provided query
//! functions (`list-files`, `symbols-in`) while computing its contributions.

use std::fmt;
use std::path::Path;
use std::sync::Mutex;

use kndo_core::adapter::ProjectPath;
use kndo_core::plugin::{
    AnnotationSink, ContentView, EdgeSink, GraphView, Plugin, PluginDescriptor, PluginTarget,
    RootSink,
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

/// Same discipline as the adapter bridge (RFC 0003 §3): a trapped/exhausted hook degrades to
/// "contributed nothing" rather than aborting the run.
use crate::engine::FUEL_PER_CALL;

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
#[derive(Default)]
struct HostViewData {
    files: Vec<w::WasmFileInfo>,
    symbols_by_file: rustc_hash::FxHashMap<smol_str::SmolStr, Vec<w::WasmSymbolInfo>>,
    // RFC 0016 §5: every path this component's declared globs matched, fetched and budget-
    // charged against the caller's `ContentView` *before* instantiation — the guest can't make
    // a host round-trip of its own choosing mid-call the way a native plugin calls
    // `ContentView::read` directly, so `read-file` just serves a lookup into this snapshot.
    content_by_path: rustc_hash::FxHashMap<String, Vec<u8>>,
    // RFC 0017 §5's read surface, snapshotted for the same borrow reason as everything above
    // (wasm-abi §5.3): the store's state must be 'static, so what a query might answer is
    // cloned per round — bounded O(files + symbols + edges + content bytes), the documented
    // acceptance. Every projection is adapter-derived only (rule R1) and pre-sorted.
    packages: Vec<w::WasmPackageInfo>,
    file_details: rustc_hash::FxHashMap<String, w::WasmFileDetails>,
    symbol_details: rustc_hash::FxHashMap<String, Vec<(String, w::WasmSymbolDetails)>>,
    imports_of: rustc_hash::FxHashMap<String, Vec<String>>,
    importers_of: rustc_hash::FxHashMap<String, Vec<String>>,
    ref_sites: rustc_hash::FxHashMap<(String, String), Vec<w::WasmRefSite>>,
    call_sites: rustc_hash::FxHashMap<String, Vec<w::WasmCallSite>>,
}

fn to_wit_span(span: kndo_core::adapter::Span) -> w::WasmSpan {
    w::WasmSpan {
        start_line: span.start.0,
        start_col: span.start.1,
        end_line: span.end.0,
        end_col: span.end.1,
    }
}

fn to_wit_package(p: &kndo_core::plugin::PackageView<'_>) -> w::WasmPackageInfo {
    w::WasmPackageInfo {
        manifest: p.manifest.map(|m| m.0.to_string()),
        name: p.name.map(str::to_string),
        root: p.root_dir.to_string(),
    }
}

impl HostViewData {
    fn empty() -> Self {
        HostViewData::default()
    }

    fn from_view(graph: &GraphView<'_>, content: &ContentView<'_>) -> Self {
        let mut data = HostViewData {
            packages: graph.packages().map(|p| to_wit_package(&p)).collect(),
            ..HostViewData::default()
        };
        for file in graph.files() {
            collect_file_projections(graph, file, &mut data);
            collect_symbol_projections(graph, file, &mut data);
        }
        collect_ref_sites(graph, &mut data);
        for path in content.matching_paths() {
            if let Some(bytes) = content.read(path) {
                data.content_by_path.insert(path.0.to_string(), bytes);
            }
        }
        data
    }
}

/// File-level projections (RFC 0017 §5.1–§5.4): details, both import directions, call sites.
/// Answers exist for unclaimed files too — a template can be asked about even though it has
/// no symbols.
fn collect_file_projections(
    graph: &GraphView<'_>,
    file: &kndo_core::graph::FileNode,
    data: &mut HostViewData,
) {
    let path = file.path.0.to_string();
    data.file_details.insert(
        path.clone(),
        w::WasmFileDetails {
            language: file.language.as_ref().map(|l| l.to_string()),
            unit: file.unit.as_ref().map(|u| u.to_string()),
            package_root: graph
                .package_of(&file.path)
                .map(|p| p.root_dir.to_string())
                .unwrap_or_default(),
        },
    );
    let imports: Vec<String> = graph
        .imports_of(&file.path)
        .into_iter()
        .map(|p| p.0.to_string())
        .collect();
    if !imports.is_empty() {
        data.imports_of.insert(path.clone(), imports);
    }
    let importers: Vec<String> = graph
        .importers_of(&file.path)
        .into_iter()
        .map(|p| p.0.to_string())
        .collect();
    if !importers.is_empty() {
        data.importers_of.insert(path.clone(), importers);
    }
    let call_sites: Vec<w::WasmCallSite> = graph
        .string_call_sites_in(&file.path)
        .iter()
        .map(|c| w::WasmCallSite {
            callee: c.callee.to_string(),
            literal: c.literal.to_string(),
            span: to_wit_span(c.span),
        })
        .collect();
    if !call_sites.is_empty() {
        data.call_sites.insert(path, call_sites);
    }
}

/// The frozen v1 file/symbol records plus §5.1's per-symbol details — claimed files only
/// (an unclaimed file structurally has no symbols).
fn collect_symbol_projections(
    graph: &GraphView<'_>,
    file: &kndo_core::graph::FileNode,
    data: &mut HostViewData,
) {
    let Some(class) = file.class else {
        return;
    };
    data.files.push(w::WasmFileInfo {
        path: file.path.0.to_string(),
        role: to_wit_role(class.role),
        origin: to_wit_origin(class.origin),
    });
    let mut symbols = Vec::new();
    let mut details = Vec::new();
    for s in graph.symbols_in(&file.path) {
        symbols.push(w::WasmSymbolInfo {
            name: s.name.to_string(),
            kind: to_wit_symbol_kind(s.kind.clone()),
            exported: s.exported,
            member_of: s.member_of.as_ref().map(|m| m.to_string()),
        });
        details.push((
            s.name.to_string(),
            w::WasmSymbolDetails {
                visibility: s.visibility.0 as u32,
                span: to_wit_span(s.span),
            },
        ));
    }
    data.symbols_by_file.insert(file.path.0.clone(), symbols);
    data.symbol_details.insert(file.path.0.to_string(), details);
}

/// §5.3's `references-to` projection, keyed by (target path, target bare name), sites sorted.
fn collect_ref_sites(graph: &GraphView<'_>, data: &mut HostViewData) {
    for (target_path, target_name, site) in graph.all_reference_sites() {
        data.ref_sites
            .entry((target_path.0.to_string(), target_name.to_string()))
            .or_default()
            .push(w::WasmRefSite {
                from_path: site.from_path.0.to_string(),
                from_symbol: site.from_symbol.map(str::to_string),
                kind: to_wit_ref_kind_out(site.kind),
                confidence: to_wit_confidence_out(site.confidence),
            });
    }
    for sites in data.ref_sites.values_mut() {
        sites.sort_by(|a, b| (&a.from_path, &a.from_symbol).cmp(&(&b.from_path, &b.from_symbol)));
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

    fn packages(&mut self) -> Vec<w::WasmPackageInfo> {
        self.packages.clone()
    }

    fn package_of(&mut self, path: String) -> Option<w::WasmPackageInfo> {
        // Ownership is total for known paths (RFC 0011 §3); an unknown path is a plain miss.
        let details = self.file_details.get(path.as_str())?;
        let root = details.package_root.clone();
        self.packages.iter().find(|p| p.root == root).cloned()
    }

    fn file_details(&mut self, path: String) -> Option<w::WasmFileDetails> {
        self.file_details.get(path.as_str()).cloned()
    }

    fn symbol_details(&mut self, path: String, symbol: String) -> Option<w::WasmSymbolDetails> {
        self.symbol_details
            .get(path.as_str())?
            .iter()
            .find(|(name, _)| *name == symbol)
            .map(|(_, d)| *d)
    }

    fn imports_of(&mut self, path: String) -> Vec<String> {
        self.imports_of
            .get(path.as_str())
            .cloned()
            .unwrap_or_default()
    }

    fn importers_of(&mut self, path: String) -> Vec<String> {
        self.importers_of
            .get(path.as_str())
            .cloned()
            .unwrap_or_default()
    }

    fn references_to(&mut self, path: String, symbol: String) -> Vec<w::WasmRefSite> {
        self.ref_sites
            .get(&(path, symbol))
            .cloned()
            .unwrap_or_default()
    }

    fn call_sites_in(&mut self, path: String) -> Vec<w::WasmCallSite> {
        self.call_sites
            .get(path.as_str())
            .cloned()
            .unwrap_or_default()
    }

    fn read_file(&mut self, path: String) -> Option<Vec<u8>> {
        self.content_by_path.get(&path).cloned()
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
    // RFC 0016 §6: computed once at load, over the raw component bytes — the graph cache key's
    // proof that *this exact* `.wasm` file, not just its self-declared id/version, produced
    // whatever the last snapshot recorded.
    content_hash: [u8; 32],
    // One instance per graph-mutation ROUND (RFC 0017 §4), not per hook: `contribute_roots` —
    // the round's first hook in declaration order — (re)instantiates against that round's
    // fresh `HostViewData` snapshot; `contribute_edges`/`annotate_symbols` reuse it, and the
    // instance is dropped when `annotate_symbols` returns. Guest state deliberately persists
    // across the three hooks of one round and structurally cannot survive into the next.
    linker: wasmtime::component::Linker<HostViewData>,
    round_instance: Mutex<Option<GuestState>>,
}

impl WasmPlugin {
    pub fn load(path: &Path) -> Result<WasmPlugin, LoadError> {
        let bytes = read_component_bytes(path)?;
        let content_hash = *blake3::hash(&bytes).as_bytes();
        let (engine, component, linker) = build_runtime(&bytes)?;
        let descriptor = probe_descriptor(&engine, &component, &linker)?;

        Ok(WasmPlugin {
            engine,
            component,
            descriptor,
            content_hash,
            linker,
            round_instance: Mutex::new(None),
        })
    }

    /// (Re)instantiate the component against a fresh snapshot of `graph`/`content`, replacing
    /// whatever instance is currently live — how every round begins (RFC 0017 §4).
    fn refresh_instance(&self, graph: &GraphView<'_>, content: &ContentView<'_>) -> Option<()> {
        let (store, bindings) = instantiate_with(
            &self.engine,
            &self.component,
            &self.linker,
            HostViewData::from_view(graph, content),
        )
        .ok()?;
        *self
            .round_instance
            .lock()
            .expect("wasm plugin store poisoned") = Some(GuestState { store, bindings });
        Some(())
    }

    /// Make sure a round instance is live without discarding one that already is — the reuse
    /// path for the round's later hooks. A hook invoked with no live instance (a caller
    /// driving the trait out of the core's roots → edges → annotate order) instantiates
    /// defensively against ITS OWN view rather than ever touching another round's state.
    fn ensure_instance(&self, graph: &GraphView<'_>, content: &ContentView<'_>) -> bool {
        if self
            .round_instance
            .lock()
            .expect("wasm plugin store poisoned")
            .is_some()
        {
            return true;
        }
        self.refresh_instance(graph, content).is_some()
    }
}

/// Reads, engine-configures, links, instantiates — split for the same reason as the adapter
/// bridge's own `instantiate` pipeline in `host.rs`: one fallible step per function keeps each
/// step's own complexity low instead of one long wall of `?`s.
fn read_component_bytes(path: &Path) -> Result<Vec<u8>, LoadError> {
    std::fs::read(path).map_err(LoadError::Io)
}

fn fuel_budgeted_engine() -> Result<wasmtime::Engine, LoadError> {
    // The process-wide shared engine (crate::engine): identical config for both bridges,
    // one JIT code cache, and the disk compilation cache enabled — `Engine` is a cheap
    // Arc-backed handle, so cloning it here keeps this function's signature.
    Ok(crate::engine::shared_engine().clone())
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
    // RFC 0015 §2: the `kndo:` namespace is reserved for built-ins — an external component
    // claiming it fails to load, exactly like an instantiation error (skipped by discovery,
    // never trusted). This is what keeps `dependencies: ["kndo:nextjs"]` unambiguous from any
    // source: nothing external can ever *be* `kndo:nextjs`.
    if kndo_core::plugin::is_reserved_id(&raw.id) {
        return Err(LoadError::Instantiate(format!(
            "descriptor claims reserved built-in id '{}' (the kndo: namespace is not claimable \
             by external plugins — RFC 0015 §2)",
            raw.id
        )));
    }
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
        dependencies: raw.dependencies.iter().map(SmolStr::new).collect(),
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

    fn content_hash(&self) -> Option<[u8; 32]> {
        Some(self.content_hash)
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

    // The round lifecycle (RFC 0017 §4): `contribute_roots` opens the round with a fresh
    // instance, `contribute_edges` reuses it, `annotate_symbols` reuses it and closes the
    // round by dropping it. Fuel is re-armed to `FUEL_PER_CALL` before every hook call, so
    // the per-call budget semantics are identical to the old instance-per-hook model — a
    // heavy `contribute_roots` can't starve `annotate_symbols`.
    fn contribute_roots(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut RootSink,
    ) {
        if self.refresh_instance(graph, content).is_none() {
            return;
        }
        let mut guard = self
            .round_instance
            .lock()
            .expect("wasm plugin store poisoned");
        let Some(GuestState { store, bindings }) = guard.as_mut() else {
            return;
        };
        if store.set_fuel(FUEL_PER_CALL).is_err() {
            return;
        }
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

    fn contribute_edges(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut EdgeSink,
    ) {
        if !self.ensure_instance(graph, content) {
            return;
        }
        let mut guard = self
            .round_instance
            .lock()
            .expect("wasm plugin store poisoned");
        let Some(GuestState { store, bindings }) = guard.as_mut() else {
            return;
        };
        if store.set_fuel(FUEL_PER_CALL).is_err() {
            return;
        }
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

    fn annotate_symbols(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut AnnotationSink,
    ) {
        if !self.ensure_instance(graph, content) {
            return;
        }
        let mut guard = self
            .round_instance
            .lock()
            .expect("wasm plugin store poisoned");
        let Some(GuestState { store, bindings }) = guard.as_mut() else {
            return;
        };
        if store.set_fuel(FUEL_PER_CALL).is_err() {
            return;
        }
        let result = bindings.call_annotate_symbols(&mut *store);
        // End of round, success or not: the instance never survives into the next one.
        *guard = None;
        let Ok(targets) = result else {
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
/// [`from_wit_ref_kind`]'s inverse, derived from it rather than written as a second
/// hand-maintained match: the wire enum below enumerates every variant once, and the round
/// trip through the one authoritative mapping guarantees the two directions can never drift.
const WIRE_REF_KINDS: [w::RefKind; 7] = [
    w::RefKind::Call,
    w::RefKind::Read,
    w::RefKind::Write,
    w::RefKind::Extend,
    w::RefKind::Implement,
    w::RefKind::Override,
    w::RefKind::TypeUse,
];

fn to_wit_ref_kind_out(kind: RefKind) -> w::RefKind {
    *WIRE_REF_KINDS
        .iter()
        .find(|wire| from_wit_ref_kind(**wire) == kind)
        .expect("both enums enumerate the same seven variants")
}

fn to_wit_confidence_out(confidence: kndo_core::vocab::Confidence) -> w::Confidence {
    match confidence {
        kndo_core::vocab::Confidence::Certain => w::Confidence::Certain,
        kndo_core::vocab::Confidence::Probable => w::Confidence::Probable,
        kndo_core::vocab::Confidence::Possible => w::Confidence::Possible,
    }
}

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
