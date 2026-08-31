//! The two-cluster reference extension — the framework case the old taxonomy
//! could not hold in one component. Cluster one, EXTRACTION: it owns its own
//! file format (`.acme` route files — `handler <name>` lines) and roots them,
//! language-style. Cluster two, CONDUCT: it activates on the project's manifest
//! naming `acme-framework`, CHAINS `demo:probe` through `dependencies` (the
//! indirect-framework path: probe's own rules never need to match), and
//! contributes a root of its own. One spec, one component, both doors.

use kndo_contract::adapter::SourceFile;
use kndo_contract::evidence::{
    EvidenceSink, EvidenceStreams, Reach, RootKind, RootTarget, SymbolKind,
};
use kndo_contract::extension::{
    Activation, ActivationRule, ConductSink, ContentAccess, Extension, ExtensionSpec, GraphAccess,
    MutatesGraph, ConductTarget,
};
use kndo_contract::vocab::{Confidence, ProjectPath, Span};
use std::sync::LazyLock;

static SPEC: LazyLock<ExtensionSpec> = LazyLock::new(|| {
    ExtensionSpec::builder("acme:framework", 1)
        .suffixes(&["acme"])
        .emits(EvidenceStreams::none())
        .conduct(
            Activation::AnyRule(vec![ActivationRule::ManifestDependency(
                "acme-framework".into(),
            )]),
            MutatesGraph::Yes,
        )
        .dependencies(&["demo:probe"])
        .build()
});

#[derive(Default)]
struct AcmeFramework;

impl Extension for AcmeFramework {
    fn spec(&self) -> &ExtensionSpec {
        &SPEC
    }

    /// A route file is live by virtue of being wired into the framework: a
    /// whole-file production root, plus one exported declaration per handler.
    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        out.root(
            RootTarget::WholeFile,
            RootKind::Production,
            Confidence::Certain,
        );
        let text = std::str::from_utf8(file.content).unwrap_or("");
        let mut offset = 0u32;
        for raw in text.split_inclusive('\n') {
            let line = raw.trim_end_matches('\n');
            if let Some(name) = line.strip_prefix("handler ") {
                let span = Span::new(offset, offset + line.len() as u32);
                out.declaration(name.trim(), SymbolKind::Function, span, Reach::Exported);
            }
            offset += raw.len() as u32;
        }
    }

    /// The framework knows `di_wired` is DI-registered when its file exists —
    /// symbol-level liveness the language cannot see, contributed from the same
    /// component that also speaks a language.
    fn contribute_roots(
        &self,
        graph: &dyn GraphAccess,
        _content: &dyn ContentAccess,
        out: &mut ConductSink,
    ) {
        if graph.contains(&ProjectPath::new("extra.kmini")) {
            out.root(
                ConductTarget::Symbol {
                    path: ProjectPath::new("extra.kmini"),
                    name: "di_wired".into(),
                },
                RootKind::Production,
                Confidence::Certain,
            );
        }
    }
}

kndo_sdk::export_extension!(AcmeFramework);
