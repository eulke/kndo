//! `kndo:swift` — the sixth built-in, on the shared playbook with Swift's own
//! rules:
//!
//! - The DEFAULT visibility is `internal` — module scope: `Scoped("module")`,
//!   the region mechanism's home rung (`private`/`fileprivate` fold to Private,
//!   both file-bounded facts; `public`/`open` are Exported). The module can
//!   narrow to `fileprivate`/`private`, so `narrowable(["module"])`.
//! - The unit is the SwiftPM TARGET, and it is flat: subdirectories inside a
//!   target are organizational, every file of the target shares one namespace,
//!   and TESTS ARE A DIFFERENT MODULE — they reach the code under test through
//!   an explicit `@testable import`, so [`Extension::sees`] needs no test
//!   mirror at all (the import is the edge). Files outside the
//!   `Sources|Tests/<Target>` layout take their first path segment as the
//!   target (a repository like Alamofire compiles `Source/**` as one module
//!   via a `path:` override; the segment is the content-free spelling of that
//!   fact).
//! - `import Foo` names a whole module and puts its top-level names in bare
//!   scope — a namespace-shaped import resolving to every file of the local
//!   target `Foo`, or nothing for SDK/external modules (keep-alive).
//! - Dispatch the source never names: `override` methods (Probable) and the
//!   non-private methods of types that declare conformances — an external
//!   protocol's requirements are not statically enumerable, and its witnesses
//!   are invoked by machinery outside the repo (Codable synthesis, delegate
//!   protocols), so they root at `Possible`: degrade toward silence on exactly
//!   the fact we cannot enumerate.
//! - Never declared: initializers and deinitializers (construction follows the
//!   type — the constructor posture every adapter shares) and enum cases
//!   (`.case` dot-shorthand references are the pervasive use form and resolve
//!   against types, not names — accusing cases on name evidence would be
//!   noise; their uses still land in the reference pool).

mod extract;
mod manifest;
mod resolve;

use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::EvidenceSink;
use kndo_contract::extension::{Extension, ExtensionSpec};
use kndo_contract::vocab::ProjectPath;
use smol_str::SmolStr;

pub struct SwiftAdapter {
    spec: ExtensionSpec,
}

impl SwiftAdapter {
    pub fn new() -> Self {
        SwiftAdapter {
            spec: kndo_toolkit::source_adapter_spec(
                "kndo:swift",
                1,
                &["swift"],
                &["**/Package.swift"],
                &["module"],
            ),
        }
    }
}

impl Default for SwiftAdapter {
    fn default() -> Self {
        SwiftAdapter::new()
    }
}

impl Extension for SwiftAdapter {
    fn spec(&self) -> &ExtensionSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        let language = tree_sitter_swift::LANGUAGE.into();
        if let Some(tree) = kndo_toolkit::parse_reporting(&language, file.content, out) {
            extract::extract(file.path, file.content, &tree, out);
        }
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        resolve::resolve(from, specifier, cx)
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
        (scope == "module").then(|| resolve::module_region(path, cx))
    }

    fn manifest_dependencies(&self, manifest: &SourceFile<'_>) -> Vec<SmolStr> {
        manifest::dependencies(manifest.content)
    }
}
