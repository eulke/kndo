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
    DependencyDeclaration, PackageEntry, ProjectRoot, Resolution, ResolveContext, SourceFile,
};
use kndo_contract::evidence::{EvidenceSink, RootKind, RootTarget};
use kndo_contract::extension::{Extension, ExtensionSpec};
use kndo_contract::vocab::{Confidence, ProjectPath};
use tree_sitter::Language;

/// The `.d.ts` fact, spelled once: the type-declaration companion extension that
/// resolution tries after the TS pair, wildcard exports anchor, and JS entries
/// publish beside themselves.
pub(crate) const TYPE_DECLARATION_EXT: &str = "d.ts";

pub struct TypeScriptAdapter {
    spec: ExtensionSpec,
    /// Dotted resolution candidates in TS priority order, derived once from the
    /// spec's declared extensions (with `.d.ts` after the TS pair) — resolution and
    /// manifest logic read this, never a second extension list.
    resolution_exts: Vec<String>,
}

impl TypeScriptAdapter {
    pub fn new() -> Self {
        let spec = kndo_toolkit::source_adapter_builder(
            "kndo:js-ts",
            // 5: package specifiers spelled inside string literals land as
            // `Possible` imports (a runtime-injected `core-js/…` is a use).
            5,
            &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"],
            &["**/package.json"],
            &[],
            // ESM/CJS initialization order makes cycles bite: TDZ errors and
            // partially-initialized modules at run time.
            kndo_contract::extension::CycleTolerance::Hazard,
        )
        // Dropping `export` is the language-checked narrowing: tsc turns any
        // missed external use into a compile error.
        .export_narrowing(kndo_contract::extension::ExportNarrowing::Expressible)
        .build();
        let mut resolution_exts = Vec::new();
        for ext in spec.suffixes() {
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

impl Extension for TypeScriptAdapter {
    fn spec(&self) -> &ExtensionSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        // Convention roots come from the path and the first bytes, before any parse:
        // a test file that fails to parse must still be rooted, or the parse failure
        // would turn into an unreachable-file accusation.
        convention_roots(file, out);
        let language = language_for(file.path);
        if let Some(tree) = kndo_toolkit::parse_reporting(&language, file.content, out) {
            extract::extract(file.content, &tree, out);
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

    fn manifest_dependencies(&self, manifest: &SourceFile<'_>) -> Vec<DependencyDeclaration> {
        manifest::dependencies(manifest)
    }

    fn imports_dependency(&self, specifier: &str, dependency: &str) -> Option<bool> {
        // `lodash/fp` names `lodash`; a scoped name carries its own slash.
        Some(kndo_toolkit::dependency_match::by_path(
            specifier, dependency, "/",
        ))
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
