//! `kndo:python` — the seventh built-in and the first FRESH-BASELINE language:
//! v1 never spoke it, so every posture below is derived from the language, not
//! quarried. The rules:
//!
//! - Visibility is convention, and the honest mapping is binary: a leading
//!   underscore is the ecosystem's "module-private" (→ Private — an import that
//!   names it anyway still keeps it, the engine's binding rule); everything
//!   else is importable published surface (→ Exported). No enforceable rung
//!   sits between them — the ladder stays empty and `internal-only` stays
//!   silent, because "add an underscore" is advice about a convention, not a
//!   language boundary.
//! - The module IS the file: nothing is visible without an import, so
//!   [`Plugin::sees`] answers nothing at all — Python is the degenerate
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
pub mod manifest;
mod resolve;

use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::EvidenceSink;
use kndo_contract::evidence::RootKind;
use kndo_contract::plugin::{DispatchRule, FileRole, Plugin, PluginSpec};
use kndo_contract::vocab::{Confidence, ProjectPath};

pub struct PythonAdapter {
    spec: PluginSpec,
}

impl PythonAdapter {
    pub fn new() -> Self {
        PythonAdapter {
            // 3: the generated banner is reported, never concluded.
            spec: kndo_toolkit::source_adapter_builder(
                "kndo:python",
                12,
                &["py"],
                &[
                    "**/pyproject.toml",
                    "**/setup.cfg",
                    "**/requirements.txt",
                    "**/requirements-*.txt",
                ],
                // Circular imports raise at import time (partially-initialized
                // module AttributeError) — the classic Python hazard.
                kndo_contract::plugin::CycleTolerance::Hazard,
            )
            // The interpreter's own directory for installed packages — no
            // package can be named `site-packages` — is never the project's
            // source; an environment is found by it, whatever the environment
            // is called, and a hidden `.venv` never enters discovery at all.
            // What a rule reads about a Python declaration: the decorators
            // it carries (a decorated def IS handed to them by the language)
            // and the bases a class promises the surface of.
            .emits(kndo_contract::evidence::EvidenceStreams::of(&[
                kndo_contract::evidence::EvidenceStream::Markers,
                kndo_contract::evidence::EvidenceStream::Relations,
                kndo_contract::evidence::EvidenceStream::Qualifiers,
            ]))
            // A Python module's namespace IS its dotted path under the
            // distribution's source root, and the source root is the
            // manifest's to say — so the engine derives it. Everything the
            // package holds is one node, and `pkg/__init__.py` IS `pkg`.
            // Every source root a Python distribution has is a manifest's to
            // state (`package-dir`, `packages.find.where`, the `src` layout the
            // tree itself shows setuptools) — the language knows none of its own.
            .nesting(kndo_contract::plugin::Nesting::ByPath { roots: Vec::new() })
            .ignores(&["**/site-packages/**"])
            // The runners' own discovery, where no manifest said what a file
            // is: pytest and unittest COLLECT `test_*.py` and `*_test.py` by
            // name and auto-load `conftest.py` — the names themselves are the
            // dispatch, which is what makes these Certain rather than a habit.
            .file_roles(&[
                FileRole::certain("**/test_*.py", RootKind::Test),
                FileRole::certain("**/*_test.py", RootKind::Test),
                FileRole::certain("**/conftest.py", RootKind::Test),
                // `python -m pkg` runs `pkg/__main__.py`: the interpreter's
                // own rule for the filename, and the one entry a manifest
                // never has to declare.
                FileRole::certain("**/__main__.py", RootKind::Production),
            ])
            .dispatch(dispatch_rules())
            .build(),
        }
    }
}

impl Default for PythonAdapter {
    fn default() -> Self {
        PythonAdapter::new()
    }
}

