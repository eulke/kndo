//! The project's structure comes from what its manifests SAY: a unit names the
//! directories it compiles and the files it is entered through, and the color
//! its entries anchor follows from its kind. No path convention, no per-adapter
//! table — the engine reads one manifest evidence and owns the rest.

mod common;

use common::reported;
use kndo::Category;
use kndo_contract::manifest::UnitKind;
use kndo_testkit::{MockPlugin, TempProject};

fn run(project: &TempProject) -> kndo::Snapshot {
    common::analyze(project, vec![Box::new(MockPlugin::new())])
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
        "unit core library roots=src entries=src/lib.kmock publish=by-entry\n\
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

#[test]
fn a_friend_unit_may_name_a_units_own_reach() {
    let lib = "pub fn api\nunit fn helper\n";
    let suite = "import ./../src/lib { api }\ncall api\ncall helper\n";
    // Declared a friend: the suite's bare `helper` is the library's.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit lib library roots=src entries=src/lib.kmock\n\
         unit suite test roots=tests entries=tests/t.kmock needs=lib friends=lib\n",
    )
    .file("src/lib.kmock", lib)
    .file("tests/t.kmock", suite);
    let snap = run(&p);
    assert!(
        reported(&snap, &Category::UNUSED).is_empty(),
        "{:?}",
        reported(&snap, &Category::UNUSED)
    );
    // Not a friend: the suite cannot name it, so its `helper` is somebody
    // else's and the library's stays dead.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit lib library roots=src entries=src/lib.kmock\n\
         unit suite test roots=tests entries=tests/t.kmock needs=lib\n",
    )
    .file("src/lib.kmock", lib)
    .file("tests/t.kmock", suite);
    let snap = run(&p);
    assert_eq!(
        reported(&snap, &Category::UNUSED),
        ["src/lib.kmock — helper"]
    );
}

#[test]
fn a_published_units_exports_are_the_outside_worlds() {
    let entry = "import ./util { other }\ncall other\n";
    let util = "pub fn other\npub fn spare\n";
    // A library publishes every export in this ecosystem: `spare` is kept by
    // the consumers no call site can show, and the keeper says so.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src entries=src/lib.kmock\n",
    )
    .file("src/lib.kmock", entry)
    .file("src/util.kmock", util);
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::new())]);
    assert!(
        reported(&snap, &Category::UNUSED).is_empty(),
        "{:?}",
        reported(&snap, &Category::UNUSED)
    );
    assert_eq!(
        common::keeper_kinds(&snap, "src/util.kmock#spare"),
        ["published"]
    );
    // The manifest keeps the library private: nobody outside consumes it.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src entries=src/lib.kmock publish=no\n",
    )
    .file("src/lib.kmock", entry)
    .file("src/util.kmock", util);
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::new())]);
    assert_eq!(
        reported(&snap, &Category::UNUSED),
        ["src/util.kmock — spare"]
    );
    // An executable hands out no API, published or not.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit app executable roots=src entries=src/lib.kmock publish=by-name\n",
    )
    .file("src/lib.kmock", entry)
    .file("src/util.kmock", util);
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::new())]);
    assert_eq!(
        reported(&snap, &Category::UNUSED),
        ["src/util.kmock — spare"]
    );
    // And where the manifest says its consumers address an ENTRY, a library's
    // non-entry export is nobody's outside either.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src entries=src/lib.kmock publish=by-entry\n",
    )
    .file("src/lib.kmock", entry)
    .file("src/util.kmock", util);
    let snap = run(&p);
    assert_eq!(
        reported(&snap, &Category::UNUSED),
        ["src/util.kmock — spare"]
    );
}

#[test]
fn an_export_of_an_unpublished_unit_may_narrow() {
    use kndo_contract::plugin::{Rung, Step};
    let laddered = || {
        MockPlugin::with(|spec| {
            spec.ladder(&[
                Step::for_free(Rung::File, "local"),
                Step::new(Rung::Exported, "pub"),
            ])
        })
    };
    let entry = "import ./util { other }\ncall other\n";
    let util = "pub fn other\npub fn helper\ncall helper\n";
    let advice = |snap: &kndo::Snapshot| -> Vec<String> {
        snap.findings
            .iter()
            .filter(|f| f.category == Category::INTERNAL_ONLY)
            .map(|f| format!("{} — {}", f.subject.label(), f.message))
            .collect()
    };
    // An executable's export is its own: used only in its file, it may narrow.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit app executable roots=src entries=src/main.kmock\n",
    )
    .file("src/main.kmock", entry)
    .file("src/util.kmock", util);
    let snap = common::analyze(&p, vec![Box::new(laddered())]);
    assert_eq!(
        advice(&snap),
        [
            "helper — declared `pub`, but every use is within its own file and nothing else in \
             the tree imports or names it — `local` would suffice for this function"
        ]
    );
    // A published library's export is the outside world's, however it is used.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src entries=src/main.kmock\n",
    )
    .file("src/main.kmock", entry)
    .file("src/util.kmock", util);
    let snap = common::analyze(&p, vec![Box::new(laddered())]);
    assert!(advice(&snap).is_empty(), "{:?}", advice(&snap));
}

