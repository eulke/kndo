use kndo_adapter_python::PythonAdapter;
use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::evidence::Reach;
use kndo_contract::extension::Extension;
use kndo_contract::vocab::ProjectPath;
use std::collections::BTreeSet;

fn path(p: &str) -> ProjectPath {
    ProjectPath::new(p)
}

#[test]
fn absolute_dotted_paths_resolve_through_src_layouts() {
    let files: BTreeSet<ProjectPath> = [
        "src/flask/json/__init__.py",
        "src/flask/helpers.py",
        "src/flask/__init__.py",
    ]
    .iter()
    .map(|p| ProjectPath::new(*p))
    .collect();
    let cx = ResolveContext::new(&files);
    let a = PythonAdapter::new();
    assert_eq!(
        a.resolve(&path("src/flask/app.py"), "flask.helpers", &cx),
        Resolution::File(path("src/flask/helpers.py"))
    );
    assert_eq!(
        a.resolve(&path("src/flask/app.py"), "flask.json", &cx),
        Resolution::File(path("src/flask/json/__init__.py")),
        "a package resolves to its __init__"
    );
    assert_eq!(
        a.resolve(&path("src/flask/app.py"), "werkzeug.routing", &cx),
        Resolution::Unresolved,
        "third-party stays keep-alive"
    );
}

#[test]
fn relative_imports_climb_by_dots() {
    let files: BTreeSet<ProjectPath> = [
        "src/pkg/__init__.py",
        "src/pkg/mod.py",
        "src/pkg/sub/__init__.py",
        "src/pkg/sub/leaf.py",
    ]
    .iter()
    .map(|p| ProjectPath::new(*p))
    .collect();
    let cx = ResolveContext::new(&files);
    let a = PythonAdapter::new();
    assert_eq!(
        a.resolve(&path("src/pkg/sub/leaf.py"), ".leaf", &cx),
        Resolution::File(path("src/pkg/sub/leaf.py")),
    );
    assert_eq!(
        a.resolve(&path("src/pkg/sub/leaf.py"), "..mod", &cx),
        Resolution::File(path("src/pkg/mod.py")),
        "each extra dot climbs one package"
    );
    assert_eq!(
        a.resolve(&path("src/pkg/mod.py"), ".", &cx),
        Resolution::File(path("src/pkg/__init__.py")),
        "a bare dot is the package itself"
    );
    assert_eq!(
        a.resolve(&path("src/pkg/mod.py"), ".sub", &cx),
        Resolution::File(path("src/pkg/sub/__init__.py")),
    );
}

#[test]
fn a_python_file_sees_nothing_without_an_import() {
    let files: BTreeSet<ProjectPath> = ["src/pkg/a.py", "src/pkg/b.py"]
        .iter()
        .map(|p| ProjectPath::new(*p))
        .collect();
    let cx = ResolveContext::new(&files);
    let a = PythonAdapter::new();
    assert!(a.sees(&path("src/pkg/a.py"), &cx).is_empty());
    assert!(
        a.seen_from(&path("src/pkg/a.py"), &Reach::Unit, &cx)
            .is_none()
    );
}
