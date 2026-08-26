//! JavaScript/TypeScript adapter: claiming & classification, declaration/import extraction,
//! import resolution, and `package.json` manifest facts.

use kndo_core::adapter::{
    AdapterDescriptor, CyclePolicy, CycleTolerance, FileClaim, FileFacts, ImportSpec,
    LanguageAdapter, ManifestFacts, ProjectPath, Resolution, ResolveCtx, SourceFile,
    VisibilityRung, VisibilityScope,
};
use smol_str::SmolStr;

mod extraction;
mod manifest;
mod parsing;
mod resolution;

pub struct JsTsAdapter;

pub(crate) const EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"];

/// This adapter's classification conventions as data; the matcher and the
/// universal vendored-tree conventions live in the toolkit (one implementation for all
/// adapters). Generated-origin content markers (@generated banners) are an extract-time
/// concern, not a path concern.
const PATH_PATTERNS: kndo_adapter_toolkit::classify::PathPatterns =
    kndo_adapter_toolkit::classify::PathPatterns {
        test_name_markers: &[".test.", ".spec."],
        // `test`/`tests` are mocha's and node:test's default lookup directories — many repos
        // keep plain-named specs there (`test/response.js`), with no name marker to catch them.
        test_dirs: &["__tests__", "__mocks__", "test", "tests"],
        tooling_name_markers: &[".config."],
        tooling_dirs: &[".storybook"],
    };

impl LanguageAdapter for JsTsAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            activation: Vec::new(),
            dependencies: Vec::new(),
            id: SmolStr::new("js-ts"),
            facts_schema_version: 16, // bump whenever the serialized facts shape or the emission semantics change
            file_globs: EXTENSIONS
                .iter()
                .map(|e| SmolStr::new(format!("**/*.{e}")))
                .collect(),
            manifest_globs: vec![
                SmolStr::new("**/package.json"),
                SmolStr::new("**/pnpm-workspace.yaml"),
            ],
            grammar_version: SmolStr::new("tree-sitter-typescript 0.23"),
            // The JS/TS ladder. Extraction emits 0/1; rung 2 ("package surface": reachable
            // through the package's `exports` map) is an assembly-time promotion — declared
            // here so the ladder is complete whenever a symbol carries level 2.
            visibility_ladder: vec![
                VisibilityRung {
                    scope: VisibilityScope::File,
                    label: SmolStr::new("module-local"),
                    surface_transitive: false,
                },
                VisibilityRung {
                    scope: VisibilityScope::Package,
                    label: SmolStr::new("exported"),
                    surface_transitive: true,
                },
                VisibilityRung {
                    scope: VisibilityScope::Public,
                    label: SmolStr::new("package surface"),
                    surface_transitive: true,
                },
            ],
            // JS/TS treats cycles as hazards at both levels — file cycles are
            // init-order bugs (TDZ crashes, partially-initialized modules), and workspace
            // package cycles break publish ordering and standalone installs.
            cycle_policy: CyclePolicy {
                file_cycles: CycleTolerance::Hazard,
                package_cycles: CycleTolerance::Hazard,
            },
            // npm's flat package name IS the import specifier's root segment — resolve()
            // structurally identifies the declared dependency every time.
            resolves_dependency_usage: true,
            package_test_dirs: Vec::new(),
            // No builtin type facts yet: this adapter declares none, and an empty table
            // simply means the chain resolver has no second tier to consult for it.
            builtin_member_types: Vec::new(),
        }
    }

    fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
        let p = path.0.as_str();
        // Extension match covers `.d.ts` too (its final extension is `ts`); the
        // declarations-only handling of `.d.ts` is extraction's concern, not claiming's.
        let ext = p.rsplit('.').next()?;
        if !EXTENSIONS.contains(&ext) {
            return None;
        }
        Some(FileClaim {
            language: SmolStr::new("js-ts"),
            class: kndo_adapter_toolkit::classify::classify(p, &PATH_PATTERNS),
        })
    }

    fn claim_manifest(&self, path: &ProjectPath) -> bool {
        // pnpm-workspace.yaml topology is not parsed — package.json only.
        path.0.rsplit('/').next() == Some("package.json")
    }

    fn extract(&self, file: &SourceFile<'_>) -> FileFacts {
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

    fn claim(path: &str) -> Option<FileClaim> {
        JsTsAdapter.claim(&ProjectPath(SmolStr::new(path)))
    }

    #[test]
    fn claims_by_extension_and_rejects_others() {
        assert!(claim("src/app.ts").is_some());
        assert!(claim("src/comp.tsx").is_some());
        assert!(claim("lib/mod.cjs").is_some());
        assert!(claim("types/global.d.ts").is_some());
        assert!(claim("main.go").is_none());
        assert!(claim("styles.css").is_none());
    }

    #[test]
    fn classifies_role_and_origin_orthogonally() {
        let c = claim("src/billing/tax.ts").unwrap().class;
        assert_eq!(
            (c.role, c.origin),
            (FileRole::Production, FileOrigin::Authored)
        );

        let c = claim("src/billing/tax.spec.ts").unwrap().class;
        assert_eq!(c.role, FileRole::Test);

        let c = claim("src/__tests__/helpers.ts").unwrap().class;
        assert_eq!(c.role, FileRole::Test);

        let c = claim("webpack.config.js").unwrap().class;
        assert_eq!(c.role, FileRole::Tooling);

        // Orthogonality: a vendored test file expresses both axes (role and origin are
        // independent facts — vocab.rs).
        let c = claim("vendor/lib/util.test.js").unwrap().class;
        assert_eq!((c.role, c.origin), (FileRole::Test, FileOrigin::Vendored));
    }
}
