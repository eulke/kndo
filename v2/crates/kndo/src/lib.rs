//! The facade. Frontends — the CLI, `kndo serve`, anything that renders results —
//! import `kndo::<Name>` and nothing deeper; when a frontend needs data or logic this
//! root does not export, that is a change to core, never a local re-derivation. This
//! crate also owns composition: [`default_adapters`] and [`default_plugins`] are the
//! one list each of what a stock kndo run speaks and runs.

pub use kndo_contract::adapter::{AdapterSpec, LanguageAdapter};
pub use kndo_contract::evidence::RootKind;
pub use kndo_contract::finding::{Finding, Severity, sort_findings};
pub use kndo_contract::subject::{FindingId, Subject, SymbolSelector};
pub use kndo_contract::vocab::{Category, Confidence, ProjectPath, Span, SubjectKind};
pub use kndo_core::{
    Abstention, AbstentionReason, AbstentionScope, Activation, ActivationRule, CONTENT_MAX_BYTES,
    CONTENT_MAX_FILES, Config, ContentView, GatePolicy, Graph, GraphView, PhaseTimings, Plugin,
    PluginContribution, PluginSeverity, PluginSink, PluginSpec, PluginTarget, Refusal, Report,
    RuleDescriptor, RunMode, RunOutcome, Session, Snapshot, SuppressedSummary, Threads, WellKnown,
    is_reserved_coordinate,
};
pub use kndo_coverage::{Coverage, FileCoverage};

use kndo_adapter_go::GoAdapter;
use kndo_adapter_rust::RustAdapter;
use kndo_adapter_ts::TypeScriptAdapter;
use kndo_plugin_coverage::LcovPlugin;

/// Every language a stock run speaks, in deterministic registration order.
pub fn default_adapters() -> Vec<Box<dyn LanguageAdapter>> {
    vec![
        Box::new(TypeScriptAdapter::new()),
        Box::new(RustAdapter::new()),
        Box::new(GoAdapter::new()),
    ]
}

/// Every plugin a stock run carries, in registration order — which is also
/// coverage-ingestion precedence. The `builtin_plugin_proofs` gate closes over this
/// list: a coordinate added here without a baseline-then-plugin proof fails the suite.
pub fn default_plugins() -> Vec<Box<dyn Plugin>> {
    vec![Box::new(LcovPlugin)]
}

/// A session over `root` with the default adapter and plugin sets — the one-call
/// entry frontends start from. Under the `wasm` feature (the build shell), the
/// composition also loads external components from `<root>/.kndo/plugins/`.
pub fn open(root: impl Into<std::path::PathBuf>, config: Config) -> Result<Session, Refusal> {
    let root = root.into();
    let mut adapters = default_adapters();
    let mut plugins = default_plugins();
    #[cfg(feature = "wasm")]
    let diagnostics = external::load_into(&root, &mut adapters, &mut plugins);
    #[cfg(not(feature = "wasm"))]
    let diagnostics = Vec::new();
    Ok(Session::open(root, config, adapters)?
        .with_plugins(plugins)
        .with_composition_diagnostics(diagnostics))
}

#[cfg(feature = "wasm")]
mod external {
    //! `.kndo/plugins/*.wasm`: presence is the opt-in; each file joins the
    //! composition as whichever world it targets. External components are
    //! SECOND in every ordering on purpose — an external adapter cannot steal a
    //! built-in language's claims, and the built-in coverage ingester keeps
    //! first-answer precedence — and their activation is evaluated by the same
    //! rules as every plugin's (`Always` is the spelling for "just run").
    //! A component that fails to load degrades to a Warn diagnostic on every
    //! report the session produces: an opted-in component silently vanishing
    //! would hide exactly the mistake the channel exists to show.

    use kndo_contract::adapter::LanguageAdapter;
    use kndo_contract::evidence::DiagnosticLevel;
    use kndo_contract::vocab::ProjectPath;
    use kndo_core::{Plugin, ReportDiagnostic};
    use kndo_host_wasm::{WasmAdapter, WasmIngester, WasmPlugin};
    use std::path::Path;

    pub(crate) fn load_into(
        root: &Path,
        adapters: &mut Vec<Box<dyn LanguageAdapter>>,
        plugins: &mut Vec<Box<dyn Plugin>>,
    ) -> Vec<ReportDiagnostic> {
        let dir = root.join(".kndo/plugins");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut components: Vec<_> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "wasm"))
            .collect();
        components.sort();

        let mut diagnostics = Vec::new();
        for path in components {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let report_path = ProjectPath::new(format!(".kndo/plugins/{name}"));
            // A component targets exactly one world; worlds are tried in a fixed
            // order and the first that instantiates wins. A definitive rejection
            // (the reserved `kndo:` namespace) stops the ladder — the component
            // loaded fine and was refused on identity, not shape.
            match WasmAdapter::load(&path) {
                Ok(adapter) => {
                    adapters.push(Box::new(adapter));
                    continue;
                }
                Err(kndo_host_wasm::LoadError::ReservedCoordinate { coordinate }) => {
                    diagnostics.push(reserved(report_path, &coordinate));
                    continue;
                }
                Err(_) => {}
            }
            match WasmPlugin::load(&path) {
                Ok(plugin) => {
                    plugins.push(Box::new(plugin));
                    continue;
                }
                Err(kndo_host_wasm::LoadError::ReservedCoordinate { coordinate }) => {
                    diagnostics.push(reserved(report_path, &coordinate));
                    continue;
                }
                Err(_) => {}
            }
            match WasmIngester::load(&path) {
                Ok(ingester) => {
                    plugins.push(Box::new(ingester));
                }
                Err(kndo_host_wasm::LoadError::ReservedCoordinate { coordinate }) => {
                    diagnostics.push(reserved(report_path, &coordinate));
                }
                Err(e) => diagnostics.push(ReportDiagnostic {
                    path: report_path,
                    level: DiagnosticLevel::Warn,
                    message: format!(
                        "not loadable as any kndo:vocab world \
                         (adapter, plugin, coverage-ingester) — skipped: {e}"
                    ),
                }),
            }
        }
        diagnostics
    }

    fn reserved(path: ProjectPath, coordinate: &str) -> ReportDiagnostic {
        ReportDiagnostic {
            path,
            level: DiagnosticLevel::Warn,
            message: format!(
                "rejected: the component claims the reserved `kndo:` coordinate \
                 namespace (`{coordinate}`) — built-ins are native; an external \
                 coordinate names its own provenance"
            ),
        }
    }
}
