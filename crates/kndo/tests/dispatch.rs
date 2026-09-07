//! Markers mean what the claiming extension's dispatch rules say: a marker the
//! rules root becomes an entry of that color, a marker they exempt keeps the
//! declaration out of the unused judgment, a marker naming the file a
//! generator's output takes what it DECLARES out of every judgment while
//! leaving the file itself judged, and a file-level exemption is said aloud in
//! the report — the same engine path every language rides.

mod common;

use common::{keeper_kinds, reported};
use kndo::Category;
use kndo_contract::evidence::{RootKind, SymbolKind};
use kndo_contract::extension::{DispatchRule, Effect, InFiles, Trigger};
use kndo_contract::vocab::Confidence;
use kndo_testkit::{MockExtension, TempProject};

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
    common::analyze(project, vec![Box::new(MockExtension::dispatching(rules()))])
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
    assert_eq!(keeper_kinds(&snap, "check.kmock#check"), ["dispatch:test"]);

    // The same marker with no rule for it derives nothing: the file has no
    // root, so the run abstains from judging it at all.
    let plain = common::analyze(&p, vec![Box::new(MockExtension::new())]);
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
    assert_eq!(keeper_kinds(&snap, "main.kmock#kept"), ["exempt"]);
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
fn a_name_rule_reads_the_role_the_project_gave_the_file() {
    let certain = |when: Trigger, then: Effect| DispatchRule {
        when,
        then,
        confidence: Confidence::Certain,
    };
    // A runner's convention and a runtime's: `Check*` is run by name where the
    // tests are, and `boot` runs in whichever binary links the file — so its
    // color is the file's, which takes two rules and not a guess.
    let rules = vec![
        certain(
            Trigger::name(
                "Check*",
                SymbolKind::Function,
                InFiles::Rooted(RootKind::Test),
            ),
            Effect::Root(RootKind::Test),
        ),
        certain(
            Trigger::name(
                "boot",
                SymbolKind::Function,
                InFiles::Rooted(RootKind::Test),
            ),
            Effect::Root(RootKind::Test),
        ),
        certain(
            Trigger::name(
                "boot",
                SymbolKind::Function,
                InFiles::NotRooted(RootKind::Test),
            ),
            Effect::Root(RootKind::Production),
        ),
    ];
    let p = TempProject::new();
    p.file(
        "check.kmock",
        "test-file\nfn CheckOne\nfn boot\nfn helper\ntype Fixture\nmember Fixture.CheckTwo\n",
    )
    .file("app.kmock", "root-file\nfn boot\nfn stale\ntype CheckKind\n");
    let snap = common::analyze(&p, vec![Box::new(MockExtension::dispatching(rules))]);

    // The rule fires where the file's role says it should, and its color is
    // the one that role implies.
    assert_eq!(
        keeper_kinds(&snap, "check.kmock#CheckOne"),
        ["dispatch:test"]
    );
    assert_eq!(keeper_kinds(&snap, "check.kmock#boot"), ["dispatch:test"]);
    assert_eq!(
        keeper_kinds(&snap, "app.kmock#boot"),
        ["dispatch:production"]
    );

    // And nowhere else. `CheckKind` is a type, not the function the rule
    // names; `Fixture.CheckTwo` is a MEMBER, dispatched by its owner and not
    // by a name rule; `helper` and `stale` match nothing.
    assert_eq!(
        reported(&snap, &Category::UNUSED),
        [
            "app.kmock — CheckKind",
            "app.kmock — stale",
            "check.kmock — Fixture",
            "check.kmock — Fixture.CheckTwo",
            "check.kmock — helper",
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
    let snap = common::analyze(&p, vec![Box::new(MockExtension::dispatching(rules))]);

    // The base names it: kept, and the keeper SAYS which base.
    assert_eq!(
        keeper_kinds(&snap, "app.kmock#Handle.shut"),
        ["witness:Closer"]
    );
    assert_eq!(
        keeper_kinds(&snap, "app.kmock#Sub.render"),
        ["witness:Override"]
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
