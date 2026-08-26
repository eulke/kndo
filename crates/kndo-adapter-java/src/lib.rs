//! Java language adapter. Precision here rests entirely on the conformance fixtures.
//! The one Java-shaped idea the adapter is built on: package identity is declared
//! (`package` statement) *and* the compiler-checked file/directory convention makes it
//! directory-shaped too — a hybrid of Rust's declared-tree model and Go's
//! directory-is-the-unit model.

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

pub struct JavaAdapter;

/// `src/test/java/**` (Maven/Gradle Standard Directory Layout) is
/// the authoritative test-role signal; Surefire's own default filename patterns are a
/// belt-and-suspenders fallback for non-standard layouts. No tooling-role convention exists
/// (same stance as Go) — `pom.xml`/`build.gradle` are manifests, never role-classified source.
const PATH_PATTERNS: kndo_adapter_toolkit::classify::PathPatterns =
    kndo_adapter_toolkit::classify::PathPatterns {
        test_name_markers: &["Test.java", "Tests.java", "TestCase.java"],
        test_dirs: &["src/test/java"],
        // `package-info.java`/`module-info.java` are descriptors consumed by javac/javadoc,
        // not by code — Tooling role, so their reachability is healthy.
        tooling_name_markers: &["package-info.java", "module-info.java"],
        tooling_dirs: &[],
    };

