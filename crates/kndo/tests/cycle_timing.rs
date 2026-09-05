//! The `cyclic` analysis judges load-time edges only: an import that runs when
//! the code around it does, or never (types), cannot take part in an
//! initialization hazard — while reachability keeps every timing.

use kndo::{CacheLocation, Category, Config, RunMode, Session, Threads};
use kndo_testkit::{MockExtension, TempProject};

fn run(project: &TempProject) -> kndo::Snapshot {
    let session = Session::open(
        project.root(),
        Config {
            threads: Threads::Auto,
            cache: CacheLocation::Off,
            ..Config::default()
        },
        vec![Box::new(MockExtension::hazardous())],
    )
    .expect("open");
    session.analyze(RunMode::Full).expect("analyze")
}

fn categories(snap: &kndo::Snapshot, category: &Category) -> Vec<String> {
    let mut out: Vec<String> = snap
        .report()
        .findings
        .iter()
        .filter(|f| &f.category == category)
        .map(|f| f.subject.render())
        .collect();
    out.sort();
    out
}

#[test]
fn only_load_time_imports_close_a_cycle() {
    // a ↔ b at load time: the hazard is real.
    let loaded = TempProject::new();
    loaded
        .file("a.kmock", "root-file\nimport ./b\npub fn a\n")
        .file("b.kmock", "import ./a\npub fn b\n");
    let snap = run(&loaded);
    assert_eq!(categories(&snap, &Category::CYCLIC), ["a.kmock"]);

    // The same loop closed by an erased edge: nothing initializes through it.
    for back in ["erased-import ./a", "lazy-import ./a"] {
        let p = TempProject::new();
        p.file("a.kmock", "root-file\nimport ./b\npub fn a\n")
            .file("b.kmock", &format!("{back}\npub fn b\n"));
        let snap = run(&p);
        assert!(
            categories(&snap, &Category::CYCLIC).is_empty(),
            "{back}: no initialization hazard"
        );
        // Reachability still crosses the edge in both directions: nothing is unused.
        assert!(
            categories(&snap, &Category::UNUSED).is_empty(),
            "{back}: b still reaches a"
        );
    }
}
