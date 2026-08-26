//! Kotlin language adapter, sharing its
//! manifest infrastructure wholesale with the Java adapter (`kndo-adapter-toolkit::
//! jvm_manifest`). Like Java's, precision rests entirely on the conformance fixtures.

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

pub struct KotlinAdapter;

/// `src/test/kotlin/**` (Kotlin Gradle plugin's Standard Directory
/// Layout) is the authoritative test-role signal; a Surefire-style filename fallback covers
/// non-standard layouts. No tooling-role convention exists (same stance as Java/Go).
const PATH_PATTERNS: kndo_adapter_toolkit::classify::PathPatterns =
    kndo_adapter_toolkit::classify::PathPatterns {
        test_name_markers: &["Test.kt", "Tests.kt", "TestCase.kt"],
        test_dirs: &["src/test/kotlin"],
        tooling_name_markers: &[],
        tooling_dirs: &[],
    };

impl LanguageAdapter for KotlinAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            activation: Vec::new(),
            dependencies: Vec::new(),
            id: SmolStr::new("kotlin"),
            facts_schema_version: 5, // bump whenever the serialized facts shape or the emission semantics change
            file_globs: vec![SmolStr::new("**/*.kt")],
            manifest_globs: vec![
                SmolStr::new("**/pom.xml"),
                SmolStr::new("**/build.gradle"),
                SmolStr::new("**/build.gradle.kts"),
                SmolStr::new("**/settings.gradle"),
                SmolStr::new("**/settings.gradle.kts"),
            ],
            grammar_version: SmolStr::new("tree-sitter-kotlin-ng 1.1.0"),
            // `package` carries no visibility meaning in Kotlin
            // (unlike Java) — the default with no modifier is `public`, not package-scoped, so
            // there is no `Unit`-scoped rung anywhere here. `internal` genuinely IS
            // `VisibilityScope::Package` (kndo's "same manifest" granularity — a Kotlin
            // compilation module) with no widening needed, unlike Java's package-private,
            // which needed `Unit`. `protected` (members only, "module ∪ subclasses anywhere")
            // widens to `Public`, mirroring Java's own two-rungs-share-a-scope pattern for the
            // same reason: no scope represents "module plus subclasses in any other module".
            visibility_ladder: vec![
                VisibilityRung {
                    scope: VisibilityScope::File,
                    label: SmolStr::new("private"),
                    surface_transitive: false,
                },
                VisibilityRung {
                    scope: VisibilityScope::Package,
                    label: SmolStr::new("internal"),
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
            // Same reasoning as Java's: no compiler enforcement against circular package
            // dependencies, routine in real Kotlin codebases.
            cycle_policy: CyclePolicy {
                file_cycles: CycleTolerance::Hazard,
                package_cycles: CycleTolerance::Hazard,
            },
            // Identical root cause to Java's — Kotlin rides
            // Maven/Gradle coordinates with no structural import→coordinate mapping.
            resolves_dependency_usage: false,
            package_test_dirs: Vec::new(),
            // No builtin type facts yet: this adapter declares none, and an empty table
            // simply means the chain resolver has no second tier to consult for it.
            builtin_member_types: Vec::new(),
        }
    }

    fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
        let p = path.0.as_str();
        if !p.ends_with(".kt") {
            return None;
        }
        Some(FileClaim {
            language: SmolStr::new("kotlin"),
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
    fn claims_kotlin_files_and_rejects_others() {
        let a = KotlinAdapter;
        assert!(a
            .claim(&path("src/main/kotlin/com/foo/Widget.kt"))
            .is_some());
        assert!(a.claim(&path("src/main/java/Widget.java")).is_none());
        assert!(a.claim(&path("build.gradle.kts")).is_none());
    }

    #[test]
    fn roles_follow_the_standard_directory_layout_and_filename_fallback() {
        let a = KotlinAdapter;
        assert_eq!(
            a.claim(&path("src/test/kotlin/com/foo/WidgetTest.kt"))
                .unwrap()
                .class
                .role,
            FileRole::Test
        );
        assert_eq!(
            a.claim(&path("scripts/AdHocTest.kt")).unwrap().class.role,
            FileRole::Test
        );
        assert_eq!(
            a.claim(&path("src/main/kotlin/com/foo/Widget.kt"))
                .unwrap()
                .class
                .role,
            FileRole::Production
        );
    }

    #[test]
    fn claim_manifest_matches_every_maven_and_gradle_manifest_file() {
        let a = KotlinAdapter;
        assert!(a.claim_manifest(&path("pom.xml")));
        assert!(a.claim_manifest(&path("build.gradle.kts")));
        assert!(!a.claim_manifest(&path("gradle.properties")));
    }

    #[test]
    fn the_trait_surface_delegates_end_to_end() {
        let a = KotlinAdapter;
        let d = a.descriptor();
        assert_eq!(d.id, "kotlin");
        assert_eq!(d.visibility_ladder.len(), 4);
        assert!(!d.resolves_dependency_usage);

        let src_path = path("src/main/kotlin/com/foo/Widget.kt");
        let facts = a.extract(&SourceFile {
            path: &src_path,
            content: b"package com.foo\nclass Widget\n",
        });
        assert_eq!(facts.unit.as_deref(), Some("com.foo"));
        assert!(facts.declarations.iter().any(|d| d.name == "Widget"));

        let known: rustc_hash::FxHashSet<ProjectPath> =
            [path("src/main/kotlin/com/foo/Widget.kt"), path("pom.xml")]
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
                specifier: SmolStr::new("kotlin.collections"),
                from: path("src/main/kotlin/com/foo/Widget.kt"),
            },
            &ctx,
        );
        assert_eq!(resolved, Resolution::Stdlib);
    }
}
