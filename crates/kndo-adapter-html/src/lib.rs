//! HTML adapter — **a document is an entry point, not a module.**
//!
//! Nothing imports a page: a browser loads it, a server renders it, a bundler is handed it. So
//! an HTML file roots itself, and the modules and stylesheets it names become reachable through
//! it. That one rule is the adapter's whole reason to exist.
//!
//! `<script type="module" src="./main.js">` inside an `index.html` is how a bundled app names
//! its real entry module, and nothing else in the project imports that file. Without a root
//! here, reachability has no
//! edge into it and the whole subtree behind it would read as dead. `<script src>` is HTML's own
//! mechanism, no more a bundler's property than `import` is webpack's — which by the layering
//! rule makes resolving it an adapter's job, not a plugin's.
//!
//! **Non-source, like JSON.** No symbols, no visibility ladder, no metrics: a document declares
//! nothing a caller can name. What it contributes is a root and a set of edges.

mod extraction;
mod resolution;

use kndo_core::adapter::{
    AdapterDescriptor, CyclePolicy, CycleTolerance, FileClaim, FileFacts, ImportSpec,
    LanguageAdapter, ProjectPath, Resolution, ResolveCtx, SourceFile,
};
use kndo_core::vocab::{FileClass, FileOrigin, FileRole};
use smol_str::SmolStr;

pub struct HtmlAdapter;

impl LanguageAdapter for HtmlAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            activation: Vec::new(),
            dependencies: Vec::new(),
            id: SmolStr::new("html"),
            facts_schema_version: 1,
            file_globs: vec![SmolStr::new("**/*.html"), SmolStr::new("**/*.htm")],
            // No manifest format of its own. A document declares no package, and claiming one
            // as a manifest would be actively wrong: assembly creates one `PackageNode` per
            // claimed manifest, so every directory holding a page would become its own package
            // and take package-scoped unit keys and dependency ownership with it.
            manifest_globs: vec![],
            // A tag scan, not a grammar: every reference here lives in one attribute of one
            // tag, and HTML's error recovery means a "malformed" document is still one a
            // browser renders — a parse tree would buy structure nothing reads.
            grammar_version: SmolStr::new("tag-scan 1"),
            // HTML has no visibility semantics; visibility analyses skip its files entirely.
            visibility_ladder: vec![],
            // A page referencing a page is a link, not a dependency — normal, and never a
            // structural defect worth reporting.
            cycle_policy: CyclePolicy {
                file_cycles: CycleTolerance::Idiomatic,
                package_cycles: CycleTolerance::Idiomatic,
            },
            // This adapter contributes no manifest and no PackageNode, so dependency_hygiene
            // never consults the flag for it.
            resolves_dependency_usage: false,
            // A document declares nothing a test could call. It is an entry point, and
            // reporting every page as a test blind spot would bury the report.
            declares_units_of_testing: false,
            package_test_dirs: Vec::new(),
            builtin_member_types: Vec::new(),
        }
    }

    fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
        let p = path.0.as_str();
        let name = p.rsplit('/').next().unwrap_or(p);
        if !(name.ends_with(".html") || name.ends_with(".htm")) {
            return None;
        }
        Some(FileClaim {
            language: SmolStr::new("html"),
            class: FileClass {
                // Every document is production: a page is a deliverable. Test fixtures under a
                // test directory are classified by the core's own path rules, which run over
                // this claim — nothing here needs to guess at them.
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
    fn claims_html_and_htm_and_nothing_else() {
        let a = HtmlAdapter;
        assert!(a.claim(&path("index.html")).is_some());
        assert!(a.claim(&path("app/pages/about.htm")).is_some());
        assert!(a.claim(&path("main.js")).is_none());
        assert!(a.claim(&path("style.css")).is_none());
        // Not a substring match on the path.
        assert!(a.claim(&path("html/main.js")).is_none());
    }

    #[test]
    fn claim_manifest_is_always_false() {
        let a = HtmlAdapter;
        assert!(!a.claim_manifest(&path("index.html")));
        assert!(!a.claim_manifest(&path("package.json")));
    }

    #[test]
    fn the_trait_surface_delegates_end_to_end() {
        let a = HtmlAdapter;
        let d = a.descriptor();
        assert_eq!(d.id, "html");
        assert!(d.visibility_ladder.is_empty());
        assert!(d.manifest_globs.is_empty());

        let p = path("app/index.html");
        let facts = a.extract(&SourceFile {
            path: &p,
            content: br#"<script type="module" src="./main.js"></script>"#,
        });
        assert_eq!(facts.roots.len(), 1);
        assert_eq!(facts.imports.len(), 1);
        assert!(facts.declarations.is_empty());

        let known: rustc_hash::FxHashSet<ProjectPath> = [path("app/main.js")].into_iter().collect();
        let ctx = ResolveCtx::new(&known);
        assert_eq!(
            a.resolve(
                &ImportSpec {
                    specifier: SmolStr::new("./main.js"),
                    from: p.clone(),
                },
                &ctx
            ),
            Resolution::File(path("app/main.js"), kndo_core::vocab::Confidence::Certain)
        );
    }
}
