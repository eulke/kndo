//! Markers mean what the claiming extension's dispatch rules say: a marker the
//! rules root becomes an entry of that color, a marker they exempt keeps the
//! declaration out of the unused judgment, a marker naming the file a
//! generator's output takes what it DECLARES out of every judgment while
//! leaving the file itself judged, and a file-level exemption is said aloud in
//! the report — the same engine path every language rides.

mod common;

use common::{keeper_kinds, reported};
use kndo::Category;
use kndo_contract::evidence::{RelationKind, RootKind, SymbolKind};
use kndo_contract::manifest::UnitKind;
use kndo_contract::plugin::{DispatchRule, Effect, Trigger};
use kndo_contract::vocab::Confidence;
use kndo_testkit::{MockPlugin, TempProject};

fn rules() -> Vec<DispatchRule> {
    let certain = |when: Trigger, then: Effect| DispatchRule {
        when,
        then,
        confidence: Confidence::Certain,
    };
    vec![
        certain(Trigger::marker("test"), Effect::Root(RootKind::Test)),
        certain(Trigger::marker_with("allow", "dead_code"), Effect::Exempt),
        certain(Trigger::marker("generated"), Effect::Generated),
    ]
}

fn run(project: &TempProject) -> kndo::Snapshot {
    common::analyze(project, vec![Box::new(MockPlugin::dispatching(rules()))])
}

#[test]
fn a_rooting_marker_is_an_entry_of_its_color() {
    let p = TempProject::new();
    // `check` carries the test marker and reaches `lib`; nothing production
    // does, so the file is test-only territory and nothing in it is unused.
    p.file(
        "check.kmock",
        "fn check\nmark check test\nimport ./lib { helper }\n",
    )
    .file("lib.kmock", "pub fn helper\n");
    let snap = run(&p);
    assert!(reported(&snap, &Category::UNUSED).is_empty());
    assert_eq!(reported(&snap, &Category::TEST_ONLY), ["lib.kmock"]);
    assert_eq!(keeper_kinds(&snap, "check.kmock#check"), ["rule:kmock#0"]);

    // The same marker with no rule for it derives nothing: the file has no
    // root, so the run abstains from judging it at all.
    let plain = common::analyze(&p, vec![Box::new(MockPlugin::new())]);
    assert!(plain.report().findings.is_empty());
    assert!(
        plain
            .report()
            .abstained
            .iter()
            .any(|a| a.category == Category::UNUSED),
        "{:?}",
        plain.report().abstained
    );
}

#[test]
fn an_exempting_marker_stands_the_unused_judgment_down() {
    let p = TempProject::new();
    p.file(
        "main.kmock",
        "root-file\nfn kept\nmark kept allow dead_code\nfn stale\nfn noted\nmark noted allow unused_imports\n",
    );
    let snap = run(&p);
    // Only the exemption the rules name counts: `unused_imports` is not it.
    assert_eq!(
        reported(&snap, &Category::UNUSED),
        ["main.kmock — noted", "main.kmock — stale"]
    );
    assert_eq!(keeper_kinds(&snap, "main.kmock#kept"), ["rule:kmock#1"]);
    assert!(snap.report().diagnostics.is_empty());
}

#[test]
fn a_file_level_exemption_covers_everything_and_is_said_aloud() {
    let p = TempProject::new();
    p.file(
        "scratch.kmock",
        "root-file\nmark-file allow dead_code\nfn draft_one\nfn draft_two\n",
    );
    let snap = run(&p);
    assert!(reported(&snap, &Category::UNUSED).is_empty());
    let report = snap.report();
    assert_eq!(report.diagnostics.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(report.diagnostics[0].path.as_str(), "scratch.kmock");
    assert_eq!(
        report.diagnostics[0].message,
        "`allow(dead_code)` at file level exempts every declaration here (2) from the unused judgment"
    );
}

#[test]
fn a_unit_level_exemption_reaches_the_unit_and_only_from_its_entry() {
    // A crate root's `#![allow(dead_code)]`: the same word, in two places. On
    // the file the build ENTERS the unit through it is the unit's statement
    // and covers every file the unit compiles; on a module file of that same
    // unit it is that file's own, and its sibling stays accused. Extraction
    // wrote the identical marker both times — the manifest is what tells them
    // apart, and only the engine has read it.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit app library roots=src entries=src/lib.kmock\n\
         unit side library roots=side entries=side/main.kmock\n",
    )
    // `app` carries the blanket on its ENTRY: `stale`, a file away, is covered.
    .file(
        "src/lib.kmock",
        "root-file\nmark-unit allow dead_code\nmount inner ./inner\npub fn api\n",
    )
    .file("src/inner.kmock", "fn stale\n")
    // `side` carries the identical marker on a file the build does not enter
    // it through: `draft` beside it is covered, `accused` in its unit-mate is
    // not.
    .file(
        "side/main.kmock",
        "root-file\nmount m ./m\nmount other ./other\npub fn go\n",
    )
    .file("side/m.kmock", "mark-unit allow dead_code\nfn draft\n")
    .file("side/other.kmock", "fn accused\n");
    let snap = run(&p);
    assert_eq!(
        reported(&snap, &Category::UNUSED),
        ["side/other.kmock — accused"],
        "a unit speaks through its entry, and nowhere else"
    );
    // And the run says which of the two happened, file by file: a blanket the
    // unit spoke is reported as the unit's, and the one it did not is reported
    // for what it is.
    let report = snap.report();
    let blankets: Vec<(&str, &str)> = report
        .diagnostics
        .iter()
        .filter_map(|d| {
            let level = ["unit", "file"]
                .into_iter()
                .find(|l| d.message.contains(&format!("at {l} level")))?;
            Some((d.path.as_str(), level))
        })
        .collect();
    assert_eq!(
        blankets,
        [
            ("side/m.kmock", "file"),
            ("src/inner.kmock", "unit"),
            ("src/lib.kmock", "unit"),
        ]
    );
}

