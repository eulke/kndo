//! `kndo:swift` — the sixth built-in, on the shared playbook with Swift's own
//! rules:
//!
//! - The DEFAULT visibility is `internal` — the MODULE's reach, which a
//!   `@testable import` widens to the test target the manifest made the
//!   module's friend, and which falls back to the namespace each file declares
//!   where no manifest named the target at all (`fileprivate` is the file's
//!   and `private` the owner's on a member; `package` is the group of targets
//!   one package aggregates; `public`/`open` are Exported). The ladder spells
//!   every rung, so an `internal` name used only in its file is advised
//!   `fileprivate`, and one used only in its type `private`.
//! - The unit is the SwiftPM TARGET, and it is flat: subdirectories inside a
//!   target are organizational, every file of the target shares one namespace,
//!   and TESTS ARE A DIFFERENT MODULE — they reach the code under test through
//!   an explicit `@testable import` — the import is the edge, and no test
//!   mirror of the namespace exists at all. Files outside the
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
use kndo_contract::evidence::RootKind;
use kndo_contract::extension::{Extension, ExtensionSpec, FileRole, Rung, Step, UnnamedUnit};
use kndo_contract::vocab::ProjectPath;

pub struct SwiftAdapter {
    spec: ExtensionSpec,
}

/// What SWIFT ITSELF dispatches on, as data. The line the design draws: a
/// language's own stdlib facts are the adapter's `dispatch_rules`, and a
/// framework's are its pack's. `@main` is Swift; XCTest's `test*` collection,
/// swift-testing's `@Test`, SwiftUI's `View` and UIKit's `@UIApplicationMain`
/// are libraries you import, and their rules are `kndo:xctest`,
/// `kndo:swift-testing`, `kndo:swiftui` and `kndo:uikit` (M8.e).
fn dispatch_rules() -> Vec<kndo_contract::extension::DispatchRule> {
    use kndo_contract::evidence::RootKind;
    use kndo_contract::extension::{DispatchRule, Effect, Trigger};
    use kndo_contract::vocab::Confidence;
    vec![
        // `@main` — the language's own entry attribute. SwiftPM resolves the
        // attributed type's `static main()` as the executable's entry, so the
        // type is what the toolchain names.
        DispatchRule {
            when: Trigger::Marker {
                path: "main".into(),
                arg: None,
                target: Some(kndo_contract::evidence::SymbolKind::Type),
            },
            then: Effect::Root(RootKind::Production),
            confidence: Confidence::Certain,
        },
        // `override` — invoked through the superclass, a call the source never
        // spells. A member that answers a promise its type made is a WITNESS:
        // alive while the type is, and of no colour, because nothing outside
        // is ENTERED through it.
        DispatchRule {
            when: Trigger::Marker {
                path: "override".into(),
                arg: None,
                target: None,
            },
            then: Effect::Witness,
            confidence: Confidence::Certain,
        },
    ]
}

impl SwiftAdapter {
    pub fn new() -> Self {
        SwiftAdapter {
            // 4: the generated banner is reported, never concluded.
            spec: kndo_toolkit::source_adapter_builder(
                "kndo:swift",
                13,
                &["swift"],
                &["**/Package.swift"],
                // Files in a module compile as one unit; cross-references are
                // routine, and the compiler rejects target-level cycles.
                kndo_contract::extension::CycleTolerance::Tolerated,
            )
            // What a rule reads about a Swift declaration: the attributes and
            // the one modifier it carries, the types it promises the surface
            // of, and — for `expr.member` — the name the member was read from.
            .emits(kndo_contract::evidence::EvidenceStreams::of(&[
                kndo_contract::evidence::EvidenceStream::Markers,
                kndo_contract::evidence::EvidenceStream::Relations,
                kndo_contract::evidence::EvidenceStream::Qualifiers,
            ]))
            .ladder(&[
                Step::new(Rung::Owner, "private"),
                Step::new(Rung::File, "fileprivate"),
                Step::new(Rung::Unit, "internal"),
                Step::new(Rung::Exported, "public"),
            ])
            // Swift spells nothing between a module and a name: the namespace
            // each file declares IS the module, so an `internal` name in a
            // tree SwiftPM never described — an Xcode example app beside the
            // package — is still bounded by it.
            .unnamed_unit(UnnamedUnit::Namespace)
            // SwiftPM's own layout and its own filenames, where no target
            // declared the file's role: the `Tests/` tree is what `swift test`
            // builds and `main.swift` is the one filename whose top-level code
            // SwiftPM runs at process start, both toolchain rules; a
            // test-shaped NAME outside that tree is only the community's
            // habit. `Package.swift` (and its version-pinned twins) is the
            // manifest: Swift by format, tooling by role.
            .file_roles(&[
                FileRole::certain("**/Package.swift", RootKind::Tooling),
                FileRole::certain("**/Package@swift-*.swift", RootKind::Tooling),
                FileRole::certain("Tests/**", RootKind::Test),
                FileRole::certain("**/Tests/**", RootKind::Test),
                FileRole::certain("**/main.swift", RootKind::Production),
                FileRole::probable("**/*Test.swift", RootKind::Test),
                FileRole::probable("**/*Tests.swift", RootKind::Test),
            ])
            .dispatch(dispatch_rules())
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

    fn extract_manifest(
        &self,
        manifest: &SourceFile<'_>,
        _cx: &ResolveContext<'_>,
        out: &mut kndo_contract::manifest::ManifestSink,
    ) {
        manifest::structure(manifest.path, manifest.content, out);
    }
}
