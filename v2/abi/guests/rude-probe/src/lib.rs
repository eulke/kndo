//! The deliberately misbehaving guest — HAND-ROLLED against the raw world (no
//! SDK), because that is the one tier where phase discipline is not structural:
//! its `extract` export calls the conduct-phase `graph-paths` import, and the
//! compliance suite asserts the host answers with a named phase-violation trap
//! that surfaces as a diagnostic, never as data.

mod bindings {
    wit_bindgen::generate!({
        path: "../../../wit",
        world: "extension",
    });
}

use bindings::kndo::vocab::types as wire;

struct RudeProbe;

impl bindings::Guest for RudeProbe {
    fn spec() -> wire::ExtensionSpec {
        wire::ExtensionSpec {
            coordinate: "demo:rude".to_string(),
            version: 1,
            suffixes: vec!["rude".to_string()],
            narrowable_scopes: Vec::new(),
            claims: vec!["**/*.rude".to_string()],
            emits: Vec::new(),
            manifests: Vec::new(),
            conducts: false,
            activation: wire::Activation::Always,
            mutates_graph: false,
            dependencies: Vec::new(),
            requested_file_access: Vec::new(),
            rules: Vec::new(),
            reads_reports: Vec::new(),
        }
    }

    fn extract(_path: String, content: Vec<u8>) -> wire::FileEvidence {
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
            declarations: Vec::new(),
            references: Vec::new(),
            imports: Vec::new(),
            roots: Vec::new(),
            comments: Vec::new(),
            metrics: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn resolve(_from: String, _specifier: String) -> wire::Resolution {
        wire::Resolution::Unresolved
    }

    fn roots(_manifest_path: String, _content: Vec<u8>) -> Vec<wire::ProjectRoot> {
        Vec::new()
    }

    fn packages(_manifest_path: String, _content: Vec<u8>) -> Vec<wire::PackageEntry> {
        Vec::new()
    }

    fn manifest_dependencies(_manifest_path: String, content: Vec<u8>) -> Vec<String> {
        // Same discipline for the manifest hook: bytes in, names out — reaching
        // for the file set here must trap as a named violation, never read an
        // empty snapshot as if it were the project.
        if content.starts_with(b"files") {
            let _ = bindings::known_files();
        }
        Vec::new()
    }

    fn sees(_path: String) -> Vec<String> {
        Vec::new()
    }

    fn seen_from(_path: String, _scope: String) -> Option<Vec<String>> {
        None
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
