//! What each Python manifest STATES, read as TOML and INI. The source-root
//! answers here are the ones `tests/captured/tooling.json` recorded from
//! setuptools itself; flit, poetry and hatch are read from their documented
//! keys, and the ledger in `EXPERIMENTS.md` carries what is still owed a
//! capture from those tools.

use kndo_adapter_python::PythonAdapter;
use kndo_contract::adapter::SourceFile;
use kndo_contract::adapter::{DependencyScope, ResolveContext};
use kndo_contract::manifest::{ManifestEvidence, ManifestSink, Publication, UnitKind};
use kndo_contract::plugin::Plugin;
use kndo_contract::vocab::ProjectPath;
use std::collections::BTreeSet;

fn read(manifest_path: &str, content: &str, tree: &[&str]) -> ManifestEvidence {
    let known: BTreeSet<ProjectPath> = tree.iter().map(|p| ProjectPath::new(*p)).collect();
    let cx = ResolveContext::new(&known);
    let path = ProjectPath::new(manifest_path);
    let mut sink = ManifestSink::new();
    PythonAdapter::new().extract_manifest(
        &SourceFile {
            path: &path,
            content: content.as_bytes(),
            region: None,
        },
        &cx,
        &mut sink,
    );
    sink.finish()
}

fn roots_of(e: &ManifestEvidence, name: &str) -> Vec<String> {
    e.units
        .iter()
        .find(|u| u.name == name)
        .map(|u| u.roots.iter().map(|r| r.path.to_string()).collect())
        .unwrap_or_default()
}

#[test]
fn each_backend_names_its_own_source_root() {
    // setuptools' `package-dir`, the answer `read_configuration` returns as
    // `{"": "src"}`.
    let e = read(
        "pyproject.toml",
        "[project]\nname = \"demo\"\n[tool.setuptools]\npackage-dir = {\"\" = \"src\"}\n",
        &[],
    );
    assert_eq!(roots_of(&e, "demo"), ["src"]);

    // setuptools' `packages.find.where`, which `read_configuration` leaves in
    // the file (discovery runs at build time, not config time) — so the file
    // is where it is read from.
    let e = read(
        "pyproject.toml",
        "[project]\nname = \"demo\"\n[tool.setuptools.packages.find]\nwhere = [\"src\", \"plugins\"]\n",
        &[],
    );
    assert_eq!(roots_of(&e, "demo"), ["src", "plugins"]);

    // poetry names the package and the directory holding it; the ROOT is the
    // directory.
    let e = read(
        "pyproject.toml",
        "[tool.poetry]\nname = \"demo\"\npackages = [{include = \"demo\", from = \"src\"}]\n",
        &[],
    );
    assert_eq!(roots_of(&e, "demo"), ["src"]);

    // hatch names the package WITH its directory, so the root is the parent.
    let e = read(
        "pyproject.toml",
        "[project]\nname = \"demo\"\n[tool.hatch.build.targets.wheel]\npackages = [\"src/demo\"]\n",
        &[],
    );
    assert_eq!(roots_of(&e, "demo"), ["src"]);
}

#[test]
fn where_no_backend_speaks_the_tree_answers() {
    // flit states the module name and nothing about where it sits, which is
    // the same question setuptools' auto-discovery asks: `src/` when the tree
    // has one.
    let e = read(
        "pyproject.toml",
        "[project]\nname = \"Flask\"\n[tool.flit.module]\nname = \"flask\"\n",
        &["src/flask/__init__.py"],
    );
    assert_eq!(roots_of(&e, "Flask"), ["src"]);

    // The `src` directory's EXISTENCE is the rule, not a name that matches the
    // distribution's — setuptools ships `pkg` from a project called
    // `type-checking-cycle` without a word about either.
    let e = read(
        "pyproject.toml",
        "[project]\nname = \"type-checking-cycle\"\n",
        &["src/pkg/__init__.py"],
    );
    assert_eq!(roots_of(&e, "type-checking-cycle"), ["src"]);

    // The same manifest over a flat layout: the manifest's own directory.
    let e = read(
        "pyproject.toml",
        "[project]\nname = \"Flask\"\n[tool.flit.module]\nname = \"flask\"\n",
        &["flask/__init__.py"],
    );
    assert_eq!(roots_of(&e, "Flask"), [""]);

    // A nested manifest's roots are relative to ITS directory.
    let e = read(
        "examples/celery/pyproject.toml",
        "[project]\nname = \"task-app\"\n[tool.flit.module]\nname = \"task_app\"\n",
        &["examples/celery/src/task_app/__init__.py"],
    );
    assert_eq!(roots_of(&e, "task-app"), ["examples/celery/src"]);
}

