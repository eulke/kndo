//! go.mod → the module unit and an entry-less package: the module path names
//! both, the directory is what subpaths resolve against, there is no file a
//! bare import points at, and the requirements are the dependency declarations.
//! Every directive has two legal spellings and may carry a comment; the reader
//! answers all of them the same way.

use kndo_adapter_go::GoAdapter;
use kndo_contract::manifest::{ManifestEvidence, Publication, UnitDep, UnitKind};

fn read(manifest_path: &str, content: &str) -> ManifestEvidence {
    kndo_testkit::manifest_evidence(&GoAdapter::new(), manifest_path, content, &[])
}

fn packages_of(manifest_path: &str, content: &str) -> Vec<(String, Option<String>, String)> {
    read(manifest_path, content)
        .packages
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

fn dependency_names(content: &str) -> Vec<String> {
    read("go.mod", content)
        .dependencies
        .into_iter()
        .map(|d| d.name.to_string())
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
fn a_module_path_survives_a_comment_and_the_block_spelling() {
    // go.mod's grammar gives every directive a parenthesised form and allows a
    // `//` comment anywhere; `go mod edit` writes both.
    for spelling in [
        "module example.com/m // the API module\n",
        "module (\n\texample.com/m\n)\n",
        "module (\n\texample.com/m // named here\n)\n",
    ] {
        assert_eq!(
            packages_of("go.mod", spelling),
            [("example.com/m".to_string(), None, String::new())],
            "{spelling:?}"
        );
    }
}

#[test]
fn the_module_is_one_published_library_unit_over_its_own_directory() {
    let evidence = read(
        "services/api/go.mod",
        "module example.com/api\n\nrequire example.com/dep v1.0.0\n",
    );
    assert_eq!(evidence.units.len(), 1);
    let unit = &evidence.units[0];
    assert_eq!(unit.name, "example.com/api");
    assert_eq!(unit.kind, UnitKind::Library);
    // No root spelled: the engine reads the manifest's own directory, which is
    // exactly the module's tree.
    assert!(unit.roots.is_empty());
    // No entry: a Go module is entered through import paths, not through a file.
    assert!(unit.entries.is_empty());
    assert_eq!(unit.depends_on, [UnitDep::on("example.com/dep")]);
    // Every package of the module hangs under the `module` line.
    assert_eq!(unit.namespace_root.as_deref(), Some("example.com/api"));
    assert_eq!(unit.publication, Publication::Unstated);
    assert!(
        unit.is_published(),
        "a resolvable module path is importable by anyone who spells it"
    );
}

#[test]
fn a_tool_directive_mentions_the_package_it_names() {
    // Go 1.24's `go get -tool`: the tool is used with no import in any file,
    // and its module stays in `require`.
    let evidence = read(
        "go.mod",
        "module example.com/m\n\nrequire golang.org/x/tools v0.1.0\n\ntool (\n\tgolang.org/x/tools/cmd/stringer\n)\n\ntool honnef.co/go/tools/cmd/staticcheck\n",
    );
    assert_eq!(
        evidence.mentions,
        [
            "golang.org/x/tools/cmd/stringer",
            "honnef.co/go/tools/cmd/staticcheck"
        ]
    );
}

#[test]
fn broken_manifests_declare_nothing() {
    for content in ["go 1.22\n", "modulename\n"] {
        let evidence = read("go.mod", content);
        assert!(evidence.packages.is_empty(), "{content:?}");
        assert!(evidence.units.is_empty(), "{content:?}");
    }
}

#[test]
fn require_lines_report_dependency_names_both_forms() {
    assert_eq!(
        dependency_names(concat!(
            "module example.com/app\n",
            "go 1.22\n",
            "require example.com/single v1.0.0\n",
            "require (\n",
            "\tgithub.com/gin-gonic/gin v1.10.0\n",
            "\tgolang.org/x/sys v0.1.0 // indirect\n",
            "\t// a comment line names nothing\n",
            ")\n",
            "requirement_not_a_keyword v0\n",
        )),
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
    let deps = read(
        "go.mod",
        "module example.com/m\n\nrequire github.com/x/single v1.0.0 // indirect\n\nrequire (\n\tgithub.com/a/b v1.2.3\n\tgithub.com/c/d v0.1.0 // indirect\n)\n",
    )
    .dependencies;
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