#[test]
fn a_generators_output_declares_nothing_this_project_answers_for() {
    let p = TempProject::new();
    // `codegen.kmock` is a generator's output that the app imports: the names
    // in it are the generator's, so the two nobody calls are not accusations
    // against this project — while `stale`, in hand-written code, still is.
    p.file(
        "main.kmock",
        "root-file\nimport ./codegen { Wire }\nfn stale\n",
    )
    .file(
        "codegen.kmock",
        "mark-file generated\npub fn Wire\npub fn Unused\npub fn AlsoUnused\n",
    );
    let snap = run(&p);
    assert_eq!(reported(&snap, &Category::UNUSED), ["main.kmock — stale"]);
    let report = snap.report();
    assert_eq!(report.diagnostics.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(report.diagnostics[0].path.as_str(), "codegen.kmock");
    assert_eq!(
        report.diagnostics[0].message,
        "`generated` marks this file a generator's output: what it declares is not judged, \
         what it imports and names still is"
    );
}

#[test]
fn a_generators_output_nobody_imports_is_still_dead_weight() {
    let p = TempProject::new();
    // Being generated is not a root: an orphan `.pb` nobody ever imports is
    // as removable as an orphan anyone wrote, and the FILE is the finding —
    // no declaration inside it is ever named.
    p.file("main.kmock", "root-file\n").file(
        "orphan.kmock",
        "mark-file generated\npub fn Wire\npub fn Unused\n",
    );
    let snap = run(&p);
    assert_eq!(reported(&snap, &Category::UNUSED), ["orphan.kmock"]);
}

#[test]
fn a_name_rule_reads_the_kind_of_compilation_the_file_lands_in() {
    let certain = |when: Trigger, then: Effect| DispatchRule {
        when,
        then,
        confidence: Confidence::Certain,
    };
    // A runner's convention and a runtime's: `Check*` is run by name in a test
    // compilation, and `boot` runs in whichever binary links the file — so its
    // color is that compilation's, which takes two rules and not a guess.
    let rules = vec![
        certain(
            Trigger::name("Check*", SymbolKind::Function, UnitKind::Test),
            Effect::Root(RootKind::Test),
        ),
        certain(
            Trigger::name("boot", SymbolKind::Function, UnitKind::Test),
            Effect::Root(RootKind::Test),
        ),
        certain(
            Trigger::name("boot", SymbolKind::Function, UnitKind::Library),
            Effect::Root(RootKind::Production),
        ),
    ];
    let p = TempProject::new();
    // Two ways a file lands in a test compilation, and the rule reads both the
    // same: `suite` is a unit the project declares of that kind, and
    // `src/inline.kmock` says so itself — the language whose tests live beside
    // what they test, with no separate unit to name them.
    p.file(
        "kmock.pkg",
        "unit lib library roots=src entries=src/app.kmock\nunit suite test roots=tests entries=tests/check.kmock\n",
    )
    .file(
        "tests/check.kmock",
        "fn CheckOne\nfn boot\nfn helper\ntype Fixture\nmember Fixture.CheckTwo\n",
    )
    .file("src/inline.kmock", "test-only\nfn CheckThree\n")
    .file("src/app.kmock", "fn boot\nfn stale\ntype CheckKind\n");
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::dispatching(rules))]);

    // The rule fires where the compilation says it should, and its color is
    // the one that compilation implies.
    assert_eq!(
        keeper_kinds(&snap, "tests/check.kmock#CheckOne"),
        ["rule:kmock#0"]
    );
    assert_eq!(
        keeper_kinds(&snap, "tests/check.kmock#boot"),
        ["rule:kmock#1"]
    );
    assert_eq!(
        keeper_kinds(&snap, "src/inline.kmock#CheckThree"),
        ["rule:kmock#0"]
    );
    // The SAME name, a different rule: `boot` in the library compilation is
    // rule 2 and in the test one rule 1 — which the keeper now says out loud,
    // so deleting either rule fails this assertion by name.
    assert_eq!(keeper_kinds(&snap, "src/app.kmock#boot"), ["rule:kmock#2"]);

    // And nowhere else. `CheckKind` is a type, not the function the rule
    // names; `Fixture.CheckTwo` is a MEMBER, dispatched by its owner and not
    // by a name rule; `helper` and `stale` match nothing.
    assert_eq!(
        reported(&snap, &Category::UNUSED),
        [
            "src/app.kmock — CheckKind",
            "src/app.kmock — stale",
            "tests/check.kmock — Fixture",
            "tests/check.kmock — Fixture.CheckTwo",
            "tests/check.kmock — helper",
        ]
    );
}