#[test]
fn every_table_that_declares_a_requirement_is_read_under_its_own_scope() {
    let e = read(
        "pyproject.toml",
        r#"
[project]
name = "demo"
dependencies = ["blinker>=1.9.0", "click >= 8.1.3"]
[project.optional-dependencies]
async = ["asgiref>=3.2"]
[dependency-groups]
dev = ["ruff", "gha-update ; python_full_version >= '3.12'"]
"#,
        &[],
    );
    let named: Vec<(String, Option<DependencyScope>)> = e
        .dependencies
        .iter()
        .map(|d| (d.name.to_string(), d.scope))
        .collect();
    assert_eq!(
        named,
        vec![
            ("blinker".to_string(), Some(DependencyScope::Prod)),
            ("click".to_string(), Some(DependencyScope::Prod)),
            ("asgiref".to_string(), Some(DependencyScope::Optional)),
            ("ruff".to_string(), Some(DependencyScope::Dev)),
            ("gha-update".to_string(), Some(DependencyScope::Dev)),
        ],
        "the runtime table is what an install pulls in; an extra is a gate the \
         CONSUMER opens; a PEP 735 group is for developing this project and no \
         install of it carries either"
    );

    // poetry states requirements as a TABLE, and `python` is the interpreter.
    let e = read(
        "pyproject.toml",
        "[tool.poetry]\nname = \"demo\"\n[tool.poetry.dependencies]\npython = \"^3.11\"\nclick = \"^8\"\n\
         [tool.poetry.group.dev.dependencies]\npytest = \"*\"\n",
        &[],
    );
    let named: Vec<(String, Option<DependencyScope>)> = e
        .dependencies
        .iter()
        .map(|d| (d.name.to_string(), d.scope))
        .collect();
    assert_eq!(
        named,
        vec![
            ("click".to_string(), Some(DependencyScope::Prod)),
            ("pytest".to_string(), Some(DependencyScope::Dev)),
        ]
    );
}

#[test]
fn a_console_script_is_an_entry_and_testpaths_are_a_test_unit() {
    let e = read(
        "pyproject.toml",
        r#"
[project]
name = "Flask"
[project.scripts]
flask = "flask.cli:main"
[tool.flit.module]
name = "flask"
[tool.pytest.ini_options]
testpaths = ["tests"]
"#,
        &["src/flask/__init__.py", "src/flask/cli.py"],
    );
    let unit = e
        .units
        .iter()
        .find(|u| u.name == "Flask")
        .expect("the unit");
    assert_eq!(
        unit.entries.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
        ["src/flask/__init__.py", "src/flask/cli.py"],
        "the package door `import flask` opens, and the module half of \
         `pkg.mod:func`"
    );
    let tests = e
        .units
        .iter()
        .find(|u| u.name == "pytest")
        .expect("pytest's own unit");
    assert_eq!(tests.kind, UnitKind::Test);
    assert_eq!(
        tests
            .roots
            .iter()
            .map(|r| r.path.as_str())
            .collect::<Vec<_>>(),
        ["tests"]
    );
}

#[test]
fn publication_is_stated_refused_or_unsaid() {
    let published = read("pyproject.toml", "[project]\nname = \"demo\"\n", &[]);
    assert_eq!(published.units[0].publication, Publication::ByName);

    // The one classifier the index itself refuses an upload for.
    let private = read(
        "pyproject.toml",
        "[project]\nname = \"demo\"\nclassifiers = [\"Private :: Do Not Upload\"]\n",
        &[],
    );
    assert_eq!(private.units[0].publication, Publication::Unpublished);

    // No `[project]` table at all: poetry named the distribution, and nothing
    // said whether it is uploaded.
    let unsaid = read("pyproject.toml", "[tool.poetry]\nname = \"demo\"\n", &[]);
    assert_eq!(unsaid.units[0].publication, Publication::Unstated);
}

