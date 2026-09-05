//! The two lines every engine test writes: analyze a throwaway project under
//! the extensions it names, and read one category's subjects back.

// kndo:allow-file test-only -- a module of a test target is a test by role, not
// production code only tests reach; M8.b's file roles retire this allow, and the
// flicker rule turns it stale the day they land.

use kndo::{CacheLocation, Category, Config, Extension, RunMode, Session, Threads};
use kndo_testkit::TempProject;

pub fn analyze(project: &TempProject, extensions: Vec<Box<dyn Extension>>) -> kndo::Snapshot {
    let session = Session::open(
        project.root(),
        Config {
            threads: Threads::Auto,
            cache: CacheLocation::Off,
            ..Config::default()
        },
        extensions,
    )
    .expect("open");
    session.analyze(RunMode::Full).expect("analyze")
}

/// The rendered subjects reported under `category`, sorted.
pub fn reported(snap: &kndo::Snapshot, category: &Category) -> Vec<String> {
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
