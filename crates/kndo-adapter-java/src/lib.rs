//! Java, through the tree-sitter-java grammar. The one Java-shaped idea this
//! adapter is built on: a package is declared (`package com.foo;`) AND the
//! compiler-checked file/directory convention makes it directory-shaped — so
//! imports resolve by PATH SUFFIX (`com.foo.Bar` → `**/com/foo/Bar.java`) and
//! [`Extension::sees`] is the directory, plus the Maven/Gradle standard
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
use kndo_contract::evidence::EvidenceSink;
use kndo_contract::extension::{Extension, ExtensionSpec, Rung, Step};
use kndo_contract::vocab::ProjectPath;

pub struct JavaAdapter {
    spec: ExtensionSpec,
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
fn dispatch_rules() -> Vec<kndo_contract::extension::DispatchRule> {
    use kndo_contract::extension::{DispatchRule, Effect, Trigger};
    use kndo_contract::vocab::Confidence;
    let witness = |when: Trigger| DispatchRule {
        when,
        then: Effect::Witness,
        confidence: Confidence::Certain,
    };
    let mut rules = vec![
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
            // Written with the FULL name, which the engine reaches by
            // qualifying `implements Closer` through this file's imports —
            // the same simple name from another package is another type.
            ("com.vendor.Closer", &["shut"]),
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
            spec: kndo_toolkit::jvm_manifest::jvm_builder("kndo:java", 15, &["java"])
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
                .namespace_span(kndo_contract::extension::NamespaceSpan::Compilation)
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

impl Extension for JavaAdapter {
    fn spec(&self) -> &ExtensionSpec {
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
}
