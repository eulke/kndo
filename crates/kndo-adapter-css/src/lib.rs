//! CSS/SCSS adapter — a non-source language, deliberately narrow: no selector/class/id
//! declarations (extracting them would seed reachability with symbols nothing in the graph
//! can be shown to consume), just file claiming, the `@import`/`@use`/`@forward` graph, and
//! custom-property/SCSS-variable/mixin/function declarations.

mod extraction;
mod parsing;
mod resolution;

use kndo_core::adapter::{
    AdapterDescriptor, CyclePolicy, CycleTolerance, FileClaim, FileFacts, ImportSpec,
    LanguageAdapter, ProjectPath, Resolution, ResolveCtx, SourceFile,
};
use kndo_core::vocab::{FileClass, FileOrigin, FileRole};
use smol_str::SmolStr;

pub struct CssAdapter;

impl LanguageAdapter for CssAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            activation: Vec::new(),
            dependencies: Vec::new(),
            id: SmolStr::new("css"),
            facts_schema_version: 2,
            file_globs: vec![SmolStr::new("**/*.css"), SmolStr::new("**/*.scss")],
            // No manifest format of its own.
            manifest_globs: vec![],
            grammar_version: SmolStr::new("tree-sitter-css 0.25.0 + tree-sitter-scss 1.0.0"),
            // Empty ladder — no visibility semantics, selectors/classes aren't
            // extracted at all, and the few symbols that do exist (custom properties,
            // SCSS variables/mixins/functions) have no language-level visibility modifiers.
            visibility_ladder: vec![],
            // Moot: this adapter's own declared symbols never have outgoing edges of their
            // own (they're referenced, never importers), and file-level @import/@use/@forward
            // cycles between CSS/SCSS files are real but rare enough that "idiomatic" (no
            // gate) is the honest default absent evidence either way.
            cycle_policy: CyclePolicy {
                file_cycles: CycleTolerance::Idiomatic,
                package_cycles: CycleTolerance::Idiomatic,
            },
            // Moot: this adapter never contributes a manifest/PackageNode, so
            // `dependency_hygiene` never consults this flag for it.
            resolves_dependency_usage: false,
            package_test_dirs: Vec::new(),
            // No builtin type facts yet: this adapter declares none, and an empty table
            // simply means the chain resolver has no second tier to consult for it.
            builtin_member_types: Vec::new(),
        }
    }

    fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
        let p = path.0.as_str();
        if !p.ends_with(".css") && !p.ends_with(".scss") {
            return None;
        }
        Some(FileClaim {
            language: SmolStr::new("css"),
            // No path-pattern-driven role/origin split: every claimed file is
            // production; origin is corrected at extract time by generated-marker detection.
            class: FileClass {
                role: FileRole::Production,
                origin: FileOrigin::Authored,
            },
        })
    }

    fn extract(&self, file: &SourceFile<'_>) -> FileFacts {
        extraction::extract(file.path.0.as_str(), file.content)
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
    fn claims_css_and_scss_files_and_rejects_others() {
        let a = CssAdapter;
        assert!(a.claim(&path("styles/main.css")).is_some());
        assert!(a.claim(&path("styles/main.scss")).is_some());
        assert!(a.claim(&path("styles/main.less")).is_none());
        assert!(a.claim(&path("data.json")).is_none());
    }

    #[test]
    fn claim_manifest_is_always_false() {
        let a = CssAdapter;
        assert!(!a.claim_manifest(&path("main.css")));
    }

    #[test]
    fn claimed_files_are_always_production_and_authored() {
        let a = CssAdapter;
        let claim = a.claim(&path("src/main.scss")).unwrap();
        assert_eq!(claim.class.role, FileRole::Production);
        assert_eq!(claim.class.origin, FileOrigin::Authored);
    }

    #[test]
    fn the_trait_surface_delegates_end_to_end() {
        let a = CssAdapter;
        let d = a.descriptor();
        assert_eq!(d.id, "css");
        assert!(d.visibility_ladder.is_empty());
        assert!(d.manifest_globs.is_empty());
        assert_eq!(d.file_globs.len(), 2);

        let src_path = path("src/main.css");
        let facts = a.extract(&SourceFile {
            path: &src_path,
            content: b":root { --brand: red; }\n.btn { color: var(--brand); }\n",
        });
        assert!(facts.declarations.iter().any(|d| d.name == "--brand"));
        assert!(facts.diagnostics.is_empty());

        let known: rustc_hash::FxHashSet<ProjectPath> =
            [path("src/main.css"), path("src/base.css")]
                .into_iter()
                .collect();
        let ctx = ResolveCtx::new(&known);
        let resolved = a.resolve(
            &ImportSpec {
                specifier: SmolStr::new("./base.css"),
                from: path("src/main.css"),
            },
            &ctx,
        );
        assert_eq!(
            resolved,
            Resolution::File(path("src/base.css"), kndo_core::vocab::Confidence::Certain)
        );
    }
}
