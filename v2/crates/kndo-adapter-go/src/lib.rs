//! Go, through the tree-sitter-go grammar. The unit Go imports is the package — a
//! directory of files sharing one namespace with no imports between siblings — so
//! this adapter leans on the contract's directory features: imports resolve to
//! [`Resolution::Files`] (every non-test `.go` in the package dir), a synthetic
//! `"."` edge ties siblings together for reachability, and the spec declares
//! [`ReferenceScope::Directory`] so analyses pool references the way the language
//! scopes them. Capitalization IS the visibility: an upper-case initial is
//! exported, anything else package-private.
//!
//! The adapter id is `go`, the same id v1 used for this territory, so oracle
//! comparisons line up file-for-file.

mod extract;
mod manifest;
mod resolve;

use kndo_contract::adapter::{
    AdapterSpec, LanguageAdapter, PackageEntry, ReferenceScope, Resolution, ResolveContext,
    SourceFile,
};
use kndo_contract::evidence::{DiagnosticLevel, EvidenceSink, EvidenceStream, EvidenceStreams};
use kndo_contract::vocab::ProjectPath;

pub struct GoAdapter {
    spec: AdapterSpec,
}

impl GoAdapter {
    pub fn new() -> Self {
        let spec = AdapterSpec::builder("go", 1)
            .extensions(&["go"])
            .emits(EvidenceStreams::of(&[
                EvidenceStream::Comments,
                EvidenceStream::Metrics,
            ]))
            .manifests(&["**/go.mod"])
            .reference_scope(ReferenceScope::Directory)
            .build();
        GoAdapter { spec }
    }
}

impl Default for GoAdapter {
    fn default() -> Self {
        GoAdapter::new()
    }
}

impl LanguageAdapter for GoAdapter {
    fn spec(&self) -> &AdapterSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        let language = tree_sitter_go::LANGUAGE.into();
        match kndo_toolkit::parse(&language, file.content) {
            Some(tree) => {
                if tree.root_node().has_error() {
                    out.diagnostic(
                        DiagnosticLevel::Info,
                        "syntax errors in file — evidence may be partial",
                        None,
                    );
                }
                extract::extract(file.path, file.content, &tree, out);
            }
            None => out.diagnostic(
                DiagnosticLevel::Warn,
                "parse produced no tree — no evidence extracted from this file",
                None,
            ),
        }
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        resolve::resolve(from, specifier, cx)
    }

    fn packages(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<PackageEntry> {
        manifest::packages(manifest, cx)
    }
}
