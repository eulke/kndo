//! Three capabilities a language declares about ITSELF, each read by one seam
//! of the engine: whose dependencies its bare specifiers name, which hidden
//! directories hold its source, and how its namespaces nest. All three are
//! data — core never names a language to honour them.

mod common;

use common::reported;
use kndo::Category;
use kndo_contract::plugin::Nesting;
use kndo_testkit::{MockPlugin, TempProject};

/// The kmock language, and a second one whose bare specifiers name kmock's
/// dependencies — css and html to js-ts, in miniature.
fn kmock_and_guest() -> Vec<Box<dyn kndo::Plugin>> {
    vec![
        Box::new(MockPlugin::with(|spec| {
            spec.dependency_identity(kndo_contract::plugin::DependencyIdentity::PackageName)
                // kmock's manifest has no sections, so an unscoped declaration
                // IS a usage claim — otherwise nothing here is judged at all.
                .dependency_scoping(kndo_contract::plugin::DependencyScoping::Unscoped)
        })),
        Box::new(MockPlugin::beside("kguest", "kguest", |spec| {
            spec.ecosystem("kmock")
        })),
    ]
}

#[test]
fn a_bare_specifier_is_judged_by_the_ecosystem_its_language_names() {
    // `styled` is declared once and imported only from a `.kguest` file. The
    // guest says out loud that its bare specifiers are kmock's, so the
    // declaration is in use; `idle` is imported by nobody and is not.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit app executable roots=. entries=main.kmock\ndep styled\ndep idle\n",
    )
    .file("main.kmock", "pub fn main\n")
    .file("page.kguest", "import styled { s }\ncall s\n");
    let snap = common::analyze(&p, kmock_and_guest());

    let deps: Vec<String> = snap
        .findings
        .iter()
        .filter(|f| f.category == Category::UNUSED)
        .filter_map(|f| match &f.subject {
            kndo::Subject::Dependency { name, .. } => Some(name.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(
        deps,
        ["idle"],
        "the guest's import is a use of kmock's declaration; the unimported one stays accused"
    );
}

#[test]
fn a_language_names_the_hidden_directories_its_source_lives_in() {
    // `.kstore` holds real source, is hidden only by convention, and no
    // manifest or launcher glob names it — so the language says so, and
    // discovery enters it.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit app executable roots=. entries=main.kmock\n",
    )
    .file(
        "main.kmock",
        "pub fn main\nimport ./.kstore/wired { w }\ncall w\n",
    )
    .file(".kstore/wired.kmock", "pub fn w\n")
    .file(".kstore/loose.kmock", "fn floats\n")
    .file(".khidden/unseen.kmock", "fn never_discovered\n");
    let snap = common::analyze(
        &p,
        vec![Box::new(MockPlugin::with(|spec| {
            spec.hidden_opt_in(&[".kstore"])
        }))],
    );

    assert_eq!(
        reported(&snap, &Category::UNUSED),
        [".kstore/loose.kmock"],
        "the opted-in directory is judged; the other dot-named one is never walked"
    );
}

#[test]
fn a_directory_nested_language_keeps_two_same_named_namespaces_apart() {
    // Two directories of one unit writing the same clause, both reached, and
    // a namespace-reaching declaration in only one of them. Whether the call
    // in `a` can name it is exactly the question the nesting answers.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit app executable roots=. entries=main.kmock\n",
    )
    .file(
        "main.kmock",
        concat!(
            "package util\n",
            "pub fn main\n",
            "import ./a/one { shared }\n",
            "import ./b/two\n",
            "call shared\n",
        ),
    )
    .file("a/one.kmock", "package util\npub fn shared\ncall helper\n")
    .file("b/two.kmock", "package util\nns fn helper\n");

    let by_directory =
        |spec: kndo_contract::plugin::PluginSpecBuilder| spec.nesting(Nesting::ByDirectory);
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::with(by_directory))]);
    assert_eq!(
        reported(&snap, &Category::UNUSED),
        ["b/two.kmock — helper"],
        "`b`'s `helper` reaches `b`'s namespace, and `a` is a different one"
    );

    // Under `Flat` the clause is the whole key: one namespace, and the call in
    // `a` names the declaration in `b`. The same tree, two answers, and which
    // is right is the LANGUAGE's to say.
    let flat = |spec: kndo_contract::plugin::PluginSpecBuilder| spec.nesting(Nesting::Flat);
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::with(flat))]);
    assert!(
        reported(&snap, &Category::UNUSED).is_empty(),
        "one namespace: the call reaches across the directories, {:?}",
        reported(&snap, &Category::UNUSED)
    );
}

#[test]
fn a_specifier_a_manifest_rewrites_resolves_against_what_it_names() {
    // `@app/` is a prefix the manifest rewrites to `src/app`, and the nested
    // manifest rewrites it somewhere else for the files it covers. Both are
    // the PROJECT's answer, asked through `cx.project().alias(from, …)` — the
    // language keeps no table of its own.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        concat!(
            "unit app executable roots=. entries=main.kmock\n",
            "alias @app/ src/app\n",
        ),
    )
    .file("inner/kmock.pkg", "alias @app/ inner/own\n")
    .file(
        "main.kmock",
        concat!(
            "pub fn main\n",
            "import @app/lib { shared }\n",
            "call shared\n",
            "import ./inner/caller\n",
        ),
    )
    .file("src/app/lib.kmock", "pub fn shared\n")
    .file(
        "inner/caller.kmock",
        "import @app/lib { local }\ncall local\n",
    )
    .file("inner/own/lib.kmock", "pub fn local\n")
    .file("src/app/idle.kmock", "pub fn nobody\n");
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::new())]);

    assert_eq!(
        reported(&snap, &Category::UNUSED),
        ["src/app/idle.kmock"],
        "both rewrites resolved: only the file no alias reaches is accused"
    );
}
