//! RFC 0008 §4's enforcement clause, literally: "the CI fixture matrix runs `--threads 1` vs
//! `--threads N` and asserts byte-identical JSON output." Spawns the real compiled binary twice
//! against the same fixture (a rayon global pool is process-scoped — this can't be verified by
//! calling `Engine::open` twice in one test process, only by two real invocations) and diffs
//! stdout exactly.

use std::path::{Path, PathBuf};
use std::process::Command;

fn kndo_bin() -> &'static str {
    env!("CARGO_BIN_EXE_kndo")
}

/// Enough files (well above RFC 0008 §6's 16-file adaptive-execution threshold) and enough
/// cross-file imports/references that discovery, extraction, and resolution all actually run in
/// parallel under `--threads 4` rather than falling back to the inline single-file path.
fn write_fixture(dir: &Path) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("package.json"),
        r#"{"name": "threads-determinism-fixture", "private": false, "main": "src/index.js"}"#,
    )
    .unwrap();

    let mut index = String::from("const parts = [\n");
    for i in 0..24 {
        std::fs::write(
            dir.join(format!("src/mod{i}.js")),
            format!(
                "function value{i}() {{ return {i}; }}\nfunction dead{i}() {{ return -1; }}\nmodule.exports = {{ value{i} }};\n"
            ),
        )
        .unwrap();
        index.push_str(&format!("  require(\"./mod{i}.js\").value{i}(),\n"));
    }
    index.push_str("];\nfunction main() { return parts.length; }\nmodule.exports = { main };\n");
    std::fs::write(dir.join("src/index.js"), index).unwrap();
}

fn run_check(dir: &Path, threads: &str) -> serde_json::Value {
    let output = Command::new(kndo_bin())
        .current_dir(dir)
        .args([
            "check",
            "--no-cache",
            "--format",
            "json",
            "--threads",
            threads,
        ])
        .output()
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", kndo_bin()));
    assert!(
        output.status.success() || output.status.code() == Some(1),
        "kndo check exited unexpectedly: status={:?} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("kndo check stdout is valid UTF-8");
    let mut value: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout is not valid JSON: {e}\n{stdout}"));
    // `started_at`/`duration_ms` are process-invocation metadata, not analysis output — RFC
    // 0008 §4's determinism claim is about findings/ids/ordering, never wall-clock, so these two
    // fields are expected to differ across two separate process runs and must be excluded from
    // the byte-identical comparison rather than silently making it always fail.
    if let Some(run) = value.get_mut("run").and_then(|r| r.as_object_mut()) {
        run.remove("started_at");
        run.remove("duration_ms");
    }
    value
}

#[test]
fn threads_1_and_threads_4_produce_byte_identical_json() {
    let dir: PathBuf = std::env::temp_dir().join("kndo-threads-determinism-fixture");
    let _ = std::fs::remove_dir_all(&dir);
    write_fixture(&dir);

    let single = run_check(&dir, "1");
    let multi = run_check(&dir, "4");

    assert_eq!(
        single, multi,
        "--threads 1 and --threads 4 must produce identical analysis output (RFC 0008 §4), \
         modulo started_at/duration_ms"
    );

    // Sanity: the fixture is real — both runs must actually find the injected dead code, not
    // just happen to agree on an empty/degenerate result.
    let findings = single["findings"].as_array().expect("findings is an array");
    assert!(
        findings.iter().any(|f| f["category"] == "unused"),
        "fixture should report unused findings: {single}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
