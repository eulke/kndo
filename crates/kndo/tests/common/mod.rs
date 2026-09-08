//! The two lines every engine test writes: analyze a throwaway project under
//! the extensions it names, and read one category's subjects back.

use kndo::query::{Answer, Outcome, Request, Verb};
use kndo::{CacheLocation, Category, Config, Plugin, RunMode, Session, Threads};
use kndo_testkit::TempProject;

// Rust compiles a shared test module once per test BINARY, so a helper another
// binary uses reads as dead here — the harness's wart, per item rather than a
// file-level blanket, which kndo would (rightly) report as one.
#[allow(dead_code)]
pub fn analyze(project: &TempProject, extensions: Vec<Box<dyn Plugin>>) -> kndo::Snapshot {
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

// Rust compiles a shared test module once per test BINARY, so a helper another
// binary uses reads as dead here — the harness's wart, per item rather than a
// file-level blanket, which kndo would (rightly) report as one.
#[allow(dead_code)]
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

// Rust compiles a shared test module once per test BINARY, so a helper another
// binary uses reads as dead here — the harness's wart, per item rather than a
// file-level blanket, which kndo would (rightly) report as one.
#[allow(dead_code)]
/// The colour `describe` gives a node: which root sets reach it.
pub fn color(snap: &kndo::Snapshot, selector: &str) -> String {
    let response = snap.query(&Request {
        verb: Verb::Describe,
        inputs: vec![selector.to_string()],
        options: Default::default(),
    });
    match &response.results[0] {
        Outcome::Ok {
            answer: Answer::Describe(d),
        } => d.node.color.as_str().to_string(),
        _ => panic!("{selector}: not a node this graph holds"),
    }
}

// Rust compiles a shared test module once per test BINARY, so a helper another
// binary uses reads as dead here — the harness's wart, per item rather than a
// file-level blanket, which kndo would (rightly) report as one.
#[allow(dead_code)]
/// What `describe` says a declaration reaches: as declared, and effectively.
pub fn reaches(snap: &kndo::Snapshot, selector: &str) -> (String, String) {
    let response = snap.query(&Request {
        verb: Verb::Describe,
        inputs: vec![selector.to_string()],
        options: Default::default(),
    });
    match &response.results[0] {
        Outcome::Ok {
            answer: Answer::Describe(d),
        } => {
            let facts = d.declaration.as_ref().expect("a declaration");
            (facts.reach.clone(), facts.effective_reach.clone())
        }
        _ => panic!("describe {selector}: not an answer"),
    }
}

// Rust compiles a shared test module once per test BINARY, so a helper another
// binary uses reads as dead here — the harness's wart, per item rather than a
// file-level blanket, which kndo would (rightly) report as one.
#[allow(dead_code)]
/// What `used-by` says keeps a node alive, by kind — the evidence a judgment
/// counted, read back through the query contract every frontend uses.
pub fn keeper_kinds(snap: &kndo::Snapshot, selector: &str) -> Vec<String> {
    let response = snap.query(&Request {
        verb: Verb::UsedBy,
        inputs: vec![selector.to_string()],
        options: Default::default(),
    });
    match &response.results[0] {
        Outcome::Ok {
            answer: Answer::UsedBy(a),
        } => a.kept_by.iter().map(|e| e.kind.to_string()).collect(),
        _ => panic!("used-by {selector}: not an answer"),
    }
}
