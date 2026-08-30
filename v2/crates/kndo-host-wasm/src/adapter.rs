//! A loaded component as a [`LanguageAdapter`]. The engine sees one more adapter:
//! same spec, same cache keys, same claim wiring — a WASM language is a first-class
//! citizen because nothing downstream can tell.

use crate::LoadError;
use crate::bindings::adapter::kndo::vocab::types as awire;
use crate::bindings::adapter::{Adapter, AdapterImports};
use crate::convert;
use crate::engine::{budgeted_store, guest_limits, shared_engine};
use kndo_contract::adapter::{
    AdapterSpec, LanguageAdapter, PackageEntry, ProjectRoot, Resolution, ResolveContext, SourceFile,
};
use kndo_contract::evidence::{DiagnosticLevel, EvidenceSink};
use kndo_contract::vocab::ProjectPath;
use smol_str::SmolStr;
use std::path::Path;
use wasmtime::component::{Component, Linker};

/// What every guest call's store carries: the project enumerations the world
/// imports answer from, and the memory limiter. Extraction populates nothing —
/// every call gets a fresh instance, so an enumeration answered empty during
/// extraction can never poison a later resolution.
pub(crate) struct AdapterStoreData {
    files: Vec<String>,
    packages: Vec<awire::PackageEntry>,
    limits: wasmtime::StoreLimits,
}

impl AdapterStoreData {
    fn empty() -> Self {
        AdapterStoreData {
            files: Vec::new(),
            packages: Vec::new(),
            limits: guest_limits(),
        }
    }

    fn of(cx: &ResolveContext<'_>) -> Self {
        AdapterStoreData {
            files: cx.known_files().map(|p| p.as_str().to_string()).collect(),
            packages: cx.packages().map(convert::package_entry_to_wire).collect(),
            limits: guest_limits(),
        }
    }
}

impl awire::Host for AdapterStoreData {}

impl AdapterImports for AdapterStoreData {
    fn known_files(&mut self) -> Vec<String> {
        self.files.clone()
    }

    fn package_entries(&mut self) -> Vec<awire::PackageEntry> {
        self.packages.clone()
    }
}

pub struct WasmAdapter {
    component: Component,
    linker: Linker<AdapterStoreData>,
    spec: AdapterSpec,
}

impl WasmAdapter {
    /// Load a component targeting the adapter world; its spec is read once and
    /// cached — a spec is folded into evidence cache keys, so it must be stable
    /// for the process, and the one call at load is what makes it so.
    pub fn load(path: &Path) -> Result<WasmAdapter, LoadError> {
        let bytes = std::fs::read(path)?;
        let component = Component::from_binary(shared_engine(), &bytes)
            .map_err(|e| LoadError::Component(e.to_string()))?;
        let mut linker: Linker<AdapterStoreData> = Linker::new(shared_engine());
        Adapter::add_to_linker(&mut linker, |data| data)
            .map_err(|e| LoadError::Component(e.to_string()))?;

        let mut store = budgeted_store(AdapterStoreData::empty());
        store.limiter(|d| &mut d.limits);
        let guest = Adapter::instantiate(&mut store, &component, &linker)
            .map_err(|e| LoadError::Component(e.to_string()))?;
        let spec = guest
            .call_spec(&mut store)
            .map_err(|e| LoadError::Component(e.to_string()))?;
        Ok(WasmAdapter {
            component,
            linker,
            spec: convert::adapter_spec(spec),
        })
    }

    /// A fresh, budgeted instance for one call. `Err` is a trap, fuel or memory
    /// exhaustion — every caller degrades it toward keep-alive.
    fn call<T>(
        &self,
        data: AdapterStoreData,
        f: impl FnOnce(&Adapter, &mut wasmtime::Store<AdapterStoreData>) -> wasmtime::Result<T>,
    ) -> wasmtime::Result<T> {
        let mut store = budgeted_store(data);
        store.limiter(|d| &mut d.limits);
        let guest = Adapter::instantiate(&mut store, &self.component, &self.linker)?;
        f(&guest, &mut store)
    }
}