#[test]
fn a_witness_is_kept_by_the_surface_its_owner_promised_and_by_no_color() {
    let certain = |when: Trigger, then: Effect| DispatchRule {
        when,
        then,
        confidence: Confidence::Certain,
    };
    let rules = vec![
        // What a base OUTSIDE the project requires — the graph can never
        // resolve `Closer`, so the rule names it and its one requirement.
        certain(Trigger::required_by("Closer", &["shut"]), Effect::Witness),
        // And the source's own statement that a supertype declares this,
        // wherever that supertype lives.
        certain(Trigger::marker("Override"), Effect::Witness),
    ];
    let p = TempProject::new();
    p.file(
        "app.kmock",
        "root-file\n\
         type Handle\n\
         implements Handle Closer\n\
         pub member Handle.shut\n\
         pub member Handle.drop\n\
         type Plain\n\
         pub member Plain.shut\n\
         type Sub\n\
         pub member Sub.render\n\
         mark render Override\n",
    );
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::dispatching(rules))]);

    // The base names it: kept, and the keeper SAYS which base.
    assert_eq!(
        keeper_kinds(&snap, "app.kmock#Handle.shut"),
        ["witness:Closer:rule:kmock#0"]
    );
    assert_eq!(
        keeper_kinds(&snap, "app.kmock#Sub.render"),
        ["witness:Override:rule:kmock#1"]
    );
    // A witness is not a root: no color rides on it, so the file's own
    // production root is the only thing coloring anything here.
    assert_eq!(common::color(&snap, "app.kmock#Handle.shut"), "production");

    // The same NAME on a type that promised nothing is judged like any other
    // member, and so is a member the base does not require. The owners are
    // accused too, and that is the law working: a witness is alive WHILE ITS
    // OWNER IS — it never argues the owner's case.
    assert_eq!(
        reported(&snap, &Category::UNUSED),
        [
            "app.kmock — Handle",
            "app.kmock — Handle.drop",
            "app.kmock — Plain",
            "app.kmock — Plain.shut",
            "app.kmock — Sub",
        ]
    );
}

#[test]
fn a_rule_can_name_a_base_and_its_requirement_separately() {
    let certain = |when: Trigger, then: Effect| DispatchRule {
        when,
        then,
        confidence: Confidence::Certain,
    };
    // The general form: a member of a type that IMPLEMENTS something, by name
    // pattern. `extends` is a different promise and this rule does not read it.
    let rules = vec![
        certain(
            Trigger::member_of(
                Trigger::relation(RelationKind::Implements, "Closer"),
                "shut*",
            ),
            Effect::Witness,
        ),
        // And a type-shaped rule, on the owner itself.
        certain(
            Trigger::relation(RelationKind::Extends, "Runner"),
            Effect::Root(RootKind::Test),
        ),
    ];
    let p = TempProject::new();
    p.file(
        "app.kmock",
        "root-file\n\
         type Handle\n\
         implements Handle Closer\n\
         pub member Handle.shutdown\n\
         pub member Handle.open\n\
         type Heir\n\
         extends Heir Runner\n\
         type Inherits\n\
         extends Inherits Closer\n\
         pub member Inherits.shutdown\n",
    );
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::dispatching(rules))]);

    assert_eq!(
        keeper_kinds(&snap, "app.kmock#Handle.shutdown"),
        ["witness:Closer:rule:kmock#0"]
    );
    assert_eq!(keeper_kinds(&snap, "app.kmock#Heir"), ["rule:kmock#1"]);
    // `open` is not the name; `Inherits` promised Closer with the OTHER kind
    // of relation, and a rule that named one kind does not read the other.
    assert_eq!(
        reported(&snap, &Category::UNUSED),
        [
            "app.kmock — Handle",
            "app.kmock — Handle.open",
            "app.kmock — Inherits",
            "app.kmock — Inherits.shutdown",
        ]
    );
}

#[test]
fn a_marker_rule_can_name_the_kind_it_means() {
    let rules = vec![DispatchRule {
        when: Trigger::marker_on("Entry", SymbolKind::Function),
        then: Effect::Root(RootKind::Production),
        confidence: Confidence::Certain,
    }];
    let p = TempProject::new();
    p.file(
        "app.kmock",
        "root-file\nfn run\nmark run Entry\ntype Holder\nmark Holder Entry\n",
    );
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::dispatching(rules))]);
    assert_eq!(keeper_kinds(&snap, "app.kmock#run"), ["rule:kmock#0"]);
    // Same marker, wrong kind: the rule said what it meant.
    assert_eq!(reported(&snap, &Category::UNUSED), ["app.kmock — Holder"]);
}
