//! The wasmtime host bridge: loads a `kndo:adapter` WASM component and wraps it as a native
//! [`LanguageAdapter`]. The v1 scope is one-directional: file claiming and extraction only.

use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use smol_str::SmolStr;

use crate::engine::{shared_engine, FUEL_PER_CALL};

use kndo_core::adapter::{
    AdapterDescriptor, AdapterDiagnostic, CyclePolicy, CycleTolerance, Declaration,
    DiagnosticLevel, FileClaim, FileFacts, ImportSpec, LanguageAdapter, ManifestFacts, ProjectPath,
    RawReference, RawRoot, RawRootTarget, Resolution, ResolveCtx, SourceFile, Span,
};
use kndo_core::vocab::{FileClass, FileOrigin, FileRole, RefKind, RootKind, SymbolKind};

mod bindings {
    wasmtime::component::bindgen!({
        path: "wit/adapter.wit",
        world: "adapter",
    });
}

use self::bindings::kndo::adapter::types as w;
use self::bindings::Adapter;

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
/// `Engine`'s perspective this is indistinguishable from a compiled-in adapter (the
/// WASM ABI is a generated bridge over the native traits) — including under parallelism:
/// `claim`/`extract` run on a *pool* of guest instances, one checked out per concurrent call,
/// so rayon's parallel extraction phase (graph.rs phase 1) parallelizes a WASM adapter's files
/// exactly as it does a compiled-in adapter's. The compiled [`Component`] is shared; an
/// instance is a cheap instantiation of it (µs against the shared engine), created on demand
/// when the pool is empty, returned after the call, and *discarded* after a trap — a trapped
/// instance's state is not something any later call should inherit.
///
/// Sound because the contract requires it: `extract` must be a pure function of
/// `(path, content)` — the facts cache serves any file's facts from any prior run's
/// extraction, so a guest depending on cross-call instance state is broken by contract.
/// The pool makes that implication normative.
pub struct WasmAdapter {
    descriptor: AdapterDescriptor,
    component: wasmtime::component::Component,
    pool: Mutex<Vec<GuestState>>,
    instances_created: AtomicUsize,
}

impl WasmAdapter {
    /// Load an already-componentized `.wasm` file (component-model binary, not a plain core
    /// module — third-party authors produce one with `kndo plugin build`, `cargo component
    /// build`, or the `wit-component` crate directly, same as this crate's own compliance
    /// test builds its demo adapter).
    pub fn load(path: &Path) -> Result<WasmAdapter, LoadError> {
        let bytes = std::fs::read(path).map_err(LoadError::Io)?;
        let component = wasmtime::component::Component::from_binary(shared_engine(), &bytes)
            .map_err(|e| LoadError::Instantiate(e.to_string()))?;
        let (first, raw_descriptor) = probe_descriptor(&component)?;
        Ok(WasmAdapter {
            descriptor: native_descriptor(raw_descriptor),
            component,
            pool: Mutex::new(vec![first]),
            instances_created: AtomicUsize::new(1),
        })
    }

    /// A ready instance: the pool's, or a fresh instantiation of the shared compiled
    /// component when every pooled instance is checked out by a concurrent call. The pool
    /// therefore grows to the actual concurrency level and no further.
    fn checkout(&self) -> Result<GuestState, LoadError> {
        if let Some(state) = self.pool.lock().expect("wasm adapter pool poisoned").pop() {
            return Ok(state);
        }
        let state = instantiate_bindings(&self.component)?;
        self.instances_created.fetch_add(1, Ordering::Relaxed);
        Ok(state)
    }

    fn checkin(&self, state: GuestState) {
        self.pool
            .lock()
            .expect("wasm adapter pool poisoned")
            .push(state);
    }

    /// How many guest instances this adapter has ever instantiated — 1 until concurrent
    /// calls force pool growth. Observability for the parallel-extraction test; not API.
    #[doc(hidden)]
    pub fn instances_created(&self) -> usize {
        self.instances_created.load(Ordering::Relaxed)
    }
}

/// Instantiate once and read + vet the descriptor. The `kndo:` namespace is
/// reserved for built-ins, the same enforcement the plugin bridge applies — a
/// component external to this build cannot claim to be `kndo:go` or any other coordinate no
/// external source could have been fetched from. Skipped-not-fatal, same as any other load
/// failure. The probing instance seeds the pool — never a throwaway.
fn probe_descriptor(
    component: &wasmtime::component::Component,
) -> Result<(GuestState, w::AdapterDescriptor), LoadError> {
    let mut first = instantiate_bindings(component)?;
    let raw = first
        .bindings
        .call_descriptor(&mut first.store)
        .map_err(|e| LoadError::Instantiate(format!("descriptor() failed: {e}")))?;
    if kndo_core::plugin::is_reserved_id(&raw.id) {
        return Err(LoadError::Instantiate(format!(
            "descriptor claims reserved built-in id '{}' (the kndo: namespace is not \
             claimable by external adapters)",
            raw.id
        )));
    }
    Ok((first, raw))
}

