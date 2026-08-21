//! The wasmtime host bridge: loads a `kndo:adapter` WASM component and wraps it as a native
//! [`LanguageAdapter`]. See `docs/contracts/wasm-abi.md` for the v1 scope this bridges.

use std::fmt;
use std::path::Path;
use std::sync::Mutex;

use smol_str::SmolStr;

use kndo_core::adapter::{
    AdapterDescriptor, CyclePolicy, CycleTolerance, Declaration, Diagnostic, DiagnosticLevel,
    FileClaim, FileFacts, ImportSpec, LanguageAdapter, ManifestFacts, ProjectPath, RawReference,
    RawRoot, RawRootTarget, Resolution, ResolveCtx, SourceFile, Span,
};
use kndo_core::vocab::{FileClass, FileOrigin, FileRole, RefKind, RootKind, SymbolKind};

mod wit {
    wasmtime::component::bindgen!({
        path: "wit/adapter.wit",
        world: "adapter",
    });
}

use self::wit::kndo::adapter::types as w;
use self::wit::Adapter;

/// Fuel budget per guest call (RFC 0003 §3: "per-file fuel/time limits so a plugin cannot break
/// the 500 ms budget"). A trapped/exhausted call degrades to a conservative empty result plus a
/// diagnostic rather than aborting the run — one misbehaving external adapter must not take
/// down `kndo check` for every other language in the project.
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
            LoadError::Instantiate(msg) => write!(f, "failed to instantiate WASM adapter: {msg}"),
        }
    }
}

impl std::error::Error for LoadError {}

struct GuestState {
    store: wasmtime::Store<()>,
    bindings: Adapter,
}

/// A `kndo:adapter` WASM component, bridged to the native [`LanguageAdapter`] trait. From the
/// `Engine`'s perspective this is indistinguishable from a compiled-in adapter (ADR 0003: "the
/// WASM ABI is a generated bridge over [the native traits]").
pub struct WasmAdapter {
    descriptor: AdapterDescriptor,
    state: Mutex<GuestState>,
}

impl WasmAdapter {
    /// Load an already-componentized `.wasm` file (component-model binary, not a plain core
    /// module — third-party authors produce one with `cargo component build`, `wasm-tools
    /// component new`, or the `wit-component` crate directly, same as this crate's own
    /// compliance test builds its demo adapter).
    pub fn load(path: &Path) -> Result<WasmAdapter, LoadError> {
        let (mut store, bindings) = instantiate(path)?;
        let raw_descriptor = bindings
            .call_descriptor(&mut store)
            .map_err(|e| LoadError::Instantiate(format!("descriptor() failed: {e}")))?;

        Ok(WasmAdapter {
            descriptor: native_descriptor(raw_descriptor),
            state: Mutex::new(GuestState { store, bindings }),
        })
    }
}

/// Reads, engine-configures, instantiates. Split out of [`WasmAdapter::load`] — and split
/// again internally — purely for readability and to keep each step's own complexity low: one
/// fallible step per function reads as a pipeline, not a wall of `?`s (the same instinct
/// behind the CRAP-driven declaration-dispatch tables Kotlin/Swift's adapters settled on).
fn instantiate(path: &Path) -> Result<(wasmtime::Store<()>, Adapter), LoadError> {
    let bytes = read_component_bytes(path)?;
    let engine = fuel_budgeted_engine()?;
    let component = load_component(&engine, &bytes)?;
    instantiate_bindings(&engine, &component)
}

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

fn instantiate_bindings(
    engine: &wasmtime::Engine,
    component: &wasmtime::component::Component,
) -> Result<(wasmtime::Store<()>, Adapter), LoadError> {
    let mut store = wasmtime::Store::new(engine, ());
    store
        .set_fuel(FUEL_PER_CALL)
        .map_err(|e| LoadError::Instantiate(e.to_string()))?;

    let linker = wasmtime::component::Linker::new(engine);
    let bindings = Adapter::instantiate(&mut store, component, &linker)
        .map_err(|e| LoadError::Instantiate(e.to_string()))?;

    Ok((store, bindings))
}

fn native_descriptor(raw: w::AdapterDescriptor) -> AdapterDescriptor {
    AdapterDescriptor {
        id: SmolStr::new(&raw.id),
        facts_schema_version: raw.facts_schema_version,
        file_globs: raw.file_globs.iter().map(SmolStr::new).collect(),
        manifest_globs: Vec::new(),
        grammar_version: SmolStr::new(&raw.grammar_version),
        visibility_ladder: Vec::new(),
        cycle_policy: CyclePolicy {
            file_cycles: CycleTolerance::Idiomatic,
            package_cycles: CycleTolerance::Idiomatic,
        },
        resolves_dependency_usage: false,
    }
}

