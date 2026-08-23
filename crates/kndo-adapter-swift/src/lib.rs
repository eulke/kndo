//! Swift language adapter (docs/adapters/swift.md). The sixth `LanguageAdapter`, and the first
//! whose manifest (`Package.swift`) is Swift source code rather than a data format —
//! `manifest.rs` reuses the same tree-sitter-swift parse `extraction` does. Like Java/Kotlin,
//! no dogfood corpus of its own (kndo is written in Rust); precision rests on the conformance
//! fixtures.

mod extraction;
mod manifest;
mod parsing;
mod resolution;

use kndo_core::adapter::{
    AdapterDescriptor, CyclePolicy, CycleTolerance, FileClaim, ImportSpec, LanguageAdapter,
    ManifestFacts, ProjectPath, Resolution, ResolveCtx, SourceFile, VisibilityRung,
    VisibilityScope,
};
use smol_str::SmolStr;

pub struct SwiftAdapter;

/// docs/adapters/swift.md §1: `Tests/**` (SwiftPM Standard Directory Layout) is the
/// authoritative test-role signal; XCTest's own `*Tests.swift` naming convention is a
/// belt-and-suspenders fallback for non-standard layouts. No tooling-role convention exists
/// (same stance as every prior adapter).
const PATH_PATTERNS: kndo_adapter_toolkit::classify::PathPatterns =
    kndo_adapter_toolkit::classify::PathPatterns {
        test_name_markers: &["Tests.swift"],
        test_dirs: &["Tests"],
        tooling_name_markers: &[],
        tooling_dirs: &[],
    };

