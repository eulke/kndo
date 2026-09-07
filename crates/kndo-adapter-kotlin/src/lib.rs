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
//! load-bearing difference. `internal` is the unit's reach: wider than a file,
//! narrower than the world, bounded from the source-set layout until the
//! module's manifest names its units. The ladder says what each rung is
//! called, and that `private` means the file on a top-level declaration and
//! the class on a member.
//!
//! The coordinate carries v1's territory (`kotlin`) under the built-in
//! namespace, so oracle comparisons line up file-for-file.

mod extract;
mod resolve;

use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::EvidenceSink;
use kndo_contract::evidence::Reach;
use kndo_contract::extension::{Extension, ExtensionSpec, Rung, Step};
use kndo_contract::vocab::ProjectPath;

pub struct KotlinAdapter {
    spec: ExtensionSpec,
}

impl KotlinAdapter {
    pub fn new() -> Self {
        KotlinAdapter {
            // 7: the generated banner is reported, never concluded.
            spec: kndo_toolkit::jvm_manifest::jvm_builder("kndo:kotlin", 7, &["kt"])
                .ladder(&[
                    Step::for_members(Rung::Owner, "private"),
                    Step::for_free(Rung::File, "private"),
                    Step::for_members(Rung::Heirs, "protected"),
                    Step::new(Rung::Unit, "internal"),
                    Step::new(Rung::Exported, "public"),
                ])
                .build(),
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
        reach: &Reach,
        cx: &ResolveContext<'_>,
    ) -> Option<Vec<ProjectPath>> {
        matches!(reach, Reach::Unit { up: 0 }).then(|| resolve::module_region(path, cx))
    }
}
