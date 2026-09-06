//! go.mod → an entry-less package: the module path names it, the directory is what
//! subpaths resolve against, and there is no file a bare import points at.

use kndo_adapter_go::GoAdapter;
use kndo_contract::adapter::{ResolveContext, SourceFile};
use kndo_contract::extension::Extension;
use kndo_contract::vocab::ProjectPath;
use std::collections::BTreeSet;

fn packages_of(manifest_path: &str, content: &str) -> Vec<(String, Option<String>, String)> {
    let known: BTreeSet<ProjectPath> = BTreeSet::new();
    let cx = ResolveContext::new(&known);
    let path = ProjectPath::new(manifest_path);
    GoAdapter::new()
        .packages(
            &SourceFile {
                path: &path,
                content: content.as_bytes(),
                region: None,
            },
            &cx,
        )
        .into_iter()
        .map(|p| {
            (
                p.name.to_string(),
                p.entry.map(|e| e.as_str().to_string()),
                p.dir.to_string(),
            )
        })
        .collect()
}

#[test]
fn module_line_declares_an_entryless_package() {
    assert_eq!(
        packages_of(
            "services/api/go.mod",
            "module example.com/api\n\ngo 1.22\n\nrequire example.com/dep v1.0.0\n",
        ),
        [(
            "example.com/api".to_string(),
            None,
            "services/api".to_string()
        )]
    );
    // Root-level module, quoted spelling.
    assert_eq!(
        packages_of("go.mod", "module \"example.com/root\"\n"),
        [("example.com/root".to_string(), None, String::new())]
    );
}

#[test]
fn broken_manifests_declare_nothing() {
    assert!(packages_of("go.mod", "go 1.22\n").is_empty());
    assert!(packages_of("go.mod", "modulename\n").is_empty());
}

#[test]
fn require_lines_report_dependency_names_both_forms() {
    let path = ProjectPath::new("go.mod");
    let content = concat!(
        "module example.com/app\n",
        "go 1.22\n",
        "require example.com/single v1.0.0\n",
        "require (\n",
        "\tgithub.com/gin-gonic/gin v1.10.0\n",
        "\tgolang.org/x/sys v0.1.0 // indirect\n",
        "\t// a comment line names nothing\n",
        ")\n",
        "requirement_not_a_keyword v0\n",
    );
    let deps: Vec<String> = GoAdapter::new()
        .manifest_dependencies(&SourceFile {
            path: &path,
            content: content.as_bytes(),
            region: None,
        })
        .into_iter()
        .map(|d| d.name.to_string())
        .collect();
    assert_eq!(
        deps,
        [
            "example.com/single",
            "github.com/gin-gonic/gin",
            "golang.org/x/sys"
        ]
    );
}

#[test]
fn indirect_requirements_declare_transitive_and_direct_ones_no_scope() {
    use kndo_contract::adapter::DependencyScope;
    let path = ProjectPath::new("go.mod");
    let deps = GoAdapter::new().manifest_dependencies(&SourceFile {
        path: &path,
        content: b"module example.com/m\n\nrequire github.com/x/single v1.0.0 // indirect\n\nrequire (\n\tgithub.com/a/b v1.2.3\n\tgithub.com/c/d v0.1.0 // indirect\n)\n",
        region: None,
    });
    let scope = |name: &str| deps.iter().find(|d| d.name == name).map(|d| d.scope);
    assert_eq!(
        scope("github.com/a/b"),
        Some(None),
        "direct: go.mod states no scope"
    );
    assert_eq!(
        scope("github.com/c/d"),
        Some(Some(DependencyScope::Transitive))
    );
    assert_eq!(
        scope("github.com/x/single"),
        Some(Some(DependencyScope::Transitive)),
        "the single-line form carries the marker too"
    );
}