impl LanguageAdapter for SwiftAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            activation: Vec::new(),
            dependencies: Vec::new(),
            id: SmolStr::new("swift"),
            facts_schema_version: 1,
            file_globs: vec![SmolStr::new("**/*.swift")],
            manifest_globs: vec![SmolStr::new("**/Package.swift")],
            grammar_version: SmolStr::new("tree-sitter-swift 0.7.3"),
            // docs/adapters/swift.md §0: every level applies at both top-level and member
            // position (no restricted subset the way Java/Kotlin's ladders have). `internal`
            // — the default when no modifier is written at all — maps to `Package` (kndo's
            // "same manifest" granularity, here an SPM target), a real structural difference
            // from Java (default ≈ Unit) and Kotlin (default = Public). `private` is real-
            // Swift narrower than `File` (scoped to the enclosing declaration) but widens up,
            // same conservative direction as every adapter's tightest-unavailable-scope case;
            // `fileprivate` is an exact match. `open` widens to `Public` alongside `public` —
            // kndo's scope model can't distinguish "subclassable outside the module" from
            // ordinary public visibility, same collapse Java's `protected`→`Public` already
            // establishes.
            visibility_ladder: vec![
                VisibilityRung {
                    scope: VisibilityScope::File,
                    label: SmolStr::new("private"),
                    surface_transitive: false,
                },
                VisibilityRung {
                    scope: VisibilityScope::File,
                    label: SmolStr::new("fileprivate"),
                    surface_transitive: false,
                },
                VisibilityRung {
                    scope: VisibilityScope::Package,
                    label: SmolStr::new("internal"),
                    surface_transitive: false,
                },
                VisibilityRung {
                    scope: VisibilityScope::Public,
                    label: SmolStr::new("public"),
                    surface_transitive: true,
                },
                VisibilityRung {
                    scope: VisibilityScope::Public,
                    label: SmolStr::new("open"),
                    surface_transitive: true,
                },
            ],
            // File cycles (two `.swift` files in the same target referencing each other) are
            // routine and idiomatic, same stance as Rust's within-crate module cycles.
            // Package/target cycles are compiler-enforced acyclic by SwiftPM itself (a real
            // target dependency cycle is a resolution failure, not buildable code) — same
            // "cannot exist in building code" reasoning as Go's own Impossible stance.
            cycle_policy: CyclePolicy {
                file_cycles: CycleTolerance::Idiomatic,
                package_cycles: CycleTolerance::Impossible,
            },
            // docs/adapters/swift.md §0/§3: `Package.swift` states a dependency's repository
            // URL, never the module/product name(s) it exports — those live in that
            // repository's own manifest, which kndo structurally never reads. Local target-to-
            // target imports resolve precisely via the ordinary same-unit fallback instead.
            resolves_dependency_usage: false,
        }
    }

    fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
        let p = path.0.as_str();
        // `Package.swift` is real Swift source too (§0/§4) — the one adapter in this codebase
        // where a manifest file also matches the source glob. "Manifests are not claimed"
        // (js-ts.md §1) is a load-bearing principle elsewhere in the engine (a manifest never
        // gets a `FileClaim` alongside its `ManifestFacts`), so it's excluded here explicitly
        // rather than accidentally satisfied the way every non-Swift manifest format is.
        if !p.ends_with(".swift") || self.claim_manifest(path) {
            return None;
        }
        Some(FileClaim {
            language: SmolStr::new("swift"),
            class: kndo_adapter_toolkit::classify::classify(p, &PATH_PATTERNS),
        })
    }

    fn claim_manifest(&self, path: &ProjectPath) -> bool {
        path.0.rsplit('/').next() == Some("Package.swift")
    }

    fn extract(&self, file: &SourceFile<'_>) -> kndo_core::adapter::FileFacts {
        extraction::extract(file.path.0.as_str(), file.content)
    }

    fn extract_manifest(&self, file: &SourceFile<'_>, ctx: &ResolveCtx<'_>) -> ManifestFacts {
        manifest::extract(file.path.0.as_str(), file.content, ctx)
    }

    fn resolve(&self, spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
        resolution::resolve(spec, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_core::vocab::FileRole;

    fn path(p: &str) -> ProjectPath {
        ProjectPath(SmolStr::new(p))
    }

    #[test]
    fn claims_swift_files_and_rejects_others() {
        let a = SwiftAdapter;
        assert!(a.claim(&path("Sources/MyLib/Widget.swift")).is_some());
        assert!(a.claim(&path("Sources/MyLib/Widget.kt")).is_none());
        assert!(a.claim(&path("Package.swift")).is_none());
    }

    #[test]
    fn roles_follow_the_standard_directory_layout_and_xctest_fallback() {
        let a = SwiftAdapter;
        assert_eq!(
            a.claim(&path("Tests/MyLibTests/WidgetTests.swift"))
                .unwrap()
                .class
                .role,
            FileRole::Test
        );
        assert_eq!(
            a.claim(&path("scripts/AdHocTests.swift"))
                .unwrap()
                .class
                .role,
            FileRole::Test
        );
        assert_eq!(
            a.claim(&path("Sources/MyLib/Widget.swift"))
                .unwrap()
                .class
                .role,
            FileRole::Production
        );
    }

    #[test]
    fn claim_manifest_matches_only_package_swift() {
        let a = SwiftAdapter;
        assert!(a.claim_manifest(&path("Package.swift")));
        assert!(!a.claim_manifest(&path("Sources/MyLib/Package.swift.txt")));
        assert!(!a.claim_manifest(&path("Package.resolved")));
    }

    #[test]
    fn the_trait_surface_delegates_end_to_end() {
        let a = SwiftAdapter;
        let d = a.descriptor();
        assert_eq!(d.id, "swift");
        assert_eq!(d.visibility_ladder.len(), 5);
        assert!(!d.resolves_dependency_usage);

        let src_path = path("Sources/MyLib/Widget.swift");
        let facts = a.extract(&SourceFile {
            path: &src_path,
            content: b"class Widget {}\n",
        });
        assert_eq!(facts.unit.as_deref(), Some("MyLib"));
        assert!(facts.declarations.iter().any(|d| d.name == "Widget"));

        let known: rustc_hash::FxHashSet<ProjectPath> =
            [path("Sources/MyLib/Widget.swift"), path("Package.swift")]
                .into_iter()
                .collect();
        let ctx = ResolveCtx::new(&known);
        let manifest_path = path("Package.swift");
        let mf = a.extract_manifest(
            &SourceFile {
                path: &manifest_path,
                content: b"let package = Package(name: \"MyLib\")",
            },
            &ctx,
        );
        assert_eq!(mf.package_name.as_deref(), Some("MyLib"));

        let resolved = a.resolve(
            &ImportSpec {
                specifier: SmolStr::new("Foundation"),
                from: path("Sources/MyLib/Widget.swift"),
            },
            &ctx,
        );
        assert_eq!(resolved, Resolution::Stdlib);
    }
}
