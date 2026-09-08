use kndo_adapter_python::PythonAdapter;
use kndo_contract::evidence::{
    Attachment, FileEvidence, ImportShape, ImportTarget, MarkerTarget, Reach, RefKind, RootKind,
    RootTarget, SymbolKind, Timing,
};
use kndo_contract::vocab::Confidence;
use kndo_testkit::{declaration_named, extract_evidence};

fn ev(path: &str, src: &str) -> FileEvidence {
    extract_evidence(&PythonAdapter::new(), path, src)
}

#[test]
fn convention_is_the_whole_ladder() {
    let e = ev(
        "src/app/mod.py",
        r#"MAX = 10
_cache = {}

class Widget:
    tag = 1
    def render(self):
        pass
    def _hidden(self):
        pass

def _module_helper():
    pass
"#,
    );
    assert_eq!(declaration_named(&e, "MAX").kind, SymbolKind::Constant);
    assert_eq!(declaration_named(&e, "MAX").reach, Reach::Exported);
    // PEP 8's "internal use" reaches the MODULE, which is what a Python
    // namespace is. A sibling that names it does so explicitly, and its own
    // binding or qualifier is what keeps the declaration.
    assert_eq!(
        declaration_named(&e, "_cache").reach,
        Reach::Namespace { up: 0 }
    );
    assert_eq!(declaration_named(&e, "Widget").reach, Reach::Exported);
    assert_eq!(
        declaration_named(&e, "_hidden").reach,
        Reach::Namespace { up: 0 }
    );
    assert_eq!(
        declaration_named(&e, "_module_helper").reach,
        Reach::Namespace { up: 0 }
    );
    let widget = e
        .declarations_with_ids()
        .find(|(_, d)| d.name == "Widget")
        .map(|(id, _)| id);
    assert_eq!(declaration_named(&e, "render").owner, widget);
    assert_eq!(declaration_named(&e, "render").kind, SymbolKind::Method);
}

#[test]
fn constructors_and_locals_never_declare_but_still_walk() {
    let e = ev(
        "src/app/mod.py",
        r#"class Box:
    def __init__(self):
        setup()

def outer():
    def inner():
        used_by_inner()
    return inner
"#,
    );
    assert!(e.declarations.iter().all(|d| d.name != "__init__"));
    assert!(e.declarations.iter().all(|d| d.name != "inner"));
    assert!(e.references.iter().any(|r| r.name == "setup"));
    assert!(e.references.iter().any(|r| r.name == "used_by_inner"));
}

#[test]
fn dispatch_the_source_never_names() {
    let e = ev(
        "src/app/mod.py",
        r#"class P:
    def __repr__(self):
        return "p"

@route
def handler():
    pass

if __name__ == "__main__":
    pass
"#,
    );
    let possible: Vec<usize> = e
        .roots
        .iter()
        .filter(|r| r.confidence == Confidence::Possible)
        .filter_map(|r| match &r.target {
            RootTarget::Declaration(id) => Some(id.index()),
            _ => None,
        })
        .collect();
    let ix = |name: &str| {
        e.declarations_with_ids()
            .find(|(_, d)| d.name == name)
            .map(|(id, _)| id.index())
            .unwrap()
    };
    assert!(
        possible.contains(&ix("__repr__")),
        "runtime protocol dispatch"
    );
    assert!(possible.contains(&ix("handler")), "@d def f IS f = d(f)");
    assert!(
        e.references.iter().any(|r| r.name == "route"),
        "the decorator itself is used"
    );
    assert!(
        e.roots
            .iter()
            .any(|r| matches!(r.target, RootTarget::WholeFile)
                && r.kind == RootKind::Production
                && r.confidence == Confidence::Certain),
        "the __main__ guard is the language's own entry idiom"
    );
}

#[test]
fn test_discovery_convention_roots_files_and_functions() {
    let e = ev(
        "tests/test_app.py",
        r#"def test_render():
    pass

def helper():
    pass
"#,
    );
    // WHICH files the runner collects is the spec's `file_roles` (gated in
    // `kndo-gates`); what extraction states is the membership — the collected
    // module joins the package in a test run and in no other — and the
    // per-function dispatch, which is a fact about this file's names.
    assert_eq!(e.attachment, Attachment::TestOnly);
    assert!(
        !e.roots
            .iter()
            .any(|r| matches!(r.target, RootTarget::WholeFile)),
        "{:?}",
        e.roots
    );
    let test_fn_rooted = e.roots.iter().any(|r| {
        matches!(&r.target, RootTarget::Declaration(id)
            if e.declarations[id.index()].name == "test_render")
            && r.kind == RootKind::Test
    });
    assert!(test_fn_rooted, "the runner dispatches test_* by name");
    let conftest = ev("tests/conftest.py", "def client():\n    pass\n");
    assert_eq!(conftest.attachment, Attachment::TestOnly);

    // A library module concludes NOTHING about itself: which files a
    // distribution publishes is `pyproject.toml`'s to say and the engine's
    // `publishes()` to read, so extraction states only what this file's own
    // bytes carry.
    let lib = ev("src/flask/app.py", "def create_app():\n    pass\n");
    assert_eq!(lib.attachment, Attachment::Regular);
    assert!(lib.roots.is_empty(), "{:?}", lib.roots);

    // …and the language's own entry idiom still is that: `if __name__ ==
    // \"__main__\"` is a statement the file makes, not a path convention.
    let script = ev(
        "src/flask/cli.py",
        "def main():\n    pass\n\nif __name__ == \"__main__\":\n    main()\n",
    );
    assert!(
        script
            .roots
            .iter()
            .any(|r| r.kind == RootKind::Production && matches!(r.target, RootTarget::WholeFile))
    );
}

