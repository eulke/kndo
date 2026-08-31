//! Rust, through the tree-sitter-rust grammar. The adapter reports evidence only —
//! declarations with binary reach (`pub` in any form is nameable beyond its file),
//! module edges (`mod foo;`, `use` trees, qualified paths), keep-alive-biased
//! references, comment spans, function metrics — and resolves module paths against
//! the file tree; judgment stays in the engine.
//!
//! Precision posture: a `use` of one item keeps its target file's whole exported
//! surface (`Namespace`), because Rust's alias scopes cannot be re-derived from one
//! file. Qualified paths (`crate::x::f()`) name their item exactly, so those bind.
//! Over-keeping is the deliberate direction — refinement arrives with the
//! visibility-ladder work, measured.
//!
//! The adapter id is `rust`, the same id v1 used for this territory, so oracle
//! comparisons line up file-for-file.

mod extract;
mod manifest;
mod resolve;

use kndo_contract::adapter::{
    DependencyDeclaration, PackageEntry, ProjectRoot, Resolution, ResolveContext, SourceFile,
};
use kndo_contract::evidence::EvidenceSink;
use kndo_contract::extension::{Extension, ExtensionSpec};
use kndo_contract::vocab::ProjectPath;

pub struct RustAdapter {
    spec: ExtensionSpec,
}

impl RustAdapter {
    pub fn new() -> Self {
        RustAdapter {
            spec: kndo_toolkit::source_adapter_spec(
                "kndo:rust",
                2,
                &["rs"],
                &["**/Cargo.toml"],
                &["crate"],
            ),
        }
    }
}

impl Default for RustAdapter {
    fn default() -> Self {
        RustAdapter::new()
    }
}

impl Extension for RustAdapter {
    fn spec(&self) -> &ExtensionSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        let language = tree_sitter_rust::LANGUAGE.into();
        if let Some(tree) = kndo_toolkit::parse_reporting(&language, file.content, out) {
            extract::extract(file.path, file.content, &tree, out);
        }
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        resolve::resolve(from, specifier, cx)
    }

    fn seen_from(
        &self,
        path: &ProjectPath,
        scope: &str,
        cx: &ResolveContext<'_>,
    ) -> Option<Vec<ProjectPath>> {
        if scope != "crate" {
            return None;
        }
        resolve::crate_region(path, cx)
    }

    fn roots(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<ProjectRoot> {
        manifest::roots(manifest, cx)
    }

    fn packages(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<PackageEntry> {
        manifest::packages(manifest, cx)
    }

    fn manifest_dependencies(&self, manifest: &SourceFile<'_>) -> Vec<DependencyDeclaration> {
        manifest::dependencies(manifest)
    }
}
