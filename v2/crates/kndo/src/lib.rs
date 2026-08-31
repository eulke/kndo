//! The facade. Frontends — the CLI, `kndo serve`, anything that renders results —
//! import `kndo::<Name>` and nothing deeper; when a frontend needs data or logic this
//! root does not export, that is a change to core, never a local re-derivation. This
//! crate also owns composition: [`default_extensions`] is the ONE list of everything
//! a stock kndo run speaks and runs — languages and conduct alike, one door.

pub use kndo_contract::adapter::{ResolveContext, SourceFile};
pub use kndo_contract::evidence::{DiagnosticLevel, RootKind};
pub use kndo_contract::extension::{
    Extension, ExtensionSpec, ExtensionSpecBuilder, GraphAccess, MutatesGraph,
};
pub use kndo_contract::finding::{Finding, Severity, sort_findings};
pub use kndo_contract::subject::{FindingId, Subject, SymbolSelector};
pub use kndo_contract::vocab::{Category, Confidence, ProjectPath, Span, SubjectKind};
pub use kndo_core::{
    Abstention, AbstentionReason, AbstentionScope, Activation, ActivationRule, CONTENT_MAX_BYTES,
    CONTENT_MAX_FILES, CategoryCount, ConductSeverity, ConductSink, ConductTarget, Config,
    ContentView, Contribution, ExtensionRun, GatePolicy, Graph, GraphView, Health, PhaseTimings,
    REPORT_SCHEMA, Refusal, Report, ReportDiagnostic, RuleDescriptor, RunInfo, RunMode, RunOutcome,
    Session, Snapshot, SuppressedSummary, Threads, is_reserved_coordinate,
};

use kndo_adapter_go::GoAdapter;
use kndo_adapter_java::JavaAdapter;
use kndo_adapter_kotlin::KotlinAdapter;
use kndo_adapter_python::PythonAdapter;
use kndo_adapter_rust::RustAdapter;
use kndo_adapter_swift::SwiftAdapter;
use kndo_adapter_ts::TypeScriptAdapter;
use kndo_coverage::LcovPlugin;

/// Everything a stock run is, in deterministic registration order: claim priority
/// among claiming extensions, and — among conduct-declaring ones — coverage
/// precedence and contribution order. The `builtin_plugin_proofs` gate closes over
/// this list's conduct subset: a conducting coordinate shipped without its
/// baseline-then-plugin proof fails the suite.
pub fn default_extensions() -> Vec<Box<dyn Extension>> {
    vec![
        Box::new(TypeScriptAdapter::new()),
        Box::new(RustAdapter::new()),
        Box::new(GoAdapter::new()),
        Box::new(JavaAdapter::new()),
        Box::new(KotlinAdapter::new()),
        Box::new(PythonAdapter::new()),
        Box::new(SwiftAdapter::new()),
        Box::new(LcovPlugin),
    ]
}

/// A session over `root` with the default extension set — the one-call
/// entry frontends start from. Under the `wasm` feature (the build shell), the
/// composition also loads external components from `<root>/.kndo/plugins/`.
pub fn open(root: impl Into<std::path::PathBuf>, config: Config) -> Result<Session, Refusal> {
    let root = root.into();
    #[cfg_attr(not(feature = "wasm"), allow(unused_mut))]
    let mut extensions = default_extensions();
    #[cfg(feature = "wasm")]
    let diagnostics = external::load_into(&root, &mut extensions);
    #[cfg(not(feature = "wasm"))]
    let diagnostics = Vec::new();
    Ok(Session::open(root, config, extensions)?.with_composition_diagnostics(diagnostics))
}

#[cfg(feature = "wasm")]
mod external {
    //! `.kndo/plugins/*.wasm`: presence is the opt-in, and there is ONE load
    //! path — a component states everything it does in its spec, so the loader
    //! never guesses. External extensions are SECOND in every ordering on
    //! purpose (an external cannot steal a built-in language's claims; the
    //! built-in ingester keeps first-answer precedence), their activation is
    //! evaluated by the same rules as every extension's, and a component that
    //! fails to load degrades to a Warn diagnostic on every report the session
    //! produces: an opted-in component silently vanishing would hide exactly
    //! the mistake the channel exists to show.

    use kndo_contract::evidence::DiagnosticLevel;
    use kndo_contract::extension::Extension;
    use kndo_contract::vocab::ProjectPath;
    use kndo_core::ReportDiagnostic;
    use kndo_host_wasm::WasmExtension;
    use std::path::Path;

    pub(crate) fn load_into(
        root: &Path,
        extensions: &mut Vec<Box<dyn Extension>>,
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
            match WasmExtension::load(&path) {
                Ok(extension) => extensions.push(Box::new(extension)),
                Err(kndo_host_wasm::LoadError::ReservedCoordinate { coordinate }) => {
                    diagnostics.push(ReportDiagnostic {
                        path: report_path,
                        level: DiagnosticLevel::Warn,
                        message: format!(
                            "rejected: the component claims the reserved `kndo:` coordinate \
                             namespace (`{coordinate}`) — built-ins are native; an external \
                             coordinate names its own provenance"
                        ),
                    });
                }
                Err(e) => diagnostics.push(ReportDiagnostic {
                    path: report_path,
                    level: DiagnosticLevel::Warn,
                    message: format!(
                        "not a loadable kndo:vocab extension component — skipped: {e}"
                    ),
                }),
            }
        }
        diagnostics
    }
}
