//! The `cyclic` analysis judges load-time edges only: an import that runs when
//! the code around it does, or never (types), cannot take part in an
//! initialization hazard — while reachability keeps every timing.

mod common;

use common::reported as categories;
use kndo::Category;
use kndo_testkit::{MockPlugin, TempProject};

fn run(project: &TempProject) -> kndo::Snapshot {
    common::analyze(project, vec![Box::new(MockPlugin::hazardous())])
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