fn instantiate_bindings(
    component: &wasmtime::component::Component,
) -> Result<GuestState, LoadError> {
    let engine = shared_engine();
    let mut store = wasmtime::Store::new(engine, ());
    store
        .set_fuel(FUEL_PER_CALL)
        .map_err(|e| LoadError::Instantiate(e.to_string()))?;

    let linker = wasmtime::component::Linker::new(engine);
    let bindings = Adapter::instantiate(&mut store, component, &linker)
        .map_err(|e| LoadError::Instantiate(e.to_string()))?;

    Ok(GuestState { store, bindings })
}

fn native_descriptor(raw: w::AdapterDescriptor) -> AdapterDescriptor {
    AdapterDescriptor {
        activation: raw
            .activation
            .into_iter()
            .map(from_wit_activation_rule)
            .collect(),
        // Dormant on both sides — rides the wire, unevaluated, so the reservation is
        // symmetric across tiers.
        dependencies: raw.dependencies.iter().map(SmolStr::new).collect(),
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
        // Not on the wire: the WIT descriptor record predates the field, and a WASM
        // adapter without it just forgoes package-relative test-dir promotion. Additive
        // whenever the ABI next revs.
        package_test_dirs: Vec::new(),
        // No builtin type facts yet: this adapter declares none, and an empty table
        // simply means the chain resolver has no second tier to consult for it.
        builtin_member_types: Vec::new(),
    }
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

impl LanguageAdapter for WasmAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        self.descriptor.clone()
    }

    fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
        // A checkout failure (instantiation refused mid-run) degrades to "not mine" — the
        // uniform miss behavior, never a panic in a parallel phase.
        let mut state = self.checkout().ok()?;
        let _ = state.store.set_fuel(FUEL_PER_CALL);
        let call = state.bindings.call_claim(&mut state.store, path.0.as_str());
        let claimed = match call {
            Ok(claimed) => {
                self.checkin(state);
                claimed?
            }
            // A trapped instance is dropped, not pooled — later calls must never inherit a
            // guest that died mid-call.
            Err(_) => return None,
        };
        Some(FileClaim {
            language: self.descriptor.id.clone(),
            class: FileClass {
                role: from_wit_role(claimed.class.role),
                origin: from_wit_origin(claimed.class.origin),
            },
        })
    }

    fn claim_manifest(&self, _path: &ProjectPath) -> bool {
        // v1 scope cut: no manifest extraction over the WASM boundary. Never calls into
        // the guest.
        false
    }

    fn extract(&self, file: &SourceFile<'_>) -> FileFacts {
        let mut state = match self.checkout() {
            Ok(state) => state,
            Err(e) => return error_facts(&self.descriptor.id, &e.to_string()),
        };
        let _ = state.store.set_fuel(FUEL_PER_CALL);

        let content = String::from_utf8_lossy(file.content);
        let call = state
            .bindings
            .call_extract(&mut state.store, file.path.0.as_str(), &content);

        match call {
            Ok(facts) => {
                self.checkin(state);
                from_wit_facts(facts)
            }
            // Trapped/exhausted instance dropped, not pooled (same reasoning as `claim`).
            Err(e) => error_facts(
                &self.descriptor.id,
                &format!("exceeded its call budget or trapped: {e}"),
            ),
        }
    }

    fn extract_manifest(&self, _file: &SourceFile<'_>, _ctx: &ResolveCtx<'_>) -> ManifestFacts {
        // v1 scope cut, mirrors `claim_manifest`.
        ManifestFacts::default()
    }

    fn resolve(&self, _spec: &ImportSpec, _ctx: &ResolveCtx<'_>) -> Resolution {
        // v1 never extracts imports (no `resolve()` guest export exists), so this is
        // structurally unreachable in practice — kept `Unresolved` for trait completeness,
        // matching the JSON adapter's own "structurally unreachable in normal operation"
        // note.
        Resolution::Unresolved
    }
}

/// The conservative empty result plus a diagnostic — one misbehaving external adapter must
/// not take down `kndo check` for every other language in the project. No path to attribute —
/// same as every other adapter's own diagnostics, the core fills that in from the file being
/// extracted when it ingests `FileFacts`.
fn error_facts(adapter_id: &str, detail: &str) -> FileFacts {
    let mut facts = FileFacts::default();
    facts.diagnostics.push(AdapterDiagnostic {
        level: DiagnosticLevel::Warn,
        message: format!("external adapter '{adapter_id}' {detail}"),
        span: None,
    });
    facts
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

// Table-driven rather than a match-per-variant (same shape the Kotlin/Swift adapters use
// for their own declaration-dispatch tables): a flat match this wide reads as more
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
            implicitly_invoked: false,
            nested_scope: false,
            visibility_inherited: false,
            visible_in_unit: None,
            markers: Vec::new(),
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
        .map(|d| AdapterDiagnostic {
            level: match d.level {
                w::DiagnosticLevel::Warn => DiagnosticLevel::Warn,
                w::DiagnosticLevel::Info => DiagnosticLevel::Info,
            },
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
