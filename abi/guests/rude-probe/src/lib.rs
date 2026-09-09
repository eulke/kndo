//! The deliberately misbehaving guest — HAND-ROLLED against the raw world (no
//! SDK), because that is the one tier where phase discipline is not structural:
//! its `extract` export calls the conduct-phase `graph-paths` import, and the
//! compliance suite asserts the host answers with a named phase-violation trap
//! that surfaces as a diagnostic, never as data.

mod bindings {
    wit_bindgen::generate!({
        path: "../../../wit",
        world: "plugin",
    });
}

use bindings::kndo::vocab::types as wire;

struct RudeProbe;

impl bindings::Guest for RudeProbe {
    fn spec() -> wire::PluginSpec {
        wire::PluginSpec {
            coordinate: "demo:rude".to_string(),
            version: 1,
            suffixes: vec!["rude".to_string()],
            ladder: Vec::new(),
            import_cycles: wire::CycleTolerance::Tolerated,
            dispatch: Vec::new(),
            claims: vec!["**/*.rude".to_string()],
            emits: Vec::new(),
            manifests: Vec::new(),
            launchers: Vec::new(),
            ignores: Vec::new(),
            unnamed_unit: wire::UnnamedUnit::Unbounded,
            nesting: wire::Nesting::PerFile,
            file_roles: Vec::new(),
            dependency_scoping: wire::DependencyScoping::Scoped,
            dependency_identity: wire::DependencyIdentity::Underivable,
            dependency_importers: Vec::new(),
            dependency_builtins: wire::DependencyBuiltins::None,
            ecosystem: None,
            hidden_opt_in: Vec::new(),
            conducts: false,
            activation: wire::Activation::Always,
            mutates_graph: false,
            dependencies: Vec::new(),
            requested_file_access: Vec::new(),
            rules: Vec::new(),
            reads_reports: Vec::new(),
        }
    }

    fn extract(
        _path: String,
        content: Vec<u8>,
        _region: Option<wire::EmbeddedRegion>,
    ) -> wire::FileEvidence {
        // The misbehavior under test, chosen by the claimed file's content: a
        // conduct import during extraction, or a PROJECT enumeration during
        // extraction (evidence caches by content alone, so the file set is
        // off-limits here too). The host must trap either — the call never
        // returns.
        if content.starts_with(b"files") {
            let _ = bindings::known_files();
        } else {
            let _ = bindings::graph_paths();
        }
        wire::FileEvidence {
            namespace: Vec::new(),
            attachment: wire::Attachment::Regular,
            declarations: Vec::new(),
            references: Vec::new(),
            imports: Vec::new(),
            roots: Vec::new(),
            markers: Vec::new(),
            relations: Vec::new(),
            comments: Vec::new(),
            metrics: Vec::new(),
            embedded: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn resolve(_from: String, _specifier: String) -> wire::Resolution {
        wire::Resolution::Unresolved
    }

    fn extract_manifest(_manifest_path: String, content: Vec<u8>) -> wire::ManifestEvidence {
        // A manifest read runs in the PROJECT phase — it resolves entries, so
        // the project enumerations are its own. The conduct imports are not:
        // reaching for the assembled graph here must trap as a named
        // violation, never answer with an empty snapshot as if it were one.
        if content.starts_with(b"graph") {
            let _ = bindings::graph_paths();
        }
        wire::ManifestEvidence {
            units: Vec::new(),
            packages: Vec::new(),
            dependencies: Vec::new(),
            mentions: Vec::new(),
            aliases: Vec::new(),
            roots: Vec::new(),
            ignores: Vec::new(),
            members: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn contribute_roots() -> Vec<wire::ContributedRoot> {
        Vec::new()
    }

    fn report_findings() -> Vec<wire::ContributedFinding> {
        Vec::new()
    }

    fn ingest(_path: String, _content: Vec<u8>) -> Option<wire::CoverageRecords> {
        None
    }
}

bindings::export!(RudeProbe with_types_in bindings);
