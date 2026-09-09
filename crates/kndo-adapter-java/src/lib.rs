//! Java, through the tree-sitter-java grammar. The one Java-shaped idea this
//! adapter is built on: a package is declared (`package com.foo;`) AND the
//! compiler-checked file/directory convention makes it directory-shaped — so
//! imports resolve by PATH SUFFIX (`com.foo.Bar` → `**/com/foo/Bar.java`) and
//! [`Plugin::sees`] is the directory, plus the Maven/Gradle standard
//! layout's test↔main mirror (a test class shares its package with the main
//! classes it exercises, from a parallel source root). Java is nominal, so —
//! unlike Go's structural interfaces — members are declared and judged; the
//! dispatch sites no source line names (`@Override` bodies, serialization
//! hooks, `main`) are rooted instead.
//!
//! The coordinate carries v1's territory (`java`) under the built-in namespace,
//! so oracle comparisons line up file-for-file.

mod extract;
mod resolve;

use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::{EvidenceSink, RootKind};
use kndo_contract::plugin::{FileRole, Plugin, PluginSpec, Rung, Step};
use kndo_contract::vocab::ProjectPath;

pub struct JavaAdapter {
    spec: PluginSpec,
}

/// What Java's annotations and its own runtime mean, as data.
///
/// `@Override` is a WITNESS, not a root: the body is reached through its
/// supertype's contract, so no call site can be required to exist — and
/// nothing outside the graph is ENTERED there, so the file takes no color
/// from it. `Certain`: the compiler rejects the annotation where no supertype
/// declares the member.
///
/// The `java.lang`/`java.io` bases are the same fact for types the project
/// does not contain: their requirements are named here because the graph can
/// never resolve them. `Serializable` is the marker interface whose hooks the
/// serialization runtime calls reflectively — four names, all private by
/// convention, none of them ever called from source.
///
/// `@SuppressWarnings("unused")` exempts: the author answered this analysis's
/// question before it was asked (the owner's 2026-09-05 decision).
/// The marker the extractor reports a JLS launcher signature as — a name this
/// crate owns, so the fact and the rule that reads it cannot drift apart.
pub(crate) const LAUNCHER: &str = "main(String[])";

fn dispatch_rules() -> Vec<kndo_contract::plugin::DispatchRule> {
    use kndo_contract::plugin::{DispatchRule, Effect, Trigger};
    use kndo_contract::vocab::Confidence;
    let witness = |when: Trigger| DispatchRule {
        when,
        then: Effect::Witness,
        confidence: Confidence::Certain,
    };
    let mut rules = vec![
        // `public static void main(String[])` — the JLS's own rule for what
        // the launcher enters, reported by the extractor as a marker because
        // no trigger spells a modifier or a signature. Matched WHOLE, so it is
        // the rule and nothing less: Certain.
        DispatchRule {
            when: Trigger::Marker {
                path: crate::LAUNCHER.into(),
                arg: None,
                target: Some(kndo_contract::evidence::SymbolKind::Method),
            },
            then: Effect::Root(kndo_contract::evidence::RootKind::Production),
            confidence: Confidence::Certain,
        },
        // Only a method can override one: the annotation the compiler allows
        // nowhere else is still a rule's to narrow, not a grammar's.
        witness(Trigger::marker_on(
            "Override",
            kndo_contract::evidence::SymbolKind::Method,
        )),
        kndo_toolkit::jvm_manifest::suppresses_unused("SuppressWarnings"),
    ];
    rules.extend(
        [
            ("Comparable", &["compareTo"][..]),
            ("Comparator", &["compare"]),
            ("Iterable", &["iterator"]),
            ("Iterator", &["hasNext", "next", "remove"]),
            ("Runnable", &["run"]),
            ("Callable", &["call"]),
            ("AutoCloseable", &["close"]),
            ("Closeable", &["close"]),
            ("Cloneable", &["clone"]),
            (
                "Serializable",
                &[
                    "readObject",
                    "writeObject",
                    "readResolve",
                    "writeReplace",
                    "readObjectNoData",
                ],
            ),
        ]
        .into_iter()
        .map(|(base, members)| witness(Trigger::required_by(base, members))),
    );
    rules
}

impl JavaAdapter {
    pub fn new() -> Self {
        JavaAdapter {
            // 14: `@Override` states a witness on a METHOD, and the bases
            // are named as the source writes them, qualified by its imports.
            spec: kndo_toolkit::jvm_manifest::jvm_builder("kndo:java", 21, &["java"])
                // The conventions, as DATA the engine applies where no unit
                // spoke for the file — never a root an adapter concluded. The
                // library-mode "every non-test class is importable surface"
                // root is NOT here: its replacement is the engine's own
                // published surface, read from the unit.
                .file_roles(&[
                    FileRole::certain("**/package-info.java", RootKind::Tooling),
                    FileRole::certain("**/module-info.java", RootKind::Tooling),
                    FileRole::certain("src/test/java/**", RootKind::Test),
                    FileRole::certain("**/src/test/java/**", RootKind::Test),
                    FileRole::probable("**/*Test.java", RootKind::Test),
                    FileRole::probable("**/*Tests.java", RootKind::Test),
                    FileRole::probable("**/*TestCase.java", RootKind::Test),
                ])
                .ladder(&[
                    // `private` is class-private and exists for members alone
                    // (a top-level class cannot take it), `public` is
                    // published, and no modifier is the package between them —
                    // the rung Java has no keyword for, which is why it needs
                    // the word its developers use.
                    Step::for_members(Rung::Owner, "private"),
                    Step::new(Rung::Namespace, "package-private"),
                    Step::for_members(Rung::Heirs, "protected"),
                    Step::new(Rung::Exported, "public"),
                ])
                // A package is a NAME, not a place: two artifacts on one
                // classpath contributing to `com.google.common.io` see each
                // other's package-private members, which is how a test module
                // exercises the library it is compiled against.
                // The `package` clause is the namespace, whole: javac reads it
                // from the file, and a package whose directory does not match
                // is unusual, not a different package.
                .nesting(kndo_contract::plugin::Nesting::ByUnit)
                .emits(kndo_contract::evidence::EvidenceStreams::of(&[
                    kndo_contract::evidence::EvidenceStream::Comments,
                    kndo_contract::evidence::EvidenceStream::Metrics,
                    kndo_contract::evidence::EvidenceStream::Markers,
                    kndo_contract::evidence::EvidenceStream::Relations,
                    kndo_contract::evidence::EvidenceStream::Qualifiers,
                ]))
                .dispatch(dispatch_rules())
                .build(),
        }
    }
}

impl Default for JavaAdapter {
    fn default() -> Self {
        JavaAdapter::new()
    }
}

impl Plugin for JavaAdapter {
    fn spec(&self) -> &PluginSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        let language = tree_sitter_java::LANGUAGE.into();
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
        kndo_toolkit::jvm_manifest::structure(manifest, cx, out);
    }
}