#[test]
fn a_tool_only_pyproject_declares_no_unit() {
    let e = read(
        "pyproject.toml",
        "[tool.ruff]\nline-length = 88\n[tool.pytest.ini_options]\ntestpaths = [\"tests\"]\n",
        &[],
    );
    assert!(
        e.units.iter().all(|u| u.kind == UnitKind::Test),
        "no distribution here — only pytest's own statement about its directories"
    );
    assert!(e.packages.is_empty());
}

#[test]
fn setup_cfg_states_the_same_things_in_ini() {
    // The values `setuptools.config.setupcfg.read_configuration` returned for
    // this exact file: package_dir {"": "src"}, install_requires
    // ["blinker>=1.9.0", "click"], extras_require {dev: ["pytest"]}.
    let e = read(
        "setup.cfg",
        r#"[metadata]
name = demo
version = 1.0
[options]
package_dir =
    = src
packages = find:
install_requires =
    blinker>=1.9.0
    click
[options.packages.find]
where = src
[options.extras_require]
dev =
    pytest
"#,
        &[],
    );
    assert_eq!(roots_of(&e, "demo"), ["src"]);
    let named: Vec<(String, Option<DependencyScope>)> = e
        .dependencies
        .iter()
        .map(|d| (d.name.to_string(), d.scope))
        .collect();
    assert_eq!(
        named,
        vec![
            ("blinker".to_string(), Some(DependencyScope::Prod)),
            ("click".to_string(), Some(DependencyScope::Prod)),
            ("pytest".to_string(), Some(DependencyScope::Optional)),
        ]
    );
}

#[test]
fn a_requirements_file_declares_dependencies_and_no_unit() {
    let e = read(
        "requirements.txt",
        "# pinned for CI\n-r base.txt\n--index-url https://example.invalid\nblinker>=1.9.0\nclick\n\n",
        &[],
    );
    assert!(e.units.is_empty() && e.packages.is_empty());
    assert_eq!(
        e.dependencies
            .iter()
            .map(|d| d.name.to_string())
            .collect::<Vec<_>>(),
        ["blinker", "click"],
        "an option line is not a requirement"
    );
}

#[test]
fn a_named_package_dir_key_is_the_units_namespace_root_and_its_own_door() {
    // `package-dir = {"" = "src"}` maps the ROOT package to a directory that
    // CONTAINS the packages, so every module's dotted path is already in its
    // path and the unit hangs under nothing.
    let e = read(
        "pyproject.toml",
        "[project]\nname = \"demo\"\n[tool.setuptools]\npackage-dir = {\"\" = \"src\"}\n",
        &["src/demo/__init__.py", "src/demo/api.py"],
    );
    let unit = e.units.iter().find(|u| u.name == "demo").expect("declared");
    assert_eq!(unit.namespace_root, None);
    assert_eq!(
        unit.entries.iter().map(|e| e.as_str()).collect::<Vec<_>>(),
        ["src/demo/__init__.py"],
        "`import demo` runs the initializer of the package UNDER the root"
    );

    // A NAMED key maps one package onto the directory itself: `lib/api.py` is
    // the module `mypkg.api`, `mypkg` is nowhere in the path, and the file
    // `import mypkg` runs is the root's own initializer.
    let e = read(
        "pyproject.toml",
        "[project]\nname = \"mypkg\"\n[tool.setuptools]\npackage-dir = {\"mypkg\" = \"lib\"}\n",
        &["lib/__init__.py", "lib/api.py"],
    );
    let unit = e
        .units
        .iter()
        .find(|u| u.name == "mypkg")
        .expect("declared");
    assert_eq!(roots_of(&e, "mypkg"), ["lib"]);
    assert_eq!(unit.namespace_root.as_deref(), Some("mypkg"));
    assert_eq!(
        unit.entries.iter().map(|e| e.as_str()).collect::<Vec<_>>(),
        ["lib/__init__.py"]
    );
}

