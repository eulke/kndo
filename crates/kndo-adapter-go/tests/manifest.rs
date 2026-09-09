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
    assert_eq!(unit.publication, Publication::ByName);
    assert!(
        unit.is_published() && unit.publishes_every_export(),
        "a resolvable module path is importable by anyone who spells it, and \
         what they spell is a NAME — so every exported identifier is on the \
         surface and no entry file gates it"
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

#[test]
fn the_indirect_comment_is_the_go_tools_rule_and_not_a_substring() {
    // Six spellings, and `go mod edit -json` was asked which are indirect: the
    // comment, trimmed and cut at its first `;`, must be exactly `indirect`.
    // Read as a substring, three of these came back wrong in both directions —
    // `//indirect` read as direct, and both `// indirect dependency` and a
    // second `// indirect` after another comment read as indirect.
    let ev = read(
        "go.mod",
        "module example.com/edges\n\ngo 1.22\n\nrequire (\n\
         \texample.com/a v1.0.0 // indirect\n\
         \texample.com/b v1.0.0 //indirect\n\
         \texample.com/c v1.0.0 // indirect; needed by a\n\
         \texample.com/d v1.0.0 // indirect dependency\n\
         \texample.com/e v1.0.0 // Indirect\n\
         \texample.com/f v1.0.0 // see https://x/y // indirect\n)\n",
    );
    let transitive = |name: &str| {
        ev.dependencies
            .iter()
            .find(|d| d.name == name)
            .unwrap_or_else(|| panic!("{name} missing: {:?}", ev.dependencies))
            .scope
            == Some(kndo_contract::adapter::DependencyScope::Transitive)
    };
    for name in ["example.com/a", "example.com/b", "example.com/c"] {
        assert!(transitive(name), "{name} is indirect to the go tool");
    }
    for name in ["example.com/d", "example.com/e", "example.com/f"] {
        assert!(!transitive(name), "{name} is direct to the go tool");
    }
}

#[test]
fn a_quoted_path_is_the_path_and_a_slash_inside_a_string_is_not_a_comment() {
    let ev = read(
        "go.mod",
        "module \"example.com/quoted\"\n\ngo 1.22\n\nrequire (\n\t\"example.com/q\" v1.0.0\n)\n",
    );
    assert_eq!(ev.units.len(), 1);
    assert_eq!(ev.units[0].name, "example.com/quoted");
    assert_eq!(ev.dependencies.len(), 1);
    assert_eq!(ev.dependencies[0].name, "example.com/q");
}

#[test]
fn a_block_of_another_verb_states_nothing_about_this_one() {
    // One reader answers every verb, so a block must close before the next
    // opens: an `exclude (` between two `require`s used to leak its contents
    // into whatever verb was being asked for.
    let ev = read(
        "go.mod",
        "module example.com/blocks\n\ngo 1.22\n\n\
         require (\n\texample.com/first v1.0.0\n)\n\n\
         exclude (\n\texample.com/notarequirement v0.9.9\n)\n\n\
         require example.com/second v1.2.0\n",
    );
    let names: Vec<&str> = ev.dependencies.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["example.com/first", "example.com/second"]);
}

/// `go.work` read with the modules it uses beside it, exactly as the engine
/// hands a reader every manifest's content.
fn read_workspace(work: &str, modules: &[(&str, &str)]) -> ManifestEvidence {
    use kndo_contract::adapter::{ResolveContext, SourceFile};
    use kndo_contract::manifest::ManifestSink;
    use kndo_contract::plugin::Plugin;
    use kndo_contract::vocab::ProjectPath;
    let known: std::collections::BTreeSet<ProjectPath> = Default::default();
    let manifests: std::collections::BTreeMap<ProjectPath, &[u8]> = modules
        .iter()
        .map(|(p, c)| (ProjectPath::new(*p), c.as_bytes()))
        .collect();
    let cx = ResolveContext::with_manifests(&known, &manifests);
    let path = ProjectPath::new("go.work");
    let mut sink = ManifestSink::new();
    GoAdapter::new().extract_manifest(
        &SourceFile {
            path: &path,
            content: work.as_bytes(),
            region: None,
        },
        &cx,
        &mut sink,
    );
    sink.finish()
}

#[test]
fn a_workspace_aggregates_the_modules_it_uses_and_mentions_what_they_are_called() {
    // A workspace declares no unit of its own. It says which `go.mod` files the
    // go tool builds together — so a requirement naming a sibling resolves to
    // that sibling — and it names each used MODULE, read from that module's own
    // `module` line: in workspace mode the toolchain resolves a sibling's
    // packages with no `require` anywhere, and a name the project itself
    // supplies is not undeclared.
    let ev = read_workspace(
        "go 1.22\n\nuse (\n\t./moda\n\t./modb\n\t./gone\n)\n",
        &[
            ("moda/go.mod", "module example.com/a\n\ngo 1.22\n"),
            ("modb/go.mod", "module example.com/b\n\ngo 1.22\n"),
        ],
    );
    assert!(
        ev.units.is_empty(),
        "a workspace is not a unit: {:?}",
        ev.units
    );
    let members: Vec<&str> = ev.members.iter().map(|m| m.as_str()).collect();
    assert_eq!(members, ["moda/go.mod", "modb/go.mod"]);
    let mentions: Vec<&str> = ev.mentions.iter().map(|m| m.as_str()).collect();
    assert_eq!(mentions, ["example.com/a", "example.com/b"]);
    // `./gone` has no `go.mod` under it: a `use` naming nothing names nothing,
    // and aggregating an absence would put a phantom in the workspace.
}

#[test]
fn a_single_line_use_is_the_same_directive_as_a_block() {
    let ev = read_workspace(
        "go 1.22\n\nuse ./only\n",
        &[("only/go.mod", "module example.com/only\n")],
    );
    assert_eq!(
        ev.mentions.iter().map(|m| m.as_str()).collect::<Vec<_>>(),
        ["example.com/only"]
    );
}

#[test]
fn a_requirement_is_a_minimum_and_the_module_path_states_its_ceiling() {
    // Go puts the major version in the path from v2 on, so `example.com/x` is
    // v0 and v1 and `example.com/x/v3` is v3 alone. A requirement is therefore
    // the half-open range from the version written to the first major that
    // would be a different module — which is why two requirements of one path
    // can never conflict, exactly as minimal version selection resolves them.
    let ev = read(
        "go.mod",
        "module example.com/m\n\ngo 1.22\n\nrequire (\n\texample.com/x v1.2.0\n\texample.com/y/v3 v3.1.4\n)\n",
    );
    let req = |name: &str| {
        ev.dependencies
            .iter()
            .find(|d| d.name == name)
            .unwrap_or_else(|| panic!("{name} declared"))
            .version_req
            .clone()
            .unwrap_or_else(|| panic!("{name} states a version"))
    };
    let v = kndo_contract::manifest::Version::new;
    assert_eq!(req("example.com/x").spelled, "v1.2.0");
    assert_eq!(req("example.com/x").range, Some((v(1, 2, 0), v(2, 0, 0))));
    assert_eq!(
        req("example.com/y/v3").range,
        Some((v(3, 1, 4), v(4, 0, 0)))
    );
    assert!(
        req("example.com/x")
            .disjoint(&req("example.com/y/v3"))
            .is_some(),
        "both state a range, so a comparison is possible at all"
    );
}
