//! The reader graded against Python's OWN tools, not against our reading of a
//! PEP. `tests/captured/tooling.json` holds what `packaging` and `setuptools`
//! answered on this machine, with their versions; the checks below replay
//! those answers through the adapter. Recapture with
//! `scratchpad/capture-py.py` when the pinned versions move.

use kndo_adapter_python::manifest::{canonicalize, dependency_name};
use serde_json::Value;

fn captured() -> Value {
    let text = include_str!("captured/tooling.json");
    serde_json::from_str(text).expect("the capture is valid json")
}

#[test]
fn pep_508_names_match_packagings_own_parse() {
    let doc = captured();
    let mut wrong = Vec::new();
    for row in doc["pep508"].as_array().expect("pep508 rows") {
        let spec = row["spec"].as_str().expect("spec");
        let want = row["name"].as_str();
        let got = dependency_name(spec);
        if got != want {
            wrong.push(format!("{spec:?}: packaging says {want:?}, we say {got:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "graded against packaging {}:\n{}",
        doc["producers"]["packaging"],
        wrong.join("\n")
    );
}

#[test]
fn pep_503_canonicalization_matches_packagings() {
    let doc = captured();
    let mut wrong = Vec::new();
    for row in doc["pep503"].as_array().expect("pep503 rows") {
        let name = row["name"].as_str().expect("name");
        let want = row["canonical"].as_str().expect("canonical");
        let got = canonicalize(name);
        if got != want {
            wrong.push(format!("{name:?}: packaging says {want:?}, we say {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
