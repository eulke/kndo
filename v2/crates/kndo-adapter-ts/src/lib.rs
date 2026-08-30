//! The first real language: TypeScript and JavaScript (`.ts`, `.tsx`, `.js`, `.jsx`,
//! `.mjs`, `.cjs`), through the tree-sitter-typescript grammars. The adapter reports
//! evidence only — declarations with reach and export aliases, ESM imports in every
//! shape, keep-alive-biased references, comment spans — and resolves relative
//! specifiers; judgment stays in the engine.
//!
//! The adapter id is `js-ts`, the same id v1 used for this territory, so oracle
//! comparisons line up file-for-file.

mod extract;
mod resolve;

use kndo_contract::adapter::{
    AdapterSpec, LanguageAdapter, Resolution, ResolveContext, SourceFile,
};
use kndo_contract::evidence::{DiagnosticLevel, EvidenceSink, EvidenceStream, EvidenceStreams};
use kndo_contract::vocab::ProjectPath;
use tree_sitter::Language;

pub struct TypeScriptAdapter {
    spec: AdapterSpec,
}

impl TypeScriptAdapter {
    pub fn new() -> Self {
        TypeScriptAdapter {
            spec: AdapterSpec::builder("js-ts", 1)
                .claims(&[
                    "**/*.ts", "**/*.tsx", "**/*.js", "**/*.jsx", "**/*.mjs", "**/*.cjs",
                ])
                .emits(EvidenceStreams::of(&[EvidenceStream::Comments]))
                .build(),
        }
    }
}

impl Default for TypeScriptAdapter {
    fn default() -> Self {
        TypeScriptAdapter::new()
    }
}

/// `.ts` gets the TypeScript grammar (where `<T>` casts are legal); everything else —
/// `.tsx`, `.jsx`, and plain JS in all its extensions — gets TSX, whose JSX support
/// is a superset of what those files can contain.
fn language_for(path: &ProjectPath) -> Language {
    if path.as_str().ends_with(".ts") {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    } else {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    }
}

impl LanguageAdapter for TypeScriptAdapter {
    fn spec(&self) -> &AdapterSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        let language = language_for(file.path);
        match kndo_toolkit::parse(&language, file.content) {
            Some(tree) => {
                if tree.root_node().has_error() {
                    out.diagnostic(
                        DiagnosticLevel::Info,
                        "syntax errors in file — evidence may be partial",
                        None,
                    );
                }
                extract::extract(file.content, &tree, out);
            }
            None => out.diagnostic(
                DiagnosticLevel::Warn,
                "parse produced no tree — no evidence extracted from this file",
                None,
            ),
        }
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        resolve::resolve(from, specifier, cx)
    }
}