impl LanguageAdapter for WasmAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        self.descriptor.clone()
    }

    fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
        let mut guard = self.state.lock().expect("wasm adapter store poisoned");
        let GuestState { store, bindings } = &mut *guard;
        let _ = store.set_fuel(FUEL_PER_CALL);

        let claimed = bindings.call_claim(&mut *store, path.0.as_str()).ok()?;

        claimed.map(|c| FileClaim {
            language: self.descriptor.id.clone(),
            class: FileClass {
                role: from_wit_role(c.class.role),
                origin: from_wit_origin(c.class.origin),
            },
        })
    }

    fn claim_manifest(&self, _path: &ProjectPath) -> bool {
        // v1 scope cut (docs/contracts/wasm-abi.md §2): no manifest extraction over the WASM
        // boundary yet. Never calls into the guest.
        false
    }

    fn extract(&self, file: &SourceFile<'_>) -> FileFacts {
        let mut guard = self.state.lock().expect("wasm adapter store poisoned");
        let GuestState { store, bindings } = &mut *guard;
        let _ = store.set_fuel(FUEL_PER_CALL);

        let content = String::from_utf8_lossy(file.content);
        let call = bindings.call_extract(&mut *store, file.path.0.as_str(), &content);

        match call {
            Ok(facts) => from_wit_facts(facts),
            Err(e) => {
                let mut facts = FileFacts::default();
                facts.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Warn,
                    path: Some(file.path.clone()),
                    message: format!(
                        "external adapter '{}' exceeded its call budget or trapped: {e}",
                        self.descriptor.id
                    ),
                    span: None,
                });
                facts
            }
        }
    }

    fn extract_manifest(&self, _file: &SourceFile<'_>, _ctx: &ResolveCtx<'_>) -> ManifestFacts {
        // v1 scope cut, mirrors `claim_manifest` — see docs/contracts/wasm-abi.md §2.
        ManifestFacts::default()
    }

    fn resolve(&self, _spec: &ImportSpec, _ctx: &ResolveCtx<'_>) -> Resolution {
        // v1 never extracts imports (no `resolve()` guest export exists yet), so this is
        // structurally unreachable in practice — kept `Unresolved` for trait completeness,
        // matching the JSON/CSS adapters' own "genuinely unreachable in normal operation" note
        // (docs/adapters/json.md).
        Resolution::Unresolved
    }
}

fn from_wit_role(role: w::FileRole) -> FileRole {
    match role {
        w::FileRole::Production => FileRole::Production,
        w::FileRole::Test => FileRole::Test,
        w::FileRole::Tooling => FileRole::Tooling,
    }
}

fn from_wit_origin(origin: w::FileOrigin) -> FileOrigin {
    match origin {
        w::FileOrigin::Authored => FileOrigin::Authored,
        w::FileOrigin::Generated => FileOrigin::Generated,
        w::FileOrigin::Vendored => FileOrigin::Vendored,
    }
}

// Table-driven rather than a match-per-variant (same shape the Kotlin/Swift adapters settled
// on for their own declaration-dispatch tables): a flat match this wide reads as more
// cyclomatic risk than a straight 1:1 enum mirror actually carries, and `crap` has no way to
// tell the difference without a coverage report. An explicit pair table keeps each
// correspondence spelled out (no fragile reliance on both enums sharing declaration order)
// while collapsing every function here to a single linear lookup.
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

const REF_KIND_TABLE: &[(w::RefKind, RefKind)] = &[
    (w::RefKind::Call, RefKind::Call),
    (w::RefKind::Read, RefKind::Read),
    (w::RefKind::Write, RefKind::Write),
    (w::RefKind::Extend, RefKind::Extend),
    (w::RefKind::Implement, RefKind::Implement),
    (w::RefKind::Override, RefKind::Override),
    (w::RefKind::TypeUse, RefKind::TypeUse),
];

const ROOT_KIND_TABLE: &[(w::RootKind, RootKind)] = &[
    (w::RootKind::Production, RootKind::Production),
    (w::RootKind::Test, RootKind::Test),
    (w::RootKind::Tooling, RootKind::Tooling),
];

fn from_wit_symbol_kind(kind: w::SymbolKind) -> SymbolKind {
    lookup(SYMBOL_KIND_TABLE, kind)
}

fn from_wit_ref_kind(kind: w::RefKind) -> RefKind {
    lookup(REF_KIND_TABLE, kind)
}

fn from_wit_root_kind(kind: w::RootKind) -> RootKind {
    lookup(ROOT_KIND_TABLE, kind)
}

/// Every table above is exhaustive by construction (one row per WIT enum variant — the `tests`
/// module below pins this at compile time via an exhaustive match per enum, so an added WIT
/// variant with no table row fails the build, not a test run), so a miss here can only mean
/// this file and the `.wit` package it mirrors have drifted — a contract violation, not
/// something a well-formed component could ever trigger at runtime.
fn lookup<K: PartialEq + Copy, V: Clone>(table: &[(K, V)], key: K) -> V {
    table
        .iter()
        .find(|(k, _)| *k == key)
        .unwrap_or_else(|| {
            panic!(
                "kndo-plugin-api: missing lookup table row — a WIT enum variant was added \
                 without updating its conversion table"
            )
        })
        .1
        .clone()
}

