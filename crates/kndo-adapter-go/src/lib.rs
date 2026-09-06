//! Go, through the tree-sitter-go grammar. What Go imports is the package — a
//! directory of files sharing one namespace with no imports between siblings —
//! and this adapter says so twice, because the package is two facts. It is a
//! NAMESPACE, which extraction declares from the package clause and the
//! directory, so a lower-case name reaches it and nothing wider. And its files
//! CO-COMPILE, which [`Extension::sees`] declares from the file set (a
//! production file sees its non-test siblings; a test file sees the whole
//! package) — sight no file's own bytes could state, and the edge that carries
//! reachability from an exported name to the file next to it. Imports resolve
//! to [`Resolution::Files`], every non-test `.go` in the package dir.
//! Capitalization IS the visibility: an upper-case initial is exported,
//! anything else package-private.
//!
//! The unit is the MODULE: `go.mod` is the one manifest, it names what other
//! modules import, and Go has no per-target section to split it.
//!
//! The adapter id is `go`, the same id v1 used for this territory, so oracle
//! comparisons line up file-for-file.

mod extract;
mod manifest;
mod resolve;

use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::EvidenceSink;
use kndo_contract::extension::{Extension, ExtensionSpec, Rung, Step};
use kndo_contract::manifest::ManifestSink;
use kndo_contract::vocab::ProjectPath;

pub struct GoAdapter {
    spec: ExtensionSpec,
}

impl GoAdapter {
    pub fn new() -> Self {
        GoAdapter {
            // 6: the package clause names a namespace, an unexported name
            // reaches it, and go.mod declares the module unit.
            spec: kndo_toolkit::source_adapter_builder(
                "kndo:go",
                6,
                &["go"],
                &["**/go.mod"],
                // The compiler forbids import cycles: one could only be a
                // resolution artifact here.
                kndo_contract::extension::CycleTolerance::Tolerated,
            )
            // The go tool's own rule: a file beginning with `_` is in no
            // package it builds, and `vendor` holds copies of other modules,
            // compiled as the dependencies they are. A `testdata` or
            // `_`-prefixed directory is not one: `./...` skips it, and an
            // import from a package compiles it.
            .ignores(&["**/vendor/**", "**/_*.go"])
            // Capitalization is the whole ladder: nothing sits below the
            // package, so a package-private name used only in its file has
            // nowhere narrower to go and `internal-only` stays silent for it.
            .ladder(&[
                Step::new(Rung::Namespace, "unexported"),
                // An exported name in an `internal` package is spelled the
                // same way and reaches the fence's subtree: the word is
                // `exported` on both rungs, and the advice below either is
                // `unexported`.
                Step::new(Rung::Directory, "exported"),
                Step::new(Rung::Exported, "exported"),
            ])
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

    fn extract_manifest(
        &self,
        manifest: &SourceFile<'_>,
        cx: &ResolveContext<'_>,
        out: &mut ManifestSink,
    ) {
        manifest::structure(manifest, cx, out);
    }

    fn sees(&self, path: &ProjectPath, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
        resolve::sees(path, cx)
    }
}