impl Plugin for PythonAdapter {
    fn spec(&self) -> &PluginSpec {
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

    fn extract_manifest(
        &self,
        manifest: &SourceFile<'_>,
        cx: &ResolveContext<'_>,
        out: &mut kndo_contract::manifest::ManifestSink,
    ) {
        manifest::structure(manifest.path, manifest.content, cx, out);
    }
}

/// The file marker a `if __name__ == "__main__":` guard is reported as — the
/// string the source itself compares against, so nothing here is invented.
pub(crate) const MAIN_GUARD: &str = "__main__";

/// What Python's own runtime and runners dispatch on, as data. Every one of
/// these was a branch in the extractor concluding a root from evidence it had
/// just emitted — the adapter reporting a fact and deciding what it means, two
/// lines apart. The fact stays the adapter's; the meaning is a rule.
///
/// Frameworks are NOT here: pytest's collection of a `TestCase` subclass,
/// Django's URL conf, Flask's `@app.route` are their packs' rules to state,
/// gated by the dependency that proves the framework is installed.
fn dispatch_rules() -> Vec<DispatchRule> {
    use kndo_contract::evidence::SymbolKind;
    use kndo_contract::manifest::UnitKind;
    use kndo_contract::plugin::{Effect, Trigger};

    // `@d def f` IS `f = d(f)`: whatever the decorator registers or wraps, the
    // hand-off is a use beyond static sight. WHICH decorator is unknowable
    // without the framework, so the path is any and the confidence is the
    // weakest tier — a keep-alive, never a claim.
    let decorated = |target: SymbolKind| DispatchRule {
        when: Trigger::Marker {
            path: "*".into(),
            arg: None,
            target: Some(target),
        },
        then: Effect::Root(RootKind::Production),
        confidence: Confidence::Possible,
    };
    // A test runner finds `test_*` by NAME, in the files it collects — and
    // which files those are is the unit's kind or the file's own attachment,
    // never this rule's to guess.
    let named_test = |trigger: Trigger| DispatchRule {
        when: trigger,
        then: Effect::Root(RootKind::Test),
        confidence: Confidence::Certain,
    };
    vec![
        // `if __name__ == "__main__":` — the language's own entry idiom,
        // reported as a file marker because that is what the source says.
        DispatchRule {
            when: Trigger::Marker {
                path: MAIN_GUARD.into(),
                arg: None,
                target: None,
            },
            then: Effect::Root(RootKind::Production),
            confidence: Confidence::Certain,
        },
        decorated(SymbolKind::Type),
        decorated(SymbolKind::Function),
        decorated(SymbolKind::Method),
        named_test(Trigger::Name {
            pattern: "test*".into(),
            kind: Some(SymbolKind::Function),
            in_unit: Some(UnitKind::Test),
        }),
        named_test(Trigger::MemberOf {
            owner: Box::new(Trigger::Name {
                pattern: "*".into(),
                kind: Some(SymbolKind::Type),
                in_unit: Some(UnitKind::Test),
            }),
            name: "test*".into(),
        }),
        // The runtime protocol invokes a dunder the source never names —
        // `str(x)` calls `__str__`, `x[i]` calls `__getitem__`. A member one
        // is a promise its owner made, so it is a WITNESS: alive while the
        // type is, and of no color, because nothing outside is ENTERED here.
        DispatchRule {
            when: Trigger::MemberOf {
                owner: Box::new(Trigger::Name {
                    pattern: "*".into(),
                    kind: Some(SymbolKind::Type),
                    in_unit: None,
                }),
                name: "__*__".into(),
            },
            then: Effect::Witness,
            confidence: Confidence::Certain,
        },
        // A MODULE-level dunder has no owner to be alive with: `__getattr__`
        // and `__dir__` are the import machinery's hooks on the module
        // itself, so they root the way any runtime entry does.
        DispatchRule {
            when: Trigger::Name {
                pattern: "__*__".into(),
                kind: Some(SymbolKind::Function),
                in_unit: None,
            },
            then: Effect::Root(RootKind::Production),
            confidence: Confidence::Possible,
        },
    ]
}
