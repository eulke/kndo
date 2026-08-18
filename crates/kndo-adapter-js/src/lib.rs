//! JavaScript/TypeScript adapter — normative spec: docs/adapters/js-ts.md.
//!
//! This commit implements §1 (claiming & classification). Extraction, resolution and
//! manifests land next, against the conformance harness.

use kndo_core::adapter::{
    AdapterDescriptor, FileClaim, FileFacts, ImportSpec, LanguageAdapter, ManifestFacts,
    ProjectPath, Resolution, ResolveCtx, SourceFile,
};
use kndo_core::vocab::{FileClass, FileOrigin, FileRole};
use smol_str::SmolStr;

mod extraction;

pub struct JsTsAdapter;

const EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"];

impl JsTsAdapter {
    fn classify(path: &str) -> FileClass {
        let file_name = path.rsplit('/').next().unwrap_or(path);

        // Origin axis (spec §1): vendored trees, then generated markers (content markers like
        // @generated banners are checked at extract time; name-level signals here).
        let origin = if path.starts_with("vendor/")
            || path.contains("/vendor/")
            || path.starts_with("third_party/")
            || path.contains("/third_party/")
        {
            FileOrigin::Vendored
        } else {
            FileOrigin::Authored
        };

        // Role axis (spec §1). Order matters: test markers beat tooling markers.
        let stem_has = |marker: &str| file_name.contains(marker);
        let role = if stem_has(".test.")
            || stem_has(".spec.")
            || path.contains("/__tests__/")
            || path.starts_with("__tests__/")
            || path.contains("/__mocks__/")
            || path.starts_with("__mocks__/")
        {
            FileRole::Test
        } else if stem_has(".config.")
            || path.contains("/.storybook/")
            || path.starts_with(".storybook/")
        {
            FileRole::Tooling
        } else {
            FileRole::Production
        };

        FileClass { role, origin }
    }
}

impl LanguageAdapter for JsTsAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            id: SmolStr::new("js-ts"),
            facts_schema_version: 1,
            file_globs: EXTENSIONS
                .iter()
                .map(|e| SmolStr::new(format!("**/*.{e}")))
                .collect(),
            manifest_globs: vec![
                SmolStr::new("**/package.json"),
                SmolStr::new("**/pnpm-workspace.yaml"),
            ],
            grammar_version: SmolStr::new("tree-sitter-typescript 0.23"),
        }
    }

    fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
        let p = path.0.as_str();
        // `.d.ts` is ours (declarations only — spec §1); plain extension match otherwise.
        let ext = p.rsplit('.').next()?;
        let claimed = p.ends_with(".d.ts") || EXTENSIONS.contains(&ext);
        if !claimed {
            return None;
        }
        Some(FileClaim {
            language: SmolStr::new("js-ts"),
            class: Self::classify(p),
        })
    }

    fn extract(&self, file: &SourceFile<'_>) -> FileFacts {
        extraction::extract(file.path.0.as_str(), file.content)
    }

    fn extract_manifest(&self, _file: &SourceFile<'_>) -> ManifestFacts {
        // Lands next: package.json identity/topology/deps per spec §4.
        ManifestFacts::default()
    }

    fn resolve(&self, _spec: &ImportSpec, _ctx: &ResolveCtx) -> Resolution {
        // Lands next: Node ESM+CJS algorithm per spec §3.
        Resolution::Unresolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // Orthogonality: a vendored test file expresses both axes (the FileFlavor bug we
        // designed away — vocab.rs).
        let c = claim("vendor/lib/util.test.js").unwrap().class;
        assert_eq!((c.role, c.origin), (FileRole::Test, FileOrigin::Vendored));
    }
}
