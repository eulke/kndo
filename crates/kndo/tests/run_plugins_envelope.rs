//! `run.plugins[]` — the envelope's answer to "which plugins ran, and why".
//!
//! The reason is *decided* by this crate's composition layer (activation rules, the
//! `dependencies` fixpoint, the three tiers) and *reported* by the engine. This test guards the
//! join between the two halves, so a refactor cannot quietly drop the reason on the floor — or,
//! worse, have core re-derive one of its own that disagrees with `doctor`.

use std::path::Path;

fn envelope(root: &Path) -> serde_json::Value {
    let overrides = kndo_core::engine::ConfigOverrides {
        use_cache: false,
        threads: Some(1),
        ..kndo_core::engine::ConfigOverrides::default()
    };
    let mut engine = kndo::open(root, overrides).expect("kndo::open");
    let json = engine.check(kndo_core::engine::RunMode::Full).to_json();
    serde_json::from_str(&json).expect("the envelope is JSON")
}

fn activated_by<'a>(envelope: &'a serde_json::Value, id: &str) -> Option<&'a str> {
    envelope["run"]["plugins"]
        .as_array()
        .expect("run.plugins is an array")
        .iter()
        .find(|p| p["id"] == id)
        .map(|p| {
            p["activated_by"]
                .as_str()
                .expect("activated_by is a string")
        })
}

#[test]
fn the_envelope_names_every_active_plugin_and_the_reason_it_activated() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies": {"next": "14.0.0"}}"#,
    )
    .unwrap();
    std::fs::write(root.join("index.ts"), "export const x = 1;\n").unwrap();

    let envelope = envelope(root);

    // A rule-gated built-in reports the rule that fired, not the bare fact that one did:
    // "why is this running" is unanswerable from "rule matched".
    assert_eq!(
        activated_by(&envelope, "kndo:nextjs"),
        Some("manifest-dependency: next"),
        "{:?}",
        envelope["run"]["plugins"]
    );
    // An always-on built-in (the coverage ingesters declare no rules) says so.
    assert_eq!(
        activated_by(&envelope, "kndo:coverage-lcov"),
        Some("always-on")
    );
    // A plugin whose gate did NOT fire is absent entirely — `run.plugins[]` is the *active*
    // set, so the field answers "why", never "whether".
    assert_eq!(activated_by(&envelope, "kndo:express"), None);

    // The envelope and `kndo doctor` are two views of one composition, and must agree
    // exactly — ids and reasons both.
    let mut from_doctor: Vec<(String, String)> = kndo::plugin_resolution(root)
        .plugins
        .into_iter()
        .filter_map(|p| p.active.map(|reason| (p.id, reason.to_string())))
        .collect();
    let mut from_envelope: Vec<(String, String)> = envelope["run"]["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["id"].as_str().unwrap().to_string(),
                p["activated_by"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    from_doctor.sort();
    from_envelope.sort();
    assert_eq!(from_envelope, from_doctor);
}
