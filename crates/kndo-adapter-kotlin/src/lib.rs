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
use kndo_contract::evidence::{EvidenceSink, RootKind};
use kndo_contract::extension::{Extension, ExtensionSpec, FileRole, Rung, Step};
use kndo_contract::vocab::ProjectPath;

pub struct KotlinAdapter {
    spec: ExtensionSpec,
}

impl KotlinAdapter {
    pub fn new() -> Self {
        KotlinAdapter {
            // 7: the generated banner is reported, never concluded.
            spec: kndo_toolkit::jvm_manifest::jvm_builder("kndo:kotlin", 12, &["kt"])
                // The conventions as data; the library-mode root's replacement
                // is the engine's published surface, read from the unit.
                .file_roles(&[
                    FileRole::certain("src/test/kotlin/**", RootKind::Test),
                    FileRole::certain("**/src/test/kotlin/**", RootKind::Test),
                    FileRole::certain("src/test/java/**", RootKind::Test),
                    FileRole::certain("**/src/test/java/**", RootKind::Test),
                    FileRole::probable("**/*Test.kt", RootKind::Test),
                    FileRole::probable("**/*Tests.kt", RootKind::Test),
                    FileRole::probable("**/*TestCase.kt", RootKind::Test),
                ])
                // A package is one name across the whole compilation, like
                // Java's: `src/test/kotlin/com/foo` and `src/main/kotlin/com/foo`
                // are the same namespace, and the test set's build holds the
                // main set it compiles against.
                .namespace_span(kndo_contract::extension::NamespaceSpan::Compilation)
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

    fn extract_manifest(
        &self,
        manifest: &SourceFile<'_>,
        cx: &ResolveContext<'_>,
        out: &mut kndo_contract::manifest::ManifestSink,
    ) {
        // A pom is a pom whichever JVM language reads it, so both read the
        // same one — the engine keeps one statement per manifest however many
        // adapters claim it.
        kndo_toolkit::jvm_manifest::structure(manifest, cx, out);
    }
}
