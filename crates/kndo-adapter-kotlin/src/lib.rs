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
use kndo_contract::plugin::{FileRole, Plugin, PluginSpec, Rung, Step};
use kndo_contract::vocab::ProjectPath;

pub struct KotlinAdapter {
    spec: PluginSpec,
}

/// What Kotlin's own language dispatches on, as data. Frameworks are NOT here:
/// JUnit's `@Test`, Spring's stereotypes and Compose's `@Composable` are their
/// packs' rules to state, gated by the dependency that proves the framework is
/// installed.
fn dispatch_rules() -> Vec<kndo_contract::plugin::DispatchRule> {
    use kndo_contract::evidence::RootKind;
    use kndo_contract::evidence::SymbolKind;
    use kndo_contract::plugin::{DispatchRule, Effect, Trigger};
    use kndo_contract::vocab::Confidence;

    // `override` and `operator` are reported as markers by the extractor —
    // both are dispatch the call site never spells. A member that implements a
    // promise its owner made is a WITNESS: alive while the owner is, and of no
    // colour, because nothing outside is ENTERED through it.
    let witness_modifier = |keyword: &'static str| DispatchRule {
        when: Trigger::Marker {
            path: keyword.into(),
            arg: None,
            target: None,
        },
        then: Effect::Witness,
        confidence: Confidence::Certain,
    };
    vec![
        witness_modifier("override"),
        witness_modifier("operator"),
        // A TOP-LEVEL `fun main` is the JVM launcher's entry — `Trigger::Name`
        // fires on owner-less declarations only, which is that rule exactly.
        // Probable rather than Certain because Kotlin's launcher accepts
        // several signatures and the name alone is what this trigger sees.
        DispatchRule {
            when: Trigger::Name {
                pattern: "main".into(),
                kind: Some(SymbolKind::Function),
                in_unit: None,
            },
            then: Effect::Root(RootKind::Production),
            confidence: Confidence::Probable,
        },
    ]
}

impl KotlinAdapter {
    pub fn new() -> Self {
        KotlinAdapter {
            // 14: the annotations a declaration carries and the supertypes it
            // promises are evidence; what one MEANS is a dispatch rule's.
            spec: kndo_toolkit::jvm_manifest::jvm_builder("kndo:kotlin", 17, &["kt"])
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
                // Kotlin's `package` clause is as free of the directory as
                // Java's, and more often takes the freedom.
                .nesting(kndo_contract::plugin::Nesting::ByUnit)
                // What this adapter WRITES, so an absence stays typed: the
                // annotations a declaration carries, the supertypes it
                // promises, and a function's shape. A stream it does not
                // declare is dropped at the sink rather than silently missing.
                .emits(kndo_contract::evidence::EvidenceStreams::of(&[
                    kndo_contract::evidence::EvidenceStream::Markers,
                    kndo_contract::evidence::EvidenceStream::Relations,
                    kndo_contract::evidence::EvidenceStream::Qualifiers,
                ]))
                .ladder(&[
                    Step::for_members(Rung::Owner, "private"),
                    Step::for_free(Rung::File, "private"),
                    Step::for_members(Rung::Heirs, "protected"),
                    Step::new(Rung::Unit, "internal"),
                    Step::new(Rung::Exported, "public"),
                ])
                .dispatch(dispatch_rules())
                .build(),
        }
    }
}

impl Default for KotlinAdapter {
    fn default() -> Self {
        KotlinAdapter::new()
    }
}

impl Plugin for KotlinAdapter {
    fn spec(&self) -> &PluginSpec {
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
