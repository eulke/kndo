//! Two checks:
//! 1. Real `--format json` output from a real `Engine` run validates against the committed
//!    schema (`schemas/kndo-output.schema.json`).
//! 2. That committed file matches what `cargo xtask gen-schema` writes from the same
//!    `kndo_core::engine::Envelope` type.
//!
//! The same two checks repeat for the navigation-query envelope against
//! `schemas/kndo-query-output.schema.json`.

use std::fs;
use std::path::PathBuf;

use kndo_core::engine::{ConfigOverrides, Engine, RunMode};
use kndo_core::query_envelope::{QueryFlags, QueryRequest, Verb};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/kndo has a workspace root two levels up")
        .to_path_buf()
}

fn schema_value() -> serde_json::Value {
    let path = workspace_root().join("schemas/kndo-output.schema.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "reading {}: {e} — run `cargo xtask gen-schema`",
            path.display()
        )
    });
    serde_json::from_str(&text).expect("committed schema is valid JSON")
}

#[test]
fn committed_schema_matches_the_type_it_was_generated_from() {
    let fresh = kndo_core::engine::json_schema();
    let fresh_text = serde_json::to_string_pretty(&fresh).unwrap();
    let committed_path = workspace_root().join("schemas/kndo-output.schema.json");
    let committed_text = fs::read_to_string(&committed_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", committed_path.display()));
    assert_eq!(
        format!("{fresh_text}\n"),
        committed_text,
        "schemas/kndo-output.schema.json is stale — regenerate with `cargo xtask gen-schema`"
    );
}

#[test]
fn real_json_output_validates_against_the_committed_schema() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "schema-validation-fixture", "private": false, "main": "src/index.ts"}"#,
    )
    .unwrap();
    fs::write(dir.path().join("src/index.ts"), "console.log(\"alive\");\n").unwrap();
    // Not `main`'s own exports (those are the package's public API, a production root in
    // their own right) — an orphan file nothing imports, genuinely dead.
    fs::write(
        dir.path().join("src/orphan.ts"),
        "export function dead(): void {}\n",
    )
    .unwrap();

    let mut engine = Engine::open(
        dir.path(),
        ConfigOverrides::default(),
        kndo::default_adapters(),
    )
    .expect("engine opens on a real temp project");
    let result = engine.check(RunMode::Full);
    // A non-empty findings array exercises more of the schema than a clean run would.
    assert!(!result.findings.is_empty());

    let instance: serde_json::Value = serde_json::from_str(&result.to_json()).unwrap();
    let schema = schema_value();
    let validator = jsonschema::validator_for(&schema).expect("committed schema itself compiles");
    let errors: Vec<String> = validator
        .iter_errors(&instance)
        .map(|e| e.to_string())
        .collect();
    assert!(
        errors.is_empty(),
        "real JSON output does not validate against schemas/kndo-output.schema.json:\n{}",
        errors.join("\n")
    );
}

#[test]
fn sanity_the_validator_actually_rejects_bad_instances() {
    let schema = schema_value();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let bad = serde_json::json!({ "not": "a valid envelope" });
    assert!(!validator.is_valid(&bad));
}

fn query_schema_value() -> serde_json::Value {
    let path = workspace_root().join("schemas/kndo-query-output.schema.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "reading {}: {e} — run `cargo xtask gen-schema`",
            path.display()
        )
    });
    serde_json::from_str(&text).expect("committed query schema is valid JSON")
}

#[test]
fn committed_query_schema_matches_the_type_it_was_generated_from() {
    let fresh = kndo_core::query_envelope::json_schema();
    let fresh_text = serde_json::to_string_pretty(&fresh).unwrap();
    let committed_path = workspace_root().join("schemas/kndo-query-output.schema.json");
    let committed_text = fs::read_to_string(&committed_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", committed_path.display()));
    assert_eq!(
        format!("{fresh_text}\n"),
        committed_text,
        "schemas/kndo-query-output.schema.json is stale — regenerate with `cargo xtask gen-schema`"
    );
}

#[test]
fn real_query_output_validates_against_the_committed_schema() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name": "query-schema-validation-fixture", "private": false, "main": "src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("src/index.ts"),
        "export function alive(): number { return helper(); }\nfunction helper(): number { return 1; }\n",
    )
    .unwrap();

    let engine = Engine::open(
        dir.path(),
        ConfigOverrides::default(),
        kndo::default_adapters(),
    )
    .expect("engine opens on a real temp project");

    let validator = jsonschema::validator_for(&query_schema_value())
        .expect("committed query schema itself compiles");

    // Exercise more than one verb's result shape in one pass — find (matches), describe
    // (declaration/degree/roots/findings), and a not-found selector (the inline `Failed` shape),
    // all in the same batch so `results[]`'s untagged variants are all covered at once.
    let results = engine.query_batch(vec![
        QueryRequest {
            id: Some("q1".to_string()),
            verb: Verb::Find,
            selectors: vec!["helper".to_string()],
            flags: QueryFlags::default(),
        },
        QueryRequest {
            id: Some("q2".to_string()),
            verb: Verb::Describe,
            selectors: vec!["src/index.ts#alive".to_string(), "nope.ts".to_string()],
            flags: QueryFlags::default(),
        },
        QueryRequest {
            id: Some("q3".to_string()),
            verb: Verb::UsedBy,
            selectors: vec!["src/index.ts#helper".to_string()],
            flags: QueryFlags::default(),
        },
        QueryRequest {
            id: Some("q4".to_string()),
            verb: Verb::Trace,
            selectors: vec!["src/index.ts#helper".to_string()],
            flags: QueryFlags::default(),
        },
    ]);
    assert_eq!(results.len(), 4);

    for result in &results {
        let instance: serde_json::Value = serde_json::from_str(&result.to_json()).unwrap();
        let errors: Vec<String> = validator
            .iter_errors(&instance)
            .map(|e| e.to_string())
            .collect();
        assert!(
            errors.is_empty(),
            "query verb `{}` output does not validate against schemas/kndo-query-output.schema.json:\n{}",
            result.verb.as_str(),
            errors.join("\n")
        );
    }
}