#[test]
fn what_a_published_unit_hands_out_is_the_ecosystems_rule() {
    use kndo_contract::plugin::{Rung, Step};
    // ONE language, ONE project shape, ONE published library. What differs is
    // the MANIFEST: a jar hands out every `pub` class in every file it holds,
    // and an npm package hands out what its entries reach and nothing else, so
    // the same export is the outside world's under one manifest and internal
    // under the other.
    //
    // It is a fact about the unit and never about the language, which is why
    // it cannot be a capability: rust and python declare entries and hand out
    // everything, swift declares none and hands out everything, npm declares
    // them and hands out only those. One adapter reading two manifests must be
    // able to answer differently for each, and a per-language flag cannot.
    let laddered = || {
        MockPlugin::with(|spec| {
            spec.ladder(&[
                Step::for_free(Rung::File, "local"),
                Step::new(Rung::Exported, "pub"),
            ])
        })
    };
    let project = |publish: &str| {
        let p = TempProject::new();
        p.file(
            "kmock.pkg",
            &format!("unit core library roots=src entries=src/main.kmock {publish}\n"),
        )
        .file("src/main.kmock", "import ./util { other }\ncall other\n")
        .file(
            "src/util.kmock",
            "pub fn other\npub fn helper\ncall helper\n",
        );
        p
    };
    let advice = |snap: &kndo::Snapshot| -> Vec<String> {
        snap.findings
            .iter()
            .filter(|f| f.category == Category::INTERNAL_ONLY)
            .map(|f| f.subject.label().to_string())
            .collect()
    };
    let jar = common::analyze(&project("publish=by-name"), vec![Box::new(laddered())]);
    assert!(
        advice(&jar).is_empty(),
        "every export of a published unit is the world's: {:?}",
        advice(&jar)
    );
    let npm = common::analyze(&project("publish=by-entry"), vec![Box::new(laddered())]);
    assert_eq!(
        advice(&npm),
        ["helper"],
        "an export no entry hands out is internal however it is spelled"
    );
    // And the silence between them: an unstated manifest lands on the WIDER
    // surface, because a wider surface accuses less.
    let silent = common::analyze(&project(""), vec![Box::new(laddered())]);
    assert!(advice(&silent).is_empty(), "{:?}", advice(&silent));
}

#[test]
fn a_units_kind_gives_its_files_their_role() {
    // A test set's files are what the runner discovers and a tooling set's
    // are built to build something else: each is rooted by its unit's kind,
    // entry or not, and hands out its exports to its runner.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src entries=src/lib.kmock\n\
         unit suite test roots=tests entries=tests/api.kmock\n\
         unit tools tooling roots=tools\n",
    )
    .file("src/lib.kmock", "pub fn api\n")
    .file("tests/api.kmock", "import ./../src/lib { api }\ncall api\n")
    .file("tests/helper.kmock", "pub fn fixture\n")
    .file("tools/gen.kmock", "pub fn generate\n");
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
    assert_eq!(roots("tests/helper.kmock"), ["Test/Certain"]);
    assert_eq!(roots("tools/gen.kmock"), ["Tooling/Certain"]);
    assert!(
        roots("src/lib.kmock") == ["Production/Certain"],
        "a library's files are reached through its entries and its surface: {:?}",
        roots("src/lib.kmock")
    );
    assert!(
        reported(&snap, &Category::UNUSED).is_empty(),
        "{:?}",
        reported(&snap, &Category::UNUSED)
    );
    assert_eq!(
        common::keeper_kinds(&snap, "tests/helper.kmock#fixture"),
        ["entry-surface"],
        "a test set's exports are its runner's"
    );
}

#[test]
fn a_non_recursive_root_takes_one_directory_and_leaves_what_nests_under_it() {
    // Two units over one tree: the outer compiles `src` alone, the inner takes
    // everything under `src/plugin`. A recursive outer root would take the
    // nested unit's deep files by the longest-prefix rule; `flat=` is the
    // manifest saying it does not.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        concat!(
            "unit shell library flat=src entries=src/lib.kmock\n",
            "unit plugin library roots=src/plugin entries=src/plugin/lib.kmock\n",
        ),
    )
    .file("src/lib.kmock", "pub fn shell\n")
    .file("src/plugin/lib.kmock", "pub fn plugin\n")
    .file("src/plugin/deep/extra.kmock", "pub fn extra\n");
    let snap = run(&p);

    assert_eq!(unit_of(&snap, "src/lib.kmock"), Some("shell"));
    assert_eq!(
        unit_of(&snap, "src/plugin/lib.kmock"),
        Some("plugin"),
        "the inner unit's own root is longer, so it wins either way"
    );
    assert_eq!(
        unit_of(&snap, "src/plugin/deep/extra.kmock"),
        Some("plugin"),
        "nested under `src` too, but `flat=src` compiles that one directory"
    );
}

#[test]
fn a_path_this_project_excludes_stays_discovered_and_is_never_claimed() {
    // A manifest's own ignore, not the language's: `generated` is a directory
    // THIS project excludes, so nothing under it is claimed or judged — and
    // the manifest naming it is still read, so its unit still stands.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        concat!(
            "unit core library roots=src entries=src/lib.kmock\n",
            "ignore generated/**\n",
        ),
    )
    .file("src/lib.kmock", "pub fn door\n")
    .file("src/orphan.kmock", "fn floats\n")
    .file("generated/machine.kmock", "fn nobody_wrote_this\n");
    let snap = run(&p);

    assert_eq!(
        reported(&snap, &Category::UNUSED),
        ["src/orphan.kmock"],
        "the excluded file is not judged; the file beside it still is"
    );
    assert!(
        snap.graph
            .files
            .iter()
            .all(|f| f.path.as_str() != "generated/machine.kmock"),
        "unclaimed, so it never enters the graph"
    );
}
