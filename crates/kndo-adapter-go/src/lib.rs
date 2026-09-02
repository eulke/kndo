//! Go, through the tree-sitter-go grammar. The unit Go imports is the package — a
//! directory of files sharing one namespace with no imports between siblings — so
//! this adapter leans on the contract's unit features: imports resolve to
//! [`Resolution::Files`] (every non-test `.go` in the package dir), and
//! [`Extension::sees`] declares what each file sees without an import
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
use kndo_contract::evidence::EvidenceSink;
use kndo_contract::extension::{Extension, ExtensionSpec};
use kndo_contract::vocab::ProjectPath;

pub struct GoAdapter {
    spec: ExtensionSpec,
}

impl GoAdapter {
    pub fn new() -> Self {
        GoAdapter {
            // 4: go.mod `// indirect` requirements declare `Transitive`.
            spec: kndo_toolkit::source_adapter_builder(
                "kndo:go",
                4,
                &["go"],
                &["**/go.mod"],
                &[],
                // The compiler forbids import cycles: one could only be a
                // resolution artifact here.
                kndo_contract::extension::CycleTolerance::Tolerated,
            )
            // go.mod has no sections: every direct requirement is a build
            // requirement, and "only tests import it" has nowhere to move.
            .dependency_scoping(kndo_contract::extension::DependencyScoping::Unscoped)
            // An import path names the module whose path prefixes it; a path
            // whose first segment carries no `.` is the standard library.
            .dependency_identity(kndo_contract::extension::DependencyIdentity::ModulePath)
            .dependency_builtins(kndo_contract::extension::DependencyBuiltins::UndottedFirstSegment)
            .build(),
        }
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
        if let Some(tree) = kndo_toolkit::parse_reporting(&language, file.content, out) {
            extract::extract(file.path, file.content, &tree, out);
        }
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        resolve::resolve(from, specifier, cx)
    }

    fn packages(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<PackageEntry> {
        manifest::packages(manifest, cx)
    }

    fn manifest_dependencies(
        &self,
        manifest: &SourceFile<'_>,
    ) -> Vec<kndo_contract::adapter::DependencyDeclaration> {
        manifest::dependencies(manifest)
    }

    fn sees(&self, path: &ProjectPath, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
        resolve::sees(path, cx)
    }

    fn seen_from(
        &self,
        path: &ProjectPath,
        scope: &str,
        cx: &ResolveContext<'_>,
    ) -> Option<Vec<ProjectPath>> {
        (scope == "package").then(|| resolve::package_region(path, cx))
    }
}
