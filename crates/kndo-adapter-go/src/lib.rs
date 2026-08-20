//! Go language adapter (docs/adapters/go.md). Second `LanguageAdapter` implementation —
//! deliberately structurally different from JS/TS (RFC 0002 §1's contract claim needs a second,
//! different language to mean anything): no relative imports, visibility is capitalization
//! rather than a keyword, and a package is a directory of files (`FileFacts::unit`, contracts
//! §2 — a core extension this adapter's design surfaced before any Go code was written).

mod extraction;
mod manifest;
mod parsing;
mod resolution;

use kndo_core::adapter::{
    AdapterDescriptor, FileClaim, ImportSpec, LanguageAdapter, ManifestFacts, ProjectPath,
    Resolution, ResolveCtx, SourceFile,
};
use smol_str::SmolStr;

pub struct GoAdapter;

/// `_test.go` is Go's sole, compiler-recognized test convention — a suffix, but `classify`'s
/// name-marker matcher is a substring check, and a marker that ends a file name is exactly a
/// suffix match (nothing can follow `.go`), so no toolkit change is needed to express it. No
/// tooling-role convention worth pattern-matching yet (docs/adapters/go.md §1, §7 open question
/// 4) — `vendor/` is already in the toolkit's universal list, so nothing Go-specific there
/// either.
const PATH_PATTERNS: kndo_adapter_toolkit::classify::PathPatterns =
    kndo_adapter_toolkit::classify::PathPatterns {
        test_name_markers: &["_test.go"],
        test_dirs: &[],
        tooling_name_markers: &[],
        tooling_dirs: &[],
    };

impl LanguageAdapter for GoAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            id: SmolStr::new("go"),
            facts_schema_version: 2, // 2: Declaration.member_of + member roots by qualified name (RFC 0012 §3)
            file_globs: vec![SmolStr::new("**/*.go")],
            manifest_globs: vec![SmolStr::new("**/go.mod")],
            grammar_version: SmolStr::new("tree-sitter-go 0.25"),
        }
    }

    fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
        let p = path.0.as_str();
        if !p.ends_with(".go") {
            return None;
        }
        Some(FileClaim {
            language: SmolStr::new("go"),
            class: kndo_adapter_toolkit::classify::classify(p, &PATH_PATTERNS),
        })
    }

    fn claim_manifest(&self, path: &ProjectPath) -> bool {
        path.0.rsplit('/').next() == Some("go.mod")
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
    fn claims_go_files_and_rejects_others() {
        let a = GoAdapter;
        assert!(a.claim(&path("main.go")).is_some());
        assert!(a.claim(&path("main.py")).is_none());
        assert!(a.claim(&path("go.mod")).is_none()); // manifest, not source
    }

    #[test]
    fn test_suffix_is_the_sole_test_role_signal() {
        let a = GoAdapter;
        let claim = a.claim(&path("pkg/foo_test.go")).unwrap();
        assert_eq!(claim.class.role, FileRole::Test);
        let claim = a.claim(&path("pkg/foo.go")).unwrap();
        assert_eq!(claim.class.role, FileRole::Production);
    }

    #[test]
    fn vendor_directory_is_vendored_origin() {
        let a = GoAdapter;
        let claim = a.claim(&path("vendor/github.com/foo/bar/baz.go")).unwrap();
        assert_eq!(claim.class.origin, FileOrigin::Vendored);
    }

    #[test]
    fn claim_manifest_matches_only_go_mod() {
        let a = GoAdapter;
        assert!(a.claim_manifest(&path("go.mod")));
        assert!(a.claim_manifest(&path("pkg/go.mod")));
        assert!(!a.claim_manifest(&path("go.sum")));
    }
}
