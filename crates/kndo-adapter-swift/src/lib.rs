//! `kndo:swift` — the sixth built-in, on the shared playbook with Swift's own
//! rules:
//!
//! - The DEFAULT visibility is `internal` — the unit's reach, bounded from
//!   the target layout until the manifest names the target (`private` and
//!   `fileprivate` fold to Private, both file-bounded facts; `public`/`open`
//!   are Exported). The ladder spells all four rungs, so an `internal` name
//!   used only in its file is advised `fileprivate`, and one used only in its
//!   type `private`.
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
use kndo_contract::evidence::{EvidenceSink, Reach};
use kndo_contract::extension::{Extension, ExtensionSpec, Rung, Step};
use kndo_contract::vocab::ProjectPath;

pub struct SwiftAdapter {
    spec: ExtensionSpec,
}

impl SwiftAdapter {
    pub fn new() -> Self {
        SwiftAdapter {
            // 4: the generated banner is reported, never concluded.
            spec: kndo_toolkit::source_adapter_builder(
                "kndo:swift",
                5,
                &["swift"],
                &["**/Package.swift"],
                // Files in a module compile as one unit; cross-references are
                // routine, and the compiler rejects target-level cycles.
                kndo_contract::extension::CycleTolerance::Tolerated,
            )
            .ladder(&[
                Step::new(Rung::Owner, "private"),
                Step::new(Rung::File, "fileprivate"),
                Step::new(Rung::Unit, "internal"),
                Step::new(Rung::Exported, "public"),
            ])
            .build(),
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

    fn seen_from(
        &self,
        path: &ProjectPath,
        reach: &Reach,
        cx: &ResolveContext<'_>,
    ) -> Option<Vec<ProjectPath>> {
        matches!(reach, Reach::Unit { up: 0 }).then(|| resolve::module_region(path, cx))
    }

    fn manifest_dependencies(
        &self,
        manifest: &SourceFile<'_>,
    ) -> Vec<kndo_contract::adapter::DependencyDeclaration> {
        manifest::dependencies(manifest.content)
            .into_iter()
            .map(kndo_contract::adapter::DependencyDeclaration::name_only)
            .collect()
    }
}
