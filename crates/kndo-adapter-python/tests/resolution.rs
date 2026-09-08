use kndo_adapter_python::PythonAdapter;
use kndo_contract::adapter::{ProjectView, Resolution, ResolveContext, UnitView};
use kndo_contract::manifest::{PathAlias, UnitKind, UnitRoot};
use kndo_contract::plugin::Plugin;
use kndo_contract::vocab::ProjectPath;
use std::collections::{BTreeMap, BTreeSet};

fn path(p: &str) -> ProjectPath {
    ProjectPath::new(p)
}

/// The manifests' answer, as `resolve` gets it: which unit compiles which
/// file, the roots it declared, and the name it hangs under. Owned by the
/// caller because the view borrows.
struct Declared {
    units: Vec<UnitView>,
    unit_of: BTreeMap<ProjectPath, u32>,
    aliases: Vec<(smol_str::SmolStr, PathAlias)>,
    namespaces: BTreeMap<ProjectPath, Vec<smol_str::SmolStr>>,
    in_namespace: BTreeMap<Vec<smol_str::SmolStr>, Vec<ProjectPath>>,
}

impl Declared {
    /// One unit over `roots`, compiling every file under them, hanging under
    /// `namespace_root` where a manifest named one.
    fn one(
        roots: &[&str],
        namespace_root: Option<&str>,
        files: &BTreeSet<ProjectPath>,
    ) -> Declared {
        let unit = UnitView {
            name: "dist".into(),
            kind: UnitKind::Library,
            roots: roots.iter().map(|r| UnitRoot::from(*r)).collect(),
            namespace_root: namespace_root.map(Into::into),
            published: true,
            compiles_against: Vec::new(),
        };
        let unit_of = files
            .iter()
            .filter(|f| roots.iter().any(|r| f.is_under(r)))
            .map(|f| (f.clone(), 0u32))
            .collect();
        Declared {
            units: vec![unit],
            unit_of,
            aliases: Vec::new(),
            namespaces: BTreeMap::new(),
            in_namespace: BTreeMap::new(),
        }
    }

    fn view(&self) -> ProjectView<'_> {
        ProjectView::new(
            &self.units,
            &self.unit_of,
            &self.aliases,
            &self.namespaces,
            &self.in_namespace,
        )
    }
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
    // `pyproject.toml` says `src` is the source root; nothing here is guessed
    // from the shape of the tree.
    let declared = Declared::one(&["src"], None, &files);
    let view = declared.view();
    let packages = BTreeMap::new();
    let cx = ResolveContext::with_project(&files, &packages, &view);
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
