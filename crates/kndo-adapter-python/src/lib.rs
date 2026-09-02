//! `kndo:python` — the seventh built-in and the first FRESH-BASELINE language:
//! v1 never spoke it, so every posture below is derived from the language, not
//! quarried. The rules:
//!
//! - Visibility is convention, and the honest mapping is binary: a leading
//!   underscore is the ecosystem's "module-private" (→ Private — an import that
//!   names it anyway still keeps it, the engine's binding rule); everything
//!   else is importable published surface (→ Exported). No enforceable rung
//!   sits between them — `narrowable` stays empty and `internal-only` stays
//!   silent, because "add an underscore" is advice about a convention, not a
//!   language boundary.
//! - The module IS the file: nothing is visible without an import, so
//!   [`Extension::sees`] answers nothing at all — Python is the degenerate
//!   case the default was designed for. Packages re-export through
//!   `__init__.py` imports, which are ordinary edges.
//! - Dispatch the source never names, each with a language-level reason:
//!   a DECORATED definition is handed to its decorator by the language itself
//!   (`@d def f` IS `f = d(f)`) — `Possible`, whatever the decorator does with
//!   it; dunder methods are invoked by the runtime protocol (`str(x)` calls
//!   `__str__`) — `Possible`; the `if __name__ == "__main__"` guard is the
//!   language's own entry idiom — whole-file `Certain`.
//! - Tests ride the ecosystem's discovery convention (`test_*.py`,
//!   `*_test.py`, and the auto-loaded `conftest.py`), and the runners dispatch
//!   `test_*` functions by NAME — those root `Certain` in test files. Fixtures
//!   survive through the file's entry surface (they are exported defs of a
//!   rooted file); pytest's param-injection magic beyond that is framework
//!   knowledge, recorded as a plugin candidate, never baked in here.
//! - Never declared: `__init__`/`__new__`/`__del__` (the constructor posture
//!   every adapter shares) and local defs nested inside functions (locals are
//!   not project surface; their bodies still contribute references).

mod extract;
mod manifest;
mod resolve;

use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::EvidenceSink;
use kndo_contract::extension::{Extension, ExtensionSpec};
use kndo_contract::vocab::ProjectPath;

pub struct PythonAdapter {
    spec: ExtensionSpec,
}

impl PythonAdapter {
    pub fn new() -> Self {
        PythonAdapter {
            spec: kndo_toolkit::source_adapter_spec(
                "kndo:python",
                1,
                &["py"],
                &[
                    "**/pyproject.toml",
                    "**/requirements.txt",
                    "**/requirements-*.txt",
                ],
                &[],
                // Circular imports raise at import time (partially-initialized
                // module AttributeError) — the classic Python hazard.
                kndo_contract::extension::CycleTolerance::Hazard,
            ),
        }
    }
}

impl Default for PythonAdapter {
    fn default() -> Self {
        PythonAdapter::new()
    }
}

impl Extension for PythonAdapter {
    fn spec(&self) -> &ExtensionSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        let language = tree_sitter_python::LANGUAGE.into();
        if let Some(tree) = kndo_toolkit::parse_reporting(&language, file.content, out) {
            extract::extract(file.path, file.content, &tree, out);
        }
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        resolve::resolve(from, specifier, cx)
    }

    fn manifest_dependencies(
        &self,
        manifest: &SourceFile<'_>,
    ) -> Vec<kndo_contract::adapter::DependencyDeclaration> {
        manifest::dependencies(manifest.path.as_str(), manifest.content)
            .into_iter()
            .map(kndo_contract::adapter::DependencyDeclaration::name_only)
            .collect()
    }
}
