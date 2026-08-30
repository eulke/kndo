//! Go, through the tree-sitter-go grammar. The unit Go imports is the package — a
//! directory of files sharing one namespace with no imports between siblings — so
//! this adapter leans on the contract's unit features: imports resolve to
//! [`Resolution::Files`] (every non-test `.go` in the package dir), and
//! [`Extension::unit_mates`] declares what each file sees without an import
//! (a production file sees its non-test siblings; a test file sees the whole
//! package), which the engine turns into reachability edges and pooled
//! references. Capitalization IS the visibility: an upper-case initial is
//! exported, anything else package-private.
//!
//! The adapter id is `go`, the same id v1 used for this territory, so oracle
//! comparisons line up file-for-file.

mod extract;
mod manifest;
mod resolve;

use kndo_contract::adapter::{PackageEntry, Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::{DiagnosticLevel, EvidenceSink, EvidenceStream, EvidenceStreams};
use kndo_contract::extension::{Extension, ExtensionSpec};
use kndo_contract::vocab::ProjectPath;
use smol_str::SmolStr;

pub struct GoAdapter {
    spec: ExtensionSpec,
}

impl GoAdapter {
    pub fn new() -> Self {
        // semantics_version 2: evidence carries no synthetic package edge — the
        // unit fact moved to `unit_mates`, out of content-keyed cache entries.
        let spec = ExtensionSpec::builder("go", 2)
            .extensions(&["go"])
            .emits(EvidenceStreams::of(&[
                EvidenceStream::Comments,
                EvidenceStream::Metrics,
            ]))
            .manifests(&["**/go.mod"])
            .build();
        GoAdapter { spec }
    }
}

impl Default for GoAdapter {
    fn default() -> Self {
        GoAdapter::new()
    }
}

impl Extension for GoAdapter {
    fn spec(&self) -> &ExtensionSpec {
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

    fn manifest_dependencies(&self, manifest: &SourceFile<'_>) -> Vec<SmolStr> {
        manifest::dependencies(manifest)
    }

    fn unit_mates(&self, path: &ProjectPath, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
        resolve::unit_mates(path, cx)
    }
}
