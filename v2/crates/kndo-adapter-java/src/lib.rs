//! Java, through the tree-sitter-java grammar. The one Java-shaped idea this
//! adapter is built on: a package is declared (`package com.foo;`) AND the
//! compiler-checked file/directory convention makes it directory-shaped — so
//! imports resolve by PATH SUFFIX (`com.foo.Bar` → `**/com/foo/Bar.java`) and
//! [`Extension::unit_mates`] is the directory, plus the Maven/Gradle standard
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
use smol_str::SmolStr;

pub struct JavaAdapter {
    spec: ExtensionSpec,
}

impl JavaAdapter {
    pub fn new() -> Self {
        JavaAdapter {
            spec: kndo_toolkit::jvm_manifest::jvm_spec("kndo:java", 2, &["java"]),
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

    fn manifest_dependencies(&self, manifest: &SourceFile<'_>) -> Vec<SmolStr> {
        kndo_toolkit::jvm_manifest::dependencies(manifest)
    }

    fn unit_mates(&self, path: &ProjectPath, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
        resolve::unit_mates(path, cx)
    }
}
