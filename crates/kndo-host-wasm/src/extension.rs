//! A loaded component as an [`Extension`] — THE bridge: one world, one load
//! path, no sniffing. The engine sees one more extension (same spec shape, same
//! cache keys, same claim wiring, same conduct containment), so a WASM extension
//! is a first-class citizen because nothing downstream can tell.
//!
//! Phase discipline is enforced here: every host import belongs to a phase, and
//! a call from the wrong one — `graph-paths` during extraction, `known-files`
//! during the conduct round — traps with a NAMED contract violation instead of
//! answering. Native code and SDK guests never reach this (their phase
//! discipline is structural: the hook signatures carry only what the phase
//! provides); the trap is the tier that covers hand-rolled bindings.

use crate::LoadError;
use crate::bindings::kndo::vocab::types as wire;
use crate::bindings::{Extension as GuestWorld, ExtensionImports};
use crate::convert;
use crate::engine::{budgeted_store, guest_limits, shared_engine};
use kndo_contract::adapter::{PackageEntry, ProjectRoot, Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::{CoverageRecords, DiagnosticLevel, EvidenceSink};
use kndo_contract::extension::is_reserved_coordinate;
use kndo_contract::extension::{ConductSink, ContentAccess, Extension, ExtensionSpec, GraphAccess};
use kndo_contract::vocab::ProjectPath;
use std::collections::BTreeMap;
use std::path::Path;
use wasmtime::component::{Component, Linker};

/// Which phase a store was built for — the authority every import checks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// Manifest-dependency reads: bytes in, names out, no project surface.
    Manifest,
    Spec,
    Extract,
    Project,
    Conduct,
    Ingest,
}

impl Phase {
    fn describe(self) -> &'static str {
        match self {
            Phase::Spec => "spec load",
            Phase::Manifest => "the manifest read",
            Phase::Extract => "extraction",
            Phase::Project => "project queries",
            Phase::Conduct => "the conduct round",
            Phase::Ingest => "ingestion",
        }
    }
}

/// One guest call's store: the phase it runs in, the data that phase provides,
/// and the memory limiter. Every call gets a fresh instance, so an enumeration
/// answered in one call can never poison another.
struct StoreData {
    phase: Phase,
    known_files: Vec<String>,
    packages: Vec<wire::PackageEntry>,
    graph_paths: Vec<String>,
    graph_declarations: Vec<wire::DeclaredSymbol>,
    readable: Vec<String>,
    snapshot: BTreeMap<String, Vec<u8>>,
    violation: Option<String>,
    limits: wasmtime::StoreLimits,
}

impl StoreData {
    fn bare(phase: Phase) -> Self {
        StoreData {
            phase,
            known_files: Vec::new(),
            packages: Vec::new(),
            graph_paths: Vec::new(),
            graph_declarations: Vec::new(),
            readable: Vec::new(),
            snapshot: BTreeMap::new(),
            violation: None,
            limits: guest_limits(),
        }
    }

    fn project(cx: &ResolveContext<'_>) -> Self {
        StoreData {
            known_files: cx.known_files().map(|p| p.as_str().to_string()).collect(),
            packages: cx.packages().map(convert::package_entry_to_wire).collect(),
            ..StoreData::bare(Phase::Project)
        }
    }

    /// The conduct round's read surface, snapshotted before instantiation: the
    /// graph's paths, and every declared-glob content match the engine's own
    /// view allowed — prefetched THROUGH it, so globs and the budget are charged
    /// by the one authority. A WASM extension's content budget is charged by
    /// declaration, not by demand; the cut still lands on the contribution
    /// because the view remembers.
    fn conduct(graph: &dyn GraphAccess, content: &dyn ContentAccess) -> Self {
        let mut snapshot = BTreeMap::new();
        let mut readable = Vec::new();
        for path in content.readable_paths() {
            if let Some(bytes) = content.read(path) {
                readable.push(path.as_str().to_string());
                snapshot.insert(path.as_str().to_string(), bytes.to_vec());
            }
        }
        StoreData {
            graph_paths: graph.paths().map(|p| p.as_str().to_string()).collect(),
            graph_declarations: graph
                .declarations()
                .map(convert::declared_symbol_to_wire)
                .collect(),
            readable,
            snapshot,
            ..StoreData::bare(Phase::Conduct)
        }
    }

    fn gate(&mut self, import: &'static str, allowed: Phase) -> wasmtime::Result<()> {
        if self.phase == allowed {
            return Ok(());
        }
        let message = format!(
            "phase contract violation: import `{import}` belongs to {} and was called during {}",
            allowed.describe(),
            self.phase.describe()
        );
        self.violation = Some(message.clone());
        Err(wasmtime::Error::msg(message))
    }
}

impl wire::Host for StoreData {}

