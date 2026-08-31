//! Kotlin, through the tree-sitter-kotlin-ng grammar. Kotlin's package is
//! declared but — unlike Java's — NOT compiler-checked against the directory;
//! this adapter leans on the convention anyway (JetBrains' own style enforces
//! it), resolving imports by path suffix with a package-directory fallback for
//! the file-name freedom Kotlin allows (`import a.b.Foo` may live in any
//! `a/b/*.kt`). The unit is the directory — mixed `.kt`/`.java` siblings share
//! one namespace at compile — plus the standard layout's test→main mirror
//! across BOTH source-set spellings (`src/test/kotlin` sees `src/main/kotlin`
//! and `src/main/java`).
//!
//! Visibility defaults to PUBLIC, the opposite of Java's package-private — a
//! load-bearing difference. `internal` (module scope) folds to Exported in the
//! binary reach: wider than a file, narrower than the world, and the analysis
//! that can tell the difference (internal-only) is exactly what the visibility
//! ladder waits for.
//!
//! The coordinate carries v1's territory (`kotlin`) under the built-in
//! namespace, so oracle comparisons line up file-for-file.

mod extract;
mod resolve;

use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::EvidenceSink;
use kndo_contract::extension::{Extension, ExtensionSpec};
use kndo_contract::vocab::ProjectPath;

pub struct KotlinAdapter {
    spec: ExtensionSpec,
}

impl KotlinAdapter {
    pub fn new() -> Self {
        KotlinAdapter {
            spec: kndo_toolkit::jvm_manifest::jvm_spec("kndo:kotlin", 2, &["kt"], &["module"]),
        }
    }
}

impl Default for KotlinAdapter {
    fn default() -> Self {
        KotlinAdapter::new()
    }
}

impl Extension for KotlinAdapter {
    fn spec(&self) -> &ExtensionSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        let language = tree_sitter_kotlin_ng::LANGUAGE.into();
        if let Some(tree) = kndo_toolkit::parse_reporting(&language, file.content, out) {
            extract::extract(file.path, file.content, &tree, out);
        }
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        resolve::resolve(from, specifier, cx)
    }

    fn packages(
        &self,
        manifest: &SourceFile<'_>,
        _cx: &ResolveContext<'_>,
    ) -> Vec<kndo_contract::adapter::PackageEntry> {
        kndo_toolkit::jvm_manifest::packages(manifest)
    }

    fn manifest_dependencies(
        &self,
        manifest: &SourceFile<'_>,
    ) -> Vec<kndo_contract::adapter::DependencyDeclaration> {
        kndo_toolkit::jvm_manifest::dependencies(manifest)
    }

    fn sees(&self, path: &ProjectPath, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
        resolve::sees(path, cx)
    }

    fn seen_from(
        &self,
        path: &ProjectPath,
        scope: &str,
        cx: &ResolveContext<'_>,
    ) -> Option<Vec<ProjectPath>> {
        (scope == "module").then(|| resolve::module_region(path, cx))
    }
}
