//! Rust language adapter (docs/adapters/rust.md). The third `LanguageAdapter`, and the first
//! whose dogfood corpus is kndo's own repository. The one Rust-shaped idea the whole adapter
//! is built on: **the module tree IS the file graph** — `mod foo;` is an import (the parent's
//! certain `ImportsFile` edge to the child), and a file no `mod` chain reaches is dead to the
//! compiler, a verdict reachability then reproduces for free.

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

pub struct RustAdapter;

/// docs/adapters/rust.md §1: integration tests, benches, and examples are test-role
/// (an example consumes the API from outside exactly like a test — a symbol alive only
/// through its own demo is the `test-only` verdict); `build.rs`, `.cargo/`, and the
/// de-facto `xtask/` task-runner convention are tooling.
const PATH_PATTERNS: kndo_adapter_toolkit::classify::PathPatterns =
    kndo_adapter_toolkit::classify::PathPatterns {
        test_name_markers: &[],
        test_dirs: &["tests", "benches", "examples"],
        tooling_name_markers: &["build.rs"],
        tooling_dirs: &[".cargo", "xtask"],
    };

impl LanguageAdapter for RustAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            id: SmolStr::new("rust"),
            // 2: test regions are the only producer-side test declaration — extraction
            // stopped emitting per-declaration Test roots (assembly derives them from
            // `test_spans` containment, contracts §2).
            facts_schema_version: 2,
            file_globs: vec![SmolStr::new("**/*.rs")],
            manifest_globs: vec![SmolStr::new("**/Cargo.toml")],
            grammar_version: SmolStr::new("tree-sitter-rust 0.24"),
            // docs/adapters/rust.md §2: [File "private", Package "pub(crate)", Public "pub"].
            // pub(super)/pub(in …) are widened to the crate rung — widening only ever
            // silences; narrowing would fabricate internal-only accusations.
            visibility_ladder: vec![
                VisibilityRung {
                    scope: VisibilityScope::File,
                    label: SmolStr::new("private"),
                },
                VisibilityRung {
                    scope: VisibilityScope::Package,
                    label: SmolStr::new("pub(crate)"),
                },
                VisibilityRung {
                    scope: VisibilityScope::Public,
                    label: SmolStr::new("pub"),
                },
            ],
            // docs/adapters/rust.md §5: module cycles inside a crate are legal and common
            // (RFC 0005 §8's own Idiomatic example). Package cycles are NOT Impossible —
            // dev-dependency cycles are legal cargo, and kndo's package edges include test
            // files — so Impossible would suppress real, visible structure.
            cycle_policy: CyclePolicy {
                file_cycles: CycleTolerance::Idiomatic,
                package_cycles: CycleTolerance::Idiomatic,
            },
            // The crate name IS the `use` specifier's root segment — resolve() structurally
            // identifies the declared dependency every time.
            resolves_dependency_usage: true,
        }
    }

    fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
        let p = path.0.as_str();
        if !p.ends_with(".rs") {
            return None;
        }
        Some(FileClaim {
            language: SmolStr::new("rust"),
            class: kndo_adapter_toolkit::classify::classify(p, &PATH_PATTERNS),
        })
    }

    fn claim_manifest(&self, path: &ProjectPath) -> bool {
        matches!(path.0.rsplit('/').next(), Some("Cargo.toml"))
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
    use kndo_core::vocab::{FileOrigin, FileRole};

    fn path(p: &str) -> ProjectPath {
        ProjectPath(SmolStr::new(p))
    }

    #[test]
    fn claims_rs_files_and_rejects_others() {
        let a = RustAdapter;
        assert!(a.claim(&path("src/lib.rs")).is_some());
        assert!(a.claim(&path("src/main.go")).is_none());
        assert!(a.claim(&path("Cargo.toml")).is_none());
    }

    #[test]
    fn roles_follow_the_cargo_directory_conventions() {
        let a = RustAdapter;
        assert_eq!(
            a.claim(&path("tests/integration.rs")).unwrap().class.role,
            FileRole::Test
        );
        assert_eq!(
            a.claim(&path("benches/lookup.rs")).unwrap().class.role,
            FileRole::Test
        );
        assert_eq!(
            a.claim(&path("examples/demo.rs")).unwrap().class.role,
            FileRole::Test
        );
        assert_eq!(
            a.claim(&path("build.rs")).unwrap().class.role,
            FileRole::Tooling
        );
        assert_eq!(
            a.claim(&path("xtask/src/main.rs")).unwrap().class.role,
            FileRole::Tooling
        );
        assert_eq!(
            a.claim(&path("src/lib.rs")).unwrap().class.role,
            FileRole::Production
        );
    }

    #[test]
    fn vendor_is_vendored() {
        let a = RustAdapter;
        assert_eq!(
            a.claim(&path("vendor/foo/src/lib.rs"))
                .unwrap()
                .class
                .origin,
            FileOrigin::Vendored
        );
    }

    #[test]
    fn the_trait_surface_delegates_end_to_end() {
        // The conformance suite reaches these methods only through the engine's dynamic
        // dispatch, which static test-reachability cannot see — exercise the trait impl
        // directly: descriptor identity, extraction, manifest extraction, resolution.
        let a = RustAdapter;
        let d = a.descriptor();
        assert_eq!(d.id, "rust");
        assert_eq!(d.visibility_ladder.len(), 3);

        let src_path = path("src/lib.rs");
        let facts = a.extract(&SourceFile {
            path: &src_path,
            content: b"mod child;\npub fn api() {}\n",
        });
        assert!(facts.imports.iter().any(|i| i.specifier == "self::child"));
        assert!(facts.declarations.iter().any(|d| d.name == "api"));

        let known: rustc_hash::FxHashSet<ProjectPath> =
            [path("src/lib.rs"), path("src/child.rs"), path("Cargo.toml")]
                .into_iter()
                .collect();
        let ctx = ResolveCtx::new(&known);
        let manifest_path = path("Cargo.toml");
        let mf = a.extract_manifest(
            &SourceFile {
                path: &manifest_path,
                content: b"[package]\nname = \"demo\"\n",
            },
            &ctx,
        );
        assert_eq!(mf.package_name.as_deref(), Some("demo"));

        let resolved = a.resolve(
            &ImportSpec {
                specifier: SmolStr::new("self::child"),
                from: path("src/lib.rs"),
            },
            &ctx,
        );
        match resolved {
            Resolution::File(p, _) => assert_eq!(p.0, "src/child.rs"),
            other => panic!("expected file resolution, got {other:?}"),
        }
    }

    #[test]
    fn claim_manifest_matches_cargo_toml_only() {
        let a = RustAdapter;
        assert!(a.claim_manifest(&path("Cargo.toml")));
        assert!(a.claim_manifest(&path("crates/x/Cargo.toml")));
        assert!(!a.claim_manifest(&path("Cargo.lock")));
    }
}
