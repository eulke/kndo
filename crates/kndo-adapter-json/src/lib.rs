//! JSON adapter — a non-source language: claims files, extracts no symbols. Its entire value
//! is giving a `.json` file a `FileClass` so `unused` and every other analysis can see it at
//! all — resolution to a JSON file happens through any other adapter's own resolver checking
//! `ResolveCtx`'s discovered-files index.

mod extraction;
mod resolution;

use kndo_core::adapter::{
    AdapterDescriptor, CyclePolicy, CycleTolerance, FileClaim, FileFacts, ImportSpec,
    LanguageAdapter, ProjectPath, Resolution, ResolveCtx, SourceFile,
};
use kndo_core::vocab::{FileClass, FileOrigin, FileRole};
use smol_str::SmolStr;

pub struct JsonAdapter;

/// The one piece of real cross-adapter awareness this adapter carries — `package.json` and
/// `tsconfig.json` are JS-TS's manifest/configuration files, end in `.json`, and would
/// otherwise be double-claimed as plain data, violating "manifests are not claimed."
const OWNED_ELSEWHERE: &[&str] = &["package.json", "tsconfig.json"];

impl LanguageAdapter for JsonAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            activation: Vec::new(),
            dependencies: Vec::new(),
            id: SmolStr::new("json"),
            facts_schema_version: 2,
            file_globs: vec![SmolStr::new("**/*.json")],
            // No manifest format of its own — every JSON-shaped manifest belongs to
            // the adapter for the language it configures, never to "JSON" as a bare format.
            manifest_globs: vec![],
            // Nothing here needs a tree-sitter grammar (no complexity metric, no token
            // stream, no query layer earns its keep over a flat value tree) — `serde_json`
            // parses-and-validates in one call.
            grammar_version: SmolStr::new("serde_json 1"),
            // Empty ladder — JSON has no visibility semantics, so visibility analyses skip
            // its files entirely.
            visibility_ladder: vec![],
            // Moot either way: a JSON file never has an outgoing edge (it never imports
            // anything), so it can never be a member of a cycle regardless of policy.
            cycle_policy: CyclePolicy {
                file_cycles: CycleTolerance::Idiomatic,
                package_cycles: CycleTolerance::Idiomatic,
            },
            // Moot: this adapter never contributes a manifest/PackageNode, so
            // `dependency_hygiene` never consults this flag for it.
            resolves_dependency_usage: false,
            // A data file holds no callable unit; "is it tested" has no answer for it.
            declares_units_of_testing: false,
            package_test_dirs: Vec::new(),
            // This adapter declares no builtin type facts: an empty table simply means the
            // chain resolver has no second tier to consult for it.
            builtin_member_types: Vec::new(),
        }
    }

    fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
        let p = path.0.as_str();
        if !p.ends_with(".json") {
            return None;
        }
        let basename = p.rsplit('/').next().unwrap_or(p);
        if OWNED_ELSEWHERE.contains(&basename) {
            return None;
        }
        Some(FileClaim {
            language: SmolStr::new("json"),
            // No path-pattern-driven role/origin split: every claimed JSON file is
            // production, authored — a documented non-goal to revisit only with real signal.
            class: FileClass {
                role: FileRole::Production,
                origin: FileOrigin::Authored,
            },
        })
    }

    fn extract(&self, file: &SourceFile<'_>) -> FileFacts {
        extraction::extract(file.content)
    }

    fn resolve(&self, spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
        resolution::resolve(spec, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(p: &str) -> ProjectPath {
        ProjectPath(SmolStr::new(p))
    }

    #[test]
    fn claims_plain_json_files_and_rejects_non_json() {
        let a = JsonAdapter;
        assert!(a.claim(&path("config/data.json")).is_some());
        assert!(a.claim(&path("data.yaml")).is_none());
    }

    #[test]
    fn well_known_manifests_are_not_claimed_here_regardless_of_directory() {
        let a = JsonAdapter;
        assert!(a.claim(&path("package.json")).is_none());
        assert!(a.claim(&path("packages/lib/package.json")).is_none());
        assert!(a.claim(&path("tsconfig.json")).is_none());
        // Not a substring match — only the exact well-known basenames are excluded.
        assert!(a.claim(&path("tsconfig.build.json")).is_some());
        assert!(a.claim(&path("my-package.json")).is_some());
    }

    #[test]
    fn claim_manifest_is_always_false() {
        let a = JsonAdapter;
        assert!(!a.claim_manifest(&path("package.json")));
        assert!(!a.claim_manifest(&path("data.json")));
    }

    #[test]
    fn claimed_files_are_always_production_and_authored() {
        let a = JsonAdapter;
        let claim = a.claim(&path("src/data.json")).unwrap();
        assert_eq!(claim.class.role, FileRole::Production);
        assert_eq!(claim.class.origin, FileOrigin::Authored);
    }

    #[test]
    fn the_trait_surface_delegates_end_to_end() {
        let a = JsonAdapter;
        let d = a.descriptor();
        assert_eq!(d.id, "json");
        assert!(d.visibility_ladder.is_empty());
        assert!(d.manifest_globs.is_empty());

        let src_path = path("data.json");
        let facts = a.extract(&SourceFile {
            path: &src_path,
            content: br#"{"ok": true}"#,
        });
        assert!(facts.declarations.is_empty());
        assert!(facts.diagnostics.is_empty());

        let known: rustc_hash::FxHashSet<ProjectPath> = [path("data.json")].into_iter().collect();
        let ctx = ResolveCtx::new(&known);
        let resolved = a.resolve(
            &ImportSpec {
                specifier: SmolStr::new("./data.json"),
                from: path("src/main.ts"),
            },
            &ctx,
        );
        assert_eq!(resolved, Resolution::Unresolved);
    }
}
