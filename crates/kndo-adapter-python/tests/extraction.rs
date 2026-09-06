use kndo_adapter_python::PythonAdapter;
use kndo_contract::evidence::{
    FileEvidence, ImportShape, ImportTarget, Reach, RefKind, RootKind, RootTarget, SymbolKind,
    Timing,
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
    assert_eq!(declaration_named(&e, "_cache").reach, Reach::File);
    assert_eq!(declaration_named(&e, "Widget").reach, Reach::Exported);
    assert_eq!(declaration_named(&e, "_hidden").reach, Reach::File);
    assert_eq!(declaration_named(&e, "_module_helper").reach, Reach::File);
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
    assert!(e.roots.iter().any(|r| {
        matches!(r.target, RootTarget::WholeFile)
            && r.kind == RootKind::Test
            && r.confidence == Confidence::Certain
    }));
    let test_fn_rooted = e.roots.iter().any(|r| {
        matches!(&r.target, RootTarget::Declaration(id)
            if e.declarations[id.index()].name == "test_render")
            && r.kind == RootKind::Test
    });
    assert!(test_fn_rooted, "the runner dispatches test_* by name");
    let conftest = ev("tests/conftest.py", "def client():\n    pass\n");
    assert!(conftest.roots.iter().any(|r| r.kind == RootKind::Test));
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
fn generated_modules_declare_nothing_accusable() {
    let e = ev(
        "src/app/pb.py",
        "# Generated by the protocol buffer compiler.  DO NOT EDIT!\nimport enum\ndef ghost():\n    pass\n",
    );
    assert!(e.declarations.is_empty());
    assert_eq!(e.imports.len(), 1);
}