impl ExtensionImports for StoreData {
    fn known_files(&mut self) -> wasmtime::Result<Vec<String>> {
        self.gate("known-files", Phase::Project)?;
        Ok(self.known_files.clone())
    }

    fn package_entries(&mut self) -> wasmtime::Result<Vec<wire::PackageEntry>> {
        self.gate("package-entries", Phase::Project)?;
        Ok(self.packages.clone())
    }

    fn graph_paths(&mut self) -> wasmtime::Result<Vec<String>> {
        self.gate("graph-paths", Phase::Conduct)?;
        Ok(self.graph_paths.clone())
    }

    fn graph_contains(&mut self, path: String) -> wasmtime::Result<bool> {
        self.gate("graph-contains", Phase::Conduct)?;
        Ok(self.graph_paths.binary_search(&path).is_ok())
    }

    fn graph_declarations(&mut self) -> wasmtime::Result<Vec<wire::DeclaredSymbol>> {
        self.gate("graph-declarations", Phase::Conduct)?;
        Ok(self.graph_declarations.clone())
    }

    fn readable_paths(&mut self) -> wasmtime::Result<Vec<String>> {
        self.gate("readable-paths", Phase::Conduct)?;
        Ok(self.readable.clone())
    }

    fn read_file(&mut self, path: String) -> wasmtime::Result<Option<Vec<u8>>> {
        self.gate("read-file", Phase::Conduct)?;
        Ok(self.snapshot.get(&path).cloned())
    }
}

pub struct WasmExtension {
    component: Component,
    linker: Linker<StoreData>,
    spec: ExtensionSpec,
}

impl WasmExtension {
    /// Load a component targeting the extension world; the spec is read once and
    /// cached — a spec is folded into evidence cache keys, so it must be stable
    /// for the process. External components claiming the `kndo:` coordinate
    /// namespace are rejected here.
    pub fn load(path: &Path) -> Result<WasmExtension, LoadError> {
        let bytes = std::fs::read(path)?;
        let component = Component::from_binary(shared_engine(), &bytes)
            .map_err(|e| LoadError::Component(e.to_string()))?;
        let mut linker: Linker<StoreData> = Linker::new(shared_engine());
        GuestWorld::add_to_linker(&mut linker, |data| data)
            .map_err(|e| LoadError::Component(e.to_string()))?;

        let mut store = budgeted_store(StoreData::bare(Phase::Spec));
        store.limiter(|d| &mut d.limits);
        let guest = GuestWorld::instantiate(&mut store, &component, &linker)
            .map_err(|e| LoadError::Component(e.to_string()))?;
        let spec = guest
            .call_spec(&mut store)
            .map_err(|e| LoadError::Component(e.to_string()))?;
        let spec = convert::extension_spec(spec);
        if is_reserved_coordinate(spec.coordinate()) {
            return Err(LoadError::ReservedCoordinate {
                coordinate: spec.coordinate().to_string(),
            });
        }
        // Coordinates legally contain `/`; rule names must not, or two
        // (coordinate, rule) pairs could spell one `ext:` category — an identity
        // collision in a stability contract. Natives assert this at the builder;
        // the wire validates here, at the same door as the namespace check.
        if let Some(rule) = spec.rules().iter().find(|r| r.name.contains('/')) {
            return Err(LoadError::Component(format!(
                "rule name `{}` contains '/' — rule names must not (categories join \
                 coordinate and rule on '/')",
                rule.name
            )));
        }
        Ok(WasmExtension {
            component,
            linker,
            spec,
        })
    }

    /// A fresh, budgeted instance for one call. `Err` carries the phase
    /// violation when one fired, and `None` for an ordinary trap, fuel or
    /// memory exhaustion — every caller degrades it toward keep-alive.
    fn call<T>(
        &self,
        data: StoreData,
        f: impl FnOnce(&GuestWorld, &mut wasmtime::Store<StoreData>) -> wasmtime::Result<T>,
    ) -> Result<T, Option<String>> {
        let mut store = budgeted_store(data);
        store.limiter(|d| &mut d.limits);
        let guest =
            GuestWorld::instantiate(&mut store, &self.component, &self.linker).map_err(|_| None)?;
        match f(&guest, &mut store) {
            Ok(value) => Ok(value),
            Err(_) => Err(store.data_mut().violation.take()),
        }
    }
}

