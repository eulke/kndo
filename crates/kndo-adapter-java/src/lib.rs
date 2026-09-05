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
use kndo_contract::extension::{Extension, ExtensionSpec};
use kndo_contract::vocab::ProjectPath;

pub struct JavaAdapter {
    spec: ExtensionSpec,
}

/// What Java's annotations mean, as data. `@Override` is dispatch the source
/// never names — the body is reached through its supertype's contract, so no
/// call site can exist — and `Probable` because an override of a method the
/// project itself declares and nobody calls is still dead, one supertype up.
/// `@SuppressWarnings("unused")` exempts: the author answered this analysis's
/// question before it was asked (the owner's 2026-09-05 decision).
fn dispatch_rules() -> Vec<kndo_contract::extension::DispatchRule> {
    vec![
        kndo_contract::extension::DispatchRule {
            when: kndo_contract::extension::Trigger::marker("Override"),
            then: kndo_contract::extension::Effect::Root(
                kndo_contract::evidence::RootKind::Production,
            ),
            confidence: kndo_contract::vocab::Confidence::Probable,
        },
        kndo_toolkit::jvm_manifest::suppresses_unused("SuppressWarnings"),
    ]
}

impl JavaAdapter {
    pub fn new() -> Self {
        JavaAdapter {
            // 4: annotations are markers, and what they mean is dispatch data.
            spec: kndo_toolkit::jvm_manifest::jvm_builder("kndo:java", 4, &["java"], &["package"])
                .emits(kndo_contract::evidence::EvidenceStreams::of(&[
                    kndo_contract::evidence::EvidenceStream::Comments,
                    kndo_contract::evidence::EvidenceStream::Metrics,
                    kndo_contract::evidence::EvidenceStream::Markers,
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
        (scope == "package").then(|| resolve::package_region(path, cx))
    }
}