#[test]
fn import_forms_take_their_shapes() {
    let e = ev(
        "src/app/mod.py",
        r#"import os
import flask.json as fj
from .helpers import make_thing, other as o
from mypkg.sub import *
"#,
    );
    let shapes: Vec<(&ImportTarget, &ImportShape)> =
        e.imports.iter().map(|i| (&i.target, &i.shape)).collect();
    assert_eq!(e.imports.len(), 6, "4 statements + 2 submodule probes");
    assert!(
        matches!(shapes[0], (ImportTarget::Package(p), ImportShape::Namespace { .. }) if p == "os")
    );
    assert!(
        matches!(shapes[1], (ImportTarget::Package(p), ImportShape::Namespace { local }) if p == "flask.json" && local == "fj")
    );
    // Each from-import binding probes its dotted path as a possible submodule.
    assert!(
        matches!(shapes[2], (ImportTarget::Relative(m), ImportShape::Namespace { local }) if m == ".helpers.make_thing" && local == "make_thing")
    );
    assert!(
        matches!(shapes[3], (ImportTarget::Relative(m), ImportShape::Namespace { local }) if m == ".helpers.other" && local == "o")
    );
    match shapes[4] {
        (ImportTarget::Relative(m), ImportShape::Bindings(bs)) => {
            assert_eq!(m, ".helpers");
            assert_eq!(bs.len(), 2);
            assert_eq!(bs[1].imported, "other");
            assert_eq!(bs[1].local, "o");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(shapes[5], (ImportTarget::Package(p), ImportShape::Glob) if p == "mypkg.sub"));
}

#[test]
fn imports_are_collected_wherever_the_language_allows_them() {
    let e = ev(
        "src/app/mod.py",
        r#"import typing as t

if t.TYPE_CHECKING:
    from .types import Hint

try:
    import speedups
except ImportError:
    speedups = None

def handler():
    from .debughelpers import explain
    explain()
"#,
    );
    let named: Vec<&str> = e
        .imports
        .iter()
        .map(|i| match &i.target {
            ImportTarget::Package(p) => p.as_str(),
            ImportTarget::Relative(p) => p.as_str(),
            other => panic!("unexpected target {other:?}"),
        })
        .collect();
    assert_eq!(
        named,
        [
            "typing",
            ".types.Hint",
            ".types",
            "speedups",
            ".debughelpers.explain",
            ".debughelpers",
        ],
        "each nested statement lands, bindings probe their submodule paths"
    );
}

#[test]
fn imports_carry_the_moment_they_run() {
    let e = ev(
        "src/app/mod.py",
        r#"import os
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .types import Hint
else:
    Hint = None
    from .runtime import Real

try:
    import speedups
except ImportError:
    speedups = None

class C:
    import json

def handler():
    from .debughelpers import explain
    explain()
"#,
    );
    let timing = |s: &str| {
        e.imports
            .iter()
            .find(|i| match &i.target {
                ImportTarget::Package(p) | ImportTarget::Relative(p) => p == s,
                _ => false,
            })
            .unwrap_or_else(|| panic!("no import {s}"))
            .timing
    };
    assert_eq!(timing("os"), Timing::Load);
    assert_eq!(
        timing(".types"),
        Timing::Erased,
        "TYPE_CHECKING is False at run time"
    );
    assert_eq!(
        timing(".runtime"),
        Timing::Load,
        "the else branch of the guard runs"
    );
    assert_eq!(
        timing("speedups"),
        Timing::Load,
        "a module-level try runs at load"
    );
    assert_eq!(
        timing("json"),
        Timing::Load,
        "a class body runs while the module loads"
    );
    assert_eq!(
        timing(".debughelpers"),
        Timing::Lazy,
        "a function body runs later"
    );
}

#[test]
fn from_dot_import_probes_the_sibling_module() {
    let e = ev("src/flaskr/__init__.py", "from . import auth\n");
    assert!(
        e.imports.iter().any(|i| matches!(
            (&i.target, &i.shape),
            (ImportTarget::Relative(m), ImportShape::Namespace { local })
                if m == ".auth" && local == "auth"
        )),
        "`from . import auth` reaches the sibling module file: {:?}",
        e.imports
    );
}

#[test]
fn dunder_all_strings_keep_their_names() {
    let e = ev("src/app/mod.py", "__all__ = [\"alpha\", \"beta\"]\n");
    assert!(
        e.references
            .iter()
            .any(|r| r.name == "alpha" && r.kind == RefKind::Read)
    );
    assert!(e.references.iter().any(|r| r.name == "beta"));
    assert!(e.declarations.iter().all(|d| d.name != "__all__"));
}

#[test]
fn class_bases_extend_and_callees_call() {
    let e = ev(
        "src/app/mod.py",
        r#"class Sub(Base):
    pass

def run(w):
    w.render()
    make()
"#,
    );
    let kind_of = |name: &str| e.references.iter().find(|r| r.name == name).unwrap().kind;
    assert_eq!(kind_of("Base"), RefKind::Extend);
    assert_eq!(kind_of("render"), RefKind::Call);
    assert_eq!(kind_of("make"), RefKind::Call);
}

#[test]
fn a_generated_module_reports_its_banner_and_says_everything_else_in_full() {
    let e = ev(
        "src/app/pb.py",
        "# Generated by the protocol buffer compiler.  DO NOT EDIT!\nimport enum\ndef ghost():\n    pass\n",
    );
    assert_eq!(e.markers.len(), 1);
    assert_eq!(e.markers[0].on, MarkerTarget::File);
    assert_eq!(e.markers[0].path, "generated");
    assert_eq!(e.declarations.len(), 1);
    assert_eq!(e.imports.len(), 1);
}

#[test]
fn what_the_runner_collects_joins_the_package_in_a_test_run_alone() {
    for path in [
        "tests/test_widget.py",
        "src/app/widget_test.py",
        "tests/conftest.py",
    ] {
        assert_eq!(
            ev(path, "x = 1\n").attachment,
            Attachment::TestOnly,
            "{path}"
        );
    }
    assert_eq!(
        ev("src/app/widget.py", "x = 1\n").attachment,
        Attachment::Regular
    );
}

#[test]
fn decorators_are_markers_bases_are_relations_and_a_default_is_a_use() {
    let e = ev(
        "app/views.py",
        r#"
import pytest
from base import Model

DEFAULT = 3

@pytest.fixture(scope="module")
def client():
    pass

@app.route("/x")
def index(limit: int = DEFAULT, later: "Model" = None):
    pass

class Widget(Model, metaclass=Meta):
    pass
"#,
    );
    let markers: Vec<(&str, &str)> = e
        .markers
        .iter()
        .filter_map(|m| match m.on {
            MarkerTarget::Declaration(id) => {
                Some((e.declarations[id.index()].name.as_str(), m.path.as_str()))
            }
            _ => None,
        })
        .collect();
    // The path as the source writes it — the engine qualifies it through this
    // file's own bindings, so `pytest.fixture` is JUnit's problem's twin and
    // not a bare name any ecosystem could collide with.
    assert!(
        markers.contains(&("client", "pytest.fixture")),
        "{markers:?}"
    );
    assert!(markers.contains(&("index", "app.route")), "{markers:?}");
    let relations: Vec<(&str, &str)> = e
        .relations
        .iter()
        .map(|r| (e.declarations[r.from.index()].name.as_str(), r.to.as_str()))
        .collect();
    // One base. `metaclass=Meta` configures the class; it is not a supertype.
    assert_eq!(relations, [("Widget", "Model")], "{relations:?}");
    // P3: `limit: int = DEFAULT` binds `limit` and READS `DEFAULT`. Naming the
    // parent kind a binder seat threw the default away.
    let names: Vec<&str> = e.references.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"DEFAULT"), "{names:?}");
}