impl Extension for WasmExtension {
    fn spec(&self) -> &ExtensionSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        let region = file.region.map(convert::region_to_wire);
        let result = self.call(StoreData::bare(Phase::Extract), |guest, store| {
            guest.call_extract(store, file.path.as_str(), file.content, region.as_ref())
        });
        match result {
            // Replayed into the engine's own sink, primed with this spec's
            // declared streams — the pairing rule and every clamp apply to the
            // wire exactly as to native writes.
            Ok(evidence) => convert::replay_evidence(evidence, out),
            Err(violation) => out.diagnostic(
                DiagnosticLevel::Warn,
                violation.unwrap_or_else(|| {
                    "component trapped or exhausted its budget — no evidence extracted \
                     from this file"
                        .to_string()
                }),
                None,
            ),
        }
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        self.call(StoreData::project(cx), |guest, store| {
            guest.call_resolve(store, from.as_str(), specifier)
        })
        .map(convert::resolution)
        .unwrap_or(Resolution::Unresolved)
    }

    fn roots(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<ProjectRoot> {
        self.call(StoreData::project(cx), |guest, store| {
            guest.call_roots(store, manifest.path.as_str(), manifest.content)
        })
        .map(|roots| roots.into_iter().map(convert::project_root).collect())
        .unwrap_or_default()
    }

    fn packages(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<PackageEntry> {
        self.call(StoreData::project(cx), |guest, store| {
            guest.call_packages(store, manifest.path.as_str(), manifest.content)
        })
        .map(|entries| entries.into_iter().map(convert::package_entry).collect())
        .unwrap_or_default()
    }

    fn manifest_dependencies(
        &self,
        manifest: &SourceFile<'_>,
    ) -> Vec<kndo_contract::adapter::DependencyDeclaration> {
        // The manifest hook gets bytes and NOTHING else — its own phase, so a
        // guest reaching for `known-files` here trips a named violation instead
        // of silently reading an empty snapshot. The ABI speaks names only;
        // scope and requirement are honestly absent, so version-skew never
        // judges a guest-declared dependency it cannot compare.
        self.call(StoreData::bare(Phase::Manifest), |guest, store| {
            guest.call_manifest_dependencies(store, manifest.path.as_str(), manifest.content)
        })
        .map(|names| {
            names
                .into_iter()
                .map(|name| kndo_contract::adapter::DependencyDeclaration {
                    name: smol_str::SmolStr::new(name),
                    scope: None,
                    version_req: None,
                })
                .collect()
        })
        .unwrap_or_default()
    }

    fn seen_from(
        &self,
        path: &ProjectPath,
        reach: &kndo_contract::evidence::Reach,
        cx: &ResolveContext<'_>,
    ) -> Option<Vec<ProjectPath>> {
        let reach = crate::convert::reach_to_wire(reach);
        // A trap or violation degrades to None — Exported treatment, keep-alive.
        self.call(StoreData::project(cx), |guest, store| {
            guest.call_seen_from(store, path.as_str(), &reach)
        })
        .ok()
        .flatten()
        .map(|paths| paths.into_iter().map(ProjectPath::new).collect())
    }

    fn contribute_roots(
        &self,
        graph: &dyn GraphAccess,
        content: &dyn ContentAccess,
        out: &mut ConductSink,
    ) {
        let roots = match self.call(StoreData::conduct(graph, content), |guest, store| {
            guest.call_contribute_roots(store)
        }) {
            Ok(roots) => roots,
            Err(reason) => {
                out.note(trap_line("contribute-roots", reason));
                return;
            }
        };
        for root in roots {
            out.root(
                convert::conduct_target(root.target),
                convert::root_kind(root.kind),
                convert::confidence(root.confidence),
            );
        }
    }

    fn report_findings(
        &self,
        graph: &dyn GraphAccess,
        content: &dyn ContentAccess,
        out: &mut ConductSink,
    ) {
        let findings = match self.call(StoreData::conduct(graph, content), |guest, store| {
            guest.call_report_findings(store)
        }) {
            Ok(findings) => findings,
            Err(reason) => {
                out.note(trap_line("report-findings", reason));
                return;
            }
        };
        for finding in findings {
            out.finding(
                &finding.rule,
                convert::conduct_severity(finding.severity),
                convert::conduct_target(finding.target),
                convert::confidence(finding.confidence),
                finding.message,
            );
        }
    }

    fn ingest(&self, report_path: &str, content: &[u8]) -> Option<CoverageRecords> {
        self.call(StoreData::bare(Phase::Ingest), |guest, store| {
            guest.call_ingest(store, report_path, content)
        })
        .ok()
        .flatten()
        .map(convert::coverage_records)
    }
}

/// Replayed evidence into the engine's sink — the copy is index-faithful because
/// the replay sink issued ids in declaration order, same as this pass does.
/// The honesty line a trapped conduct call leaves on its contribution: the
/// violated gate's own words when one fired, or the anonymous-trap wording
/// (fuel, memory, a guest panic) — never silence.
fn trap_line(hook: &str, reason: Option<String>) -> String {
    match reason {
        Some(v) => format!("{hook} call refused: {v}"),
        None => format!(
            "{hook} call trapped (guest panic, fuel or memory exhaustion) — its contribution is lost"
        ),
    }
}