#[test]
fn a_distribution_answers_to_every_spelling_pep_503_normalizes() {
    // PyPI matches on the normal form, so a requirement spelled
    // `Flask_SQLAlchemy` and a distribution named `Flask-SQLAlchemy` are one
    // package — and the underscore spelling is the one an import uses.
    let e = read(
        "pyproject.toml",
        "[project]\nname = \"Flask_SQLAlchemy\"\nversion = \"1.0\"\n",
        &[],
    );
    let package = e.packages.first().expect("the distribution");
    assert_eq!(package.name, "Flask_SQLAlchemy", "the name as WRITTEN");
    assert_eq!(
        package
            .aliases
            .iter()
            .map(|a| a.as_str())
            .collect::<Vec<_>>(),
        ["flask-sqlalchemy", "flask_sqlalchemy"],
        "the normal form and the spelling an import uses, never the name itself"
    );

    // A name already in the normal form answers to itself alone.
    let e = read(
        "pyproject.toml",
        "[project]\nname = \"flask\"\nversion = \"1.0\"\n",
        &[],
    );
    assert!(e.packages[0].aliases.is_empty());
}

#[test]
fn a_requirement_is_its_specifier_and_the_range_that_specifier_reads_as() {
    // What `version-skew` compares. A clause this cannot map keeps its text and
    // no range, and a requirement with no specifier at all states nothing —
    // both stop a comparison rather than inventing one.
    let read = |spec: &str| {
        let evidence = kndo_testkit::manifest_evidence(
            &PythonAdapter::new(),
            "pyproject.toml",
            &format!("[project]\nname = \"d\"\ndependencies = [\"{spec}\"]\n"),
            &[],
        );
        evidence.dependencies.first().cloned().expect("declared")
    };
    let range = |spec: &str| read(spec).version_req.and_then(|r| r.range);
    let v = kndo_contract::manifest::Version::new;

    assert_eq!(
        range("d>=2.0").map(|r| r.0),
        Some(v(2, 0, 0)),
        ">= is a floor"
    );
    assert_eq!(
        range("d<3").map(|r| r.1),
        Some(v(3, 0, 0)),
        "< is a ceiling"
    );
    // `==1.4.*` is every 1.4 release; `==1.4.2` is that release alone.
    assert_eq!(range("d==1.4.*"), Some((v(1, 4, 0), v(1, 5, 0))));
    assert_eq!(range("d==1.4.2"), Some((v(1, 4, 2), v(1, 4, 3))));
    // PEP 440's compatible release: `~=1.4.2` admits 1.4.x from 1.4.2 on.
    assert_eq!(range("d~=1.4.2"), Some((v(1, 4, 2), v(1, 5, 0))));
    assert_eq!(range("d~=1.4"), Some((v(1, 4, 0), v(2, 0, 0))));
    assert_eq!(range("d>=2.0,<3"), Some((v(2, 0, 0), v(3, 0, 0))));

    // Extras belong to the name and a marker to the environment: neither asks
    // anything of the version.
    assert_eq!(range("d[redis]==5.2.7"), Some((v(5, 2, 7), v(5, 2, 8))));
    assert_eq!(
        range("d>=1.0 ; python_version < '3.9'"),
        Some((v(1, 0, 0), v(u64::MAX, u64::MAX, u64::MAX)))
    );

    // Nothing to compare: a bare name, and a direct URL reference.
    assert!(read("d").version_req.is_none());
    assert!(
        read("d @ https://example.invalid/d.whl")
            .version_req
            .is_none()
    );
    // A clause with no bound keeps the text and refuses the range.
    let excluded = read("d!=2.0").version_req.expect("spelled");
    assert_eq!(excluded.spelled, "!=2.0");
    assert_eq!(
        excluded.range,
        Some((v(0, 0, 0), v(u64::MAX, u64::MAX, u64::MAX)))
    );
}