impl LanguageAdapter for WasmAdapter {
    fn spec(&self) -> &AdapterSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        let result = self.call(AdapterStoreData::empty(), |guest, store| {
            guest.call_extract(store, file.path.as_str(), file.content)
        });
        match result {
            Ok(evidence) => {
                // Replayed through a second sink under the spec's declared
                // streams, then copied into the engine's — the pairing rule and
                // every clamp apply to the wire exactly as to native writes.
                let replayed = convert::replay_evidence(
                    evidence,
                    file.content.len() as u32,
                    self.spec.emits().clone(),
                );
                copy_into(replayed, out);
            }
            Err(_) => out.diagnostic(
                DiagnosticLevel::Warn,
                "component trapped or exhausted its budget — no evidence extracted \
                 from this file",
                None,
            ),
        }
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        self.call(AdapterStoreData::of(cx), |guest, store| {
            guest.call_resolve(store, from.as_str(), specifier)
        })
        .map(convert::resolution)
        .unwrap_or(Resolution::Unresolved)
    }

    fn roots(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<ProjectRoot> {
        self.call(AdapterStoreData::of(cx), |guest, store| {
            guest.call_roots(store, manifest.path.as_str(), manifest.content)
        })
        .map(|roots| {
            roots
                .into_iter()
                .map(|r| ProjectRoot {
                    file: ProjectPath::new(r.file),
                    kind: convert::root_kind(r.kind),
                    confidence: convert::confidence(r.confidence),
                })
                .collect()
        })
        .unwrap_or_default()
    }

    fn packages(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<PackageEntry> {
        self.call(AdapterStoreData::of(cx), |guest, store| {
            guest.call_packages(store, manifest.path.as_str(), manifest.content)
        })
        .map(|entries| entries.into_iter().map(convert::package_entry).collect())
        .unwrap_or_default()
    }

    fn manifest_dependencies(&self, manifest: &SourceFile<'_>) -> Vec<SmolStr> {
        self.call(AdapterStoreData::empty(), |guest, store| {
            guest.call_manifest_dependencies(store, manifest.path.as_str(), manifest.content)
        })
        .map(|names| names.into_iter().map(SmolStr::new).collect())
        .unwrap_or_default()
    }

    fn unit_mates(&self, path: &ProjectPath, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
        self.call(AdapterStoreData::of(cx), |guest, store| {
            guest.call_unit_mates(store, path.as_str())
        })
        .map(|mates| mates.into_iter().map(ProjectPath::new).collect())
        .unwrap_or_default()
    }
}

/// Copy replayed evidence into the engine's sink. The replay already validated
/// and clamped everything, so this is a faithful transcription — ids are re-issued
/// in the same order and land identically.
fn copy_into(evidence: kndo_contract::evidence::FileEvidence, out: &mut EvidenceSink) {
    let ids: Vec<_> = evidence
        .declarations
        .iter()
        .map(|d| out.declaration(d.name.clone(), d.kind.clone(), d.span, d.reach))
        .collect();
    for (ix, d) in evidence.declarations.iter().enumerate() {
        if let Some(owner) = d.owner {
            out.member_of(ids[ix], ids[owner.index()]);
        }
        if let Some(alias) = &d.exported_as {
            out.exported_as(ids[ix], alias.clone());
        }
    }
    for r in evidence.references {
        out.reference(r.name, r.kind, r.span);
    }
    for i in evidence.imports {
        out.import(i.target, i.shape, i.span, i.confidence);
    }
    for r in evidence.roots {
        let target = match r.target {
            kndo_contract::evidence::RootTarget::WholeFile => {
                kndo_contract::evidence::RootTarget::WholeFile
            }
            kndo_contract::evidence::RootTarget::Declaration(id) => {
                kndo_contract::evidence::RootTarget::Declaration(ids[id.index()])
            }
            other => other,
        };
        out.root(target, r.kind, r.confidence);
    }
    for c in evidence.comments {
        out.comment(c.span, c.text);
    }
    for (id, m) in evidence.metrics {
        out.metrics(ids[id.index()], m);
    }
    for d in evidence.diagnostics {
        out.diagnostic(d.level, d.message, d.span);
    }
}