#[test]
fn a_member_says_what_it_was_read_from_and_a_guarded_def_is_still_surface() {
    let e = ev(
        "app/late.py",
        r#"
import sys
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .models import Later


type Alias = int


def resolve(target: "Later") -> "Later | None":
    return registry.lookup(target)


if sys.version_info >= (3, 12):
    def shim():
        return 1
else:
    def shim():
        return 2
"#,
    );
    // `registry.lookup(...)` was read FROM `registry` — the receiver the member
    // pool needs, and the reason `internal_only` stops abstaining on members.
    let on: Vec<(&str, Option<&str>)> = e
        .references
        .iter()
        .map(|r| (r.name.as_str(), r.on.as_deref()))
        .collect();
    assert!(on.contains(&("lookup", Some("registry"))), "{on:?}");
    assert!(on.contains(&("registry", None)), "{on:?}");
    // A forward annotation is a type by another spelling — quotes are there
    // because the name is not bound YET, not because it is a string.
    assert!(on.contains(&("Later", None)), "{on:?}");
    // PEP 695's `type X = …` declares a name.
    assert_eq!(declaration_named(&e, "Alias").kind, SymbolKind::Type);
    // A def behind a version guard is module surface: the guard decides WHICH
    // definition binds, never whether the name exists.
    assert_eq!(declaration_named(&e, "shim").kind, SymbolKind::Function);
}
