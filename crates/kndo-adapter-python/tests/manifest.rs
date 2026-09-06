use kndo_adapter_python::PythonAdapter;
use kndo_contract::adapter::SourceFile;
use kndo_contract::extension::Extension;
use kndo_contract::vocab::ProjectPath;

#[test]
fn pyproject_and_requirements_names() {
    let a = PythonAdapter::new();
    let pyproject = br#"[project]
name = "demo"
dependencies = [
    "flask>=3.0",
    "sqlalchemy[asyncio]>=2 ; python_version > '3.9'",
]

[project.optional-dependencies]
dev = [
    "pytest",
]
"#;
    let names = a
        .manifest_dependencies(&SourceFile {
            path: &ProjectPath::new("pyproject.toml"),
            content: pyproject,
            region: None,
        })
        .into_iter()
        .map(|d| d.name)
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["flask", "pytest", "sqlalchemy"]);

    let reqs = b"# comment\nflask==3.0\nblinker>=1.6\n-r other.txt\n";
    let names = a
        .manifest_dependencies(&SourceFile {
            path: &ProjectPath::new("requirements.txt"),
            content: reqs,
            region: None,
        })
        .into_iter()
        .map(|d| d.name)
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["blinker", "flask"]);
}