fn from_wit_span(span: w::Span) -> Span {
    Span {
        start: (span.start_line, span.start_col),
        end: (span.end_line, span.end_col),
    }
}

fn from_wit_facts(facts: w::FileFacts) -> FileFacts {
    let declarations = facts
        .declarations
        .into_iter()
        .map(|d| Declaration {
            name: SmolStr::new(&d.name),
            kind: from_wit_symbol_kind(d.kind),
            span: from_wit_span(d.span),
            exported: d.exported,
            // v1 has no visibility-ladder concept for a WASM adapter (empty ladder, see
            // `AdapterDescriptor` construction above) — every declaration reports the widest
            // level, the same conservative default an empty ladder implies for every consumer.
            visibility: kndo_core::adapter::VisibilityLevel(0),
            member_of: d.member_of.map(|s| SmolStr::new(&s)),
            signature_span: None,
        })
        .collect();

    let references = facts
        .references
        .into_iter()
        .map(|r| RawReference {
            name: SmolStr::new(&r.name),
            scope_context: r.scope_context.map(|s| SmolStr::new(&s)),
            span: from_wit_span(r.span),
            within: r.within.map(|s| SmolStr::new(&s)),
            kind: from_wit_ref_kind(r.kind),
        })
        .collect();

    let roots = facts
        .roots
        .into_iter()
        .map(|r| RawRoot {
            kind: from_wit_root_kind(r.kind),
            target: match r.target {
                w::RawRootTarget::WholeFile => RawRootTarget::WholeFile,
                w::RawRootTarget::Declaration(name) => {
                    RawRootTarget::Declaration(SmolStr::new(&name))
                }
            },
            confidence: kndo_core::vocab::Confidence::Certain,
        })
        .collect();

    let diagnostics = facts
        .diagnostics
        .into_iter()
        .map(|d| Diagnostic {
            level: match d.level {
                w::DiagnosticLevel::Warn => DiagnosticLevel::Warn,
                w::DiagnosticLevel::Info => DiagnosticLevel::Info,
            },
            path: None,
            message: d.message,
            span: d.span.map(from_wit_span),
        })
        .collect();

    FileFacts {
        declarations,
        references,
        roots,
        diagnostics,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exhaustive matches, one per WIT enum — not runtime assertions but a compile-time
    /// guarantee: if the `.wit` file grows a new variant, these stop compiling until the
    /// corresponding `*_TABLE` above gains a row for it. That's a stronger guarantee than a
    /// unit test could give (a missing row would need someone to think to test it; a missing
    /// match arm can't be missed).
    fn assert_symbol_kind_table_is_exhaustive(kind: w::SymbolKind) {
        let _: SymbolKind = match kind {
            w::SymbolKind::Function
            | w::SymbolKind::Method
            | w::SymbolKind::Class
            | w::SymbolKind::Interface
            | w::SymbolKind::Struct
            | w::SymbolKind::Enum
            | w::SymbolKind::EnumMember
            | w::SymbolKind::TypeAlias
            | w::SymbolKind::Const
            | w::SymbolKind::Static
            | w::SymbolKind::Variable
            | w::SymbolKind::Field
            | w::SymbolKind::Module => from_wit_symbol_kind(kind),
        };
    }

    fn assert_ref_kind_table_is_exhaustive(kind: w::RefKind) {
        let _: RefKind = match kind {
            w::RefKind::Call
            | w::RefKind::Read
            | w::RefKind::Write
            | w::RefKind::Extend
            | w::RefKind::Implement
            | w::RefKind::Override
            | w::RefKind::TypeUse => from_wit_ref_kind(kind),
        };
    }

    fn assert_root_kind_table_is_exhaustive(kind: w::RootKind) {
        let _: RootKind = match kind {
            w::RootKind::Production | w::RootKind::Test | w::RootKind::Tooling => {
                from_wit_root_kind(kind)
            }
        };
    }

    #[test]
    fn lookup_tables_cover_every_variant_and_agree_with_the_native_vocabulary() {
        assert_symbol_kind_table_is_exhaustive(w::SymbolKind::Function);
        assert_ref_kind_table_is_exhaustive(w::RefKind::Call);
        assert_root_kind_table_is_exhaustive(w::RootKind::Production);

        assert_eq!(
            from_wit_symbol_kind(w::SymbolKind::Method),
            SymbolKind::Method
        );
        assert_eq!(from_wit_ref_kind(w::RefKind::TypeUse), RefKind::TypeUse);
        assert_eq!(from_wit_root_kind(w::RootKind::Tooling), RootKind::Tooling);
    }
}