impl LanguageAdapter for JavaAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            activation: Vec::new(),
            dependencies: Vec::new(),
            id: SmolStr::new("java"),
            facts_schema_version: 6, // bump whenever the serialized facts shape or the emission semantics change
            file_globs: vec![SmolStr::new("**/*.java")],
            manifest_globs: vec![
                SmolStr::new("**/pom.xml"),
                SmolStr::new("**/build.gradle"),
                SmolStr::new("**/build.gradle.kts"),
                SmolStr::new("**/settings.gradle"),
                SmolStr::new("**/settings.gradle.kts"),
            ],
            grammar_version: SmolStr::new("tree-sitter-java 0.23.5"),
            // Only two rungs apply to a TOP-LEVEL type (public /
            // package-private — private/protected are illegal there, Go's own shape); the
            // full four apply to members. Package-private maps to `Unit`, NOT kndo's `Package`
            // scope: `Package` means "same manifest/workspace-member" (PackageId —
            // JS's granularity, one npm package), a DIFFERENT thing from a Java `package`
            // (a `com.foo` namespace, potentially one of many sharing a single Maven module).
            // `FileFacts::unit` already carries the declared Java package name — checked
            // BEFORE the PackageId comparison in `required_scope` — so `Unit` is the correct,
            // exact rung, mirroring Go's own choice for its package-scoped visibility.
            // `protected` widens to `Public` — cross-package subclass access can't be ruled
            // out without a typechecker, so `Unit`/`Package` would under-report. Two rungs
            // sharing a scope is legal (Rust's `pub(super)` precedent); VisibilityLevel
            // still distinguishes them for the label text.
            visibility_ladder: vec![
                VisibilityRung {
                    scope: VisibilityScope::File,
                    label: SmolStr::new("private"),
                    surface_transitive: false,
                },
                VisibilityRung {
                    scope: VisibilityScope::Unit,
                    label: SmolStr::new("package-private"),
                    surface_transitive: false,
                },
                VisibilityRung {
                    scope: VisibilityScope::Public,
                    label: SmolStr::new("protected"),
                    surface_transitive: true,
                },
                VisibilityRung {
                    scope: VisibilityScope::Public,
                    label: SmolStr::new("public"),
                    surface_transitive: true,
                },
            ],
            // Circular package dependencies are routine in real Java codebases (no compiler
            // enforcement at all, unlike Rust's module tree) — Hazard, not Impossible, same
            // tolerance as JS's.
            cycle_policy: CyclePolicy {
                file_cycles: CycleTolerance::Hazard,
                package_cycles: CycleTolerance::Hazard,
            },
            // No reliable package→Maven/Gradle-coordinate
            // mapping exists without resolving the classpath — resolve() never emits
            // Resolution::Dependency for a third-party import, so dependency_hygiene's
            // unused/test-only verdicts would be a false-positive flood if attempted here.
            resolves_dependency_usage: false,
            package_test_dirs: Vec::new(),
            // No builtin type facts yet: this adapter declares none, and an empty table
            // simply means the chain resolver has no second tier to consult for it.
            builtin_member_types: Vec::new(),
        }
    }

    fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
        let p = path.0.as_str();
        if !p.ends_with(".java") {
            return None;
        }
        Some(FileClaim {
            language: SmolStr::new("java"),
            class: kndo_adapter_toolkit::classify::classify(p, &PATH_PATTERNS),
        })
    }

    fn claim_manifest(&self, path: &ProjectPath) -> bool {
        matches!(
            path.0.rsplit('/').next(),
            Some(
                "pom.xml"
                    | "build.gradle"
                    | "build.gradle.kts"
                    | "settings.gradle"
                    | "settings.gradle.kts"
            )
        )
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
    fn claims_java_files_and_rejects_others() {
        let a = JavaAdapter;
        assert!(a
            .claim(&path("src/main/java/com/foo/Widget.java"))
            .is_some());
        assert!(a.claim(&path("src/main/kotlin/Widget.kt")).is_none());
        assert!(a.claim(&path("pom.xml")).is_none());
    }

    #[test]
    fn roles_follow_the_standard_directory_layout_and_surefire_fallback() {
        let a = JavaAdapter;
        assert_eq!(
            a.claim(&path("src/test/java/com/foo/WidgetTest.java"))
                .unwrap()
                .class
                .role,
            FileRole::Test
        );
        // Surefire-named fallback outside the standard layout.
        assert_eq!(
            a.claim(&path("scripts/AdHocTest.java")).unwrap().class.role,
            FileRole::Test
        );
        assert_eq!(
            a.claim(&path("src/main/java/com/foo/Widget.java"))
                .unwrap()
                .class
                .role,
            FileRole::Production
        );
    }

    #[test]
    fn claim_manifest_matches_every_maven_and_gradle_manifest_file() {
        let a = JavaAdapter;
        assert!(a.claim_manifest(&path("pom.xml")));
        assert!(a.claim_manifest(&path("build.gradle")));
        assert!(a.claim_manifest(&path("build.gradle.kts")));
        assert!(a.claim_manifest(&path("settings.gradle")));
        assert!(a.claim_manifest(&path("settings.gradle.kts")));
        assert!(!a.claim_manifest(&path("gradle.properties")));
    }

    #[test]
    fn the_trait_surface_delegates_end_to_end() {
        // The conformance suite reaches these methods only through the engine's dynamic
        // dispatch, which static test-reachability cannot see — exercise the trait impl
        // directly: descriptor identity, extraction, manifest extraction, resolution.
        let a = JavaAdapter;
        let d = a.descriptor();
        assert_eq!(d.id, "java");
        assert_eq!(d.visibility_ladder.len(), 4);
        assert!(!d.resolves_dependency_usage);

        let src_path = path("src/main/java/com/foo/Widget.java");
        let facts = a.extract(&SourceFile {
            path: &src_path,
            content: b"package com.foo;\npublic class Widget {}\n",
        });
        assert_eq!(facts.unit.as_deref(), Some("com.foo"));
        assert!(facts.declarations.iter().any(|d| d.name == "Widget"));

        let known: rustc_hash::FxHashSet<ProjectPath> =
            [path("src/main/java/com/foo/Widget.java"), path("pom.xml")]
                .into_iter()
                .collect();
        let ctx = ResolveCtx::new(&known);
        let manifest_path = path("pom.xml");
        let mf = a.extract_manifest(
            &SourceFile {
                path: &manifest_path,
                content: b"<project><artifactId>demo</artifactId></project>",
            },
            &ctx,
        );
        assert_eq!(mf.package_name.as_deref(), Some("demo"));

        let resolved = a.resolve(
            &ImportSpec {
                specifier: SmolStr::new("java.util"),
                from: path("src/main/java/com/foo/Widget.java"),
            },
            &ctx,
        );
        assert_eq!(resolved, Resolution::Stdlib);
    }
}
