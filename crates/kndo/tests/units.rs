//! The project's structure comes from what its manifests SAY: a unit names the
//! directories it compiles and the files it is entered through, and the color
//! its entries anchor follows from its kind. No path convention, no per-adapter
//! table — the engine reads one manifest evidence and owns the rest.

mod common;

use common::reported;
use kndo::Category;
use kndo_contract::manifest::UnitKind;
use kndo_testkit::{MockExtension, TempProject};

fn run(project: &TempProject) -> kndo::Snapshot {
    common::analyze(project, vec![Box::new(MockExtension::new())])
}

fn unit_of<'s>(snap: &'s kndo::Snapshot, path: &str) -> Option<&'s str> {
    let file = snap.graph.files.iter().find(|f| f.path.as_str() == path)?;
    let unit = file.unit?;
    Some(snap.graph.project.units[unit as usize].name.as_str())
}

#[test]
fn a_units_kind_decides_the_color_its_entries_anchor() {
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src entries=src/lib.kmock\n\
         unit suite test roots=tests entries=tests/api.kmock\n",
    )
    .file("src/lib.kmock", "pub fn shared\nfn helper\ncall helper\n")
    .file("src/fixtures.kmock", "pub fn sample\n")
    .file("src/orphan.kmock", "fn floats\n")
    .file(
        "tests/api.kmock",
        "import ./../src/lib { shared }\ncall shared\nimport ./../src/fixtures { sample }\ncall sample\n",
    );
    let snap = run(&p);

    // The library entry is production and the test entry is test: the file
    // only the suite reaches is test-only rather than dead, and the file
    // nothing enters at all is unused.
    assert_eq!(reported(&snap, &Category::UNUSED), ["src/orphan.kmock"]);
    assert_eq!(
        reported(&snap, &Category::TEST_ONLY),
        ["src/fixtures.kmock"]
    );

    assert_eq!(unit_of(&snap, "src/lib.kmock"), Some("core"));
    assert_eq!(unit_of(&snap, "tests/api.kmock"), Some("suite"));
    assert_eq!(
        snap.graph.project.units[0].kind,
        UnitKind::Library,
        "units are ordered by (manifest, name): core before suite"
    );
}

#[test]
fn a_file_belongs_to_the_unit_whose_source_root_is_deepest() {
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit outer library roots= entries=app.kmock\n\
         unit inner library roots=vendor/inner excludes=vendor/inner/skip\n",
    )
    .file("app.kmock", "pub fn api\n")
    .file("vendor/inner/lib.kmock", "pub fn deep\n")
    .file("vendor/inner/skip/gen.kmock", "pub fn generated\n");
    let snap = run(&p);

    assert_eq!(unit_of(&snap, "app.kmock"), Some("outer"));
    assert_eq!(unit_of(&snap, "vendor/inner/lib.kmock"), Some("inner"));
    // An exclude removes the file from ITS unit, and the root-directory unit
    // above does not inherit it: the manifest said nothing compiles it.
    assert_eq!(unit_of(&snap, "vendor/inner/skip/gen.kmock"), Some("outer"));
}

#[test]
fn a_manifest_root_carries_its_own_confidence_and_a_unit_entry_is_certain() {
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library entries=lib.kmock\nrun tool.kmock\n",
    )
    .file("lib.kmock", "pub fn api\n")
    .file("tool.kmock", "pub fn generate\n");
    let snap = run(&p);
    let roots = |path: &str| -> Vec<String> {
        snap.graph
            .files
            .iter()
            .find(|f| f.path.as_str() == path)
            .map(|f| {
                f.roots()
                    .map(|r| format!("{:?}/{:?}", r.kind, r.confidence))
                    .collect()
            })
            .unwrap_or_default()
    };
    assert_eq!(roots("lib.kmock"), ["Production/Certain"]);
    assert_eq!(
        roots("tool.kmock"),
        ["Tooling/Probable"],
        "a file the manifest merely runs is not a unit entry"
    );
    assert!(reported(&snap, &Category::UNUSED).is_empty());
}
