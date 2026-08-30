//! The first real language: TypeScript and JavaScript (`.ts`, `.tsx`, `.js`, `.jsx`,
//! `.mjs`, `.cjs`), through the tree-sitter-typescript grammars. The adapter reports
//! evidence only — declarations with reach and export aliases, ESM imports in every
//! shape, keep-alive-biased references, comment spans — and resolves relative
//! specifiers; judgment stays in the engine.
//!
//! The adapter id is `js-ts`, the same id v1 used for this territory, so oracle
//! comparisons line up file-for-file.

mod extract;
mod manifest;
mod resolve;

use kndo_contract::adapter::{
    AdapterSpec, LanguageAdapter, PackageEntry, ProjectRoot, Resolution, ResolveContext, SourceFile,
};
use kndo_contract::evidence::{
    DiagnosticLevel, EvidenceSink, EvidenceStream, EvidenceStreams, RootKind, RootTarget,
};
use kndo_contract::vocab::{Confidence, ProjectPath};
use tree_sitter::Language;

/// The `.d.ts` fact, spelled once: the type-declaration companion extension that
/// resolution tries after the TS pair, wildcard exports anchor, and JS entries
/// publish beside themselves.
pub(crate) const TYPE_DECLARATION_EXT: &str = "d.ts";

pub struct TypeScriptAdapter {
    spec: AdapterSpec,
    /// Dotted resolution candidates in TS priority order, derived once from the
    /// spec's declared extensions (with `.d.ts` after the TS pair) — resolution and
    /// manifest logic read this, never a second extension list.
    resolution_exts: Vec<String>,
}

impl TypeScriptAdapter {
    pub fn new() -> Self {
        // semantics_version 2: the adapter emits Metrics (winnowing fingerprints,
        // cyclomatic, loc) for every function-shaped declaration.
        let spec = AdapterSpec::builder("js-ts", 2)
            .extensions(&["ts", "tsx", "js", "jsx", "mjs", "cjs"])
            .emits(EvidenceStreams::of(&[
                EvidenceStream::Comments,
                EvidenceStream::Metrics,
            ]))
            .manifests(&["**/package.json"])
            .build();
        let mut resolution_exts = Vec::new();
        for ext in spec.extensions() {
            resolution_exts.push(format!(".{ext}"));
            if ext == "tsx" {
                resolution_exts.push(format!(".{TYPE_DECLARATION_EXT}"));
            }
        }
        TypeScriptAdapter {
            spec,
            resolution_exts,
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
        // Convention roots come from the path and the first bytes, before any parse:
        // a test file that fails to parse must still be rooted, or the parse failure
        // would turn into an unreachable-file accusation.
        convention_roots(file, out);
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
        resolve::resolve(from, specifier, cx, &self.resolution_exts)
    }

    fn roots(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<ProjectRoot> {
        manifest::roots(manifest, cx, &self.resolution_exts)
    }

    fn packages(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<PackageEntry> {
        manifest::packages(manifest, cx, &self.resolution_exts)
    }
}

/// Roots the ecosystem's conventions declare without a manifest: a shebang is an
/// executable entry, `*.test.*`/`*.spec.*`/`__tests__/` files are run by the test
/// runner, `*.config.*` and rc-dotfiles are read by their tools. Convention is
/// `Probable`, never `Certain` — only the shebang is the file's own statement.
fn convention_roots(file: &SourceFile<'_>, out: &mut EvidenceSink) {
    if file.content.starts_with(b"#!") {
        out.root(
            RootTarget::WholeFile,
            RootKind::Production,
            Confidence::Certain,
        );
    }
    let path = file.path.as_str();
    let name = path.rsplit('/').next().unwrap_or(path);
    let in_dir = |d: &str| path.contains(&format!("/{d}/")) || path.starts_with(&format!("{d}/"));
    let is_test = in_dir("__tests__")
        || in_dir("test")
        || in_dir("tests")
        || name.contains(".test.")
        || name.contains(".spec.");
    if is_test {
        out.root(RootTarget::WholeFile, RootKind::Test, Confidence::Probable);
    } else if name.contains(".config.") || (name.starts_with('.') && name.contains("rc.")) {
        out.root(
            RootTarget::WholeFile,
            RootKind::Tooling,
            Confidence::Probable,
        );
    }
}
