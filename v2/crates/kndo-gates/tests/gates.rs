//! One `#[test]` per registry entry; the generated workflow runs each by exact name
//! and greps this harness's `--list` output for existence.

use kndo_gates::{render_ci, workflow_path};

#[test]
fn contract_fingerprint_is_intentional() {
    let committed = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../kndo-contract/fingerprint.txt"
    ))
    .expect("fingerprint.txt exists — run `cargo xtask gen-fingerprint`");
    assert_eq!(
        committed.trim(),
        kndo_contract::contract_fingerprint_hex(),
        "\nthe contract's shape changed. If that is deliberate, run \
         `cargo xtask gen-fingerprint` and commit the new fingerprint.txt in the SAME \
         commit — the diff is the announcement. If it is not deliberate, you changed \
         a contract type without meaning to.\n"
    );
}

#[test]
fn generated_ci_is_current() {
    let committed = std::fs::read_to_string(workflow_path())
        .expect("the generated workflow exists — run `cargo xtask gen-ci`");
    assert_eq!(
        committed,
        render_ci(),
        "\n.github/workflows/v2.yml drifted from kndo-gates' registry — \
         run `cargo xtask gen-ci` and commit the result\n"
    );
}

use kndo_core::{Config, GatePolicy, RunMode, RunOutcome, Session, Snapshot, Threads};
use kndo_testkit::{MockAdapter, TempProject};

fn run(root: &std::path::Path, use_cache: bool, threads: Threads) -> Snapshot {
    let session = Session::open(
        root,
        Config { threads, use_cache },
        vec![Box::new(MockAdapter::new())],
    )
    .expect("open session");
    session.analyze(RunMode::Full).expect("analyze")
}

fn serialized(snap: &Snapshot) -> (String, String) {
    (snap.graph.to_json(), snap.report().to_json())
}

fn fixture() -> TempProject {
    let p = TempProject::new();
    p.file(
        "main.kmock",
        "root-file\nimport ./lib { helper }\ncall helper\nfn local_used\ncall local_used\nfn dead_one\n# a note\n",
    );
    p.file(
        "lib.kmock",
        "pub fn helper\npub fn unused_export\nfn private_dead\n",
    );
    p.file("orphan.kmock", "fn floats\n");
    p
}

#[test]
fn dogfood_kndo_reports_nothing_on_itself() {
    // The REAL default adapter set over this repository — the same composition a user
    // runs. The repo's JS surface today has no roots, so `unused` abstains rather than
    // accusing (an abstention is honest; a finding here would be a bug in ours to fix).
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let session = kndo::open(repo_root, Config::default()).expect("open repo");
    let snap = session.analyze(RunMode::Full).expect("analyze repo");
    assert!(
        snap.findings.is_empty(),
        "kndo-on-kndo must report nothing; got {:#?}",
        snap.findings
    );
    assert!(
        !snap.graph.files.is_empty(),
        "the default adapters claim this repository's own JS surface — an empty claim \
         set would make this gate vacuous"
    );
}

#[test]
fn report_schema_is_generated_and_valid() {
    // The committed schema is derived, never hand-written; and it VALIDATES real
    // output — the fixture's own report and a versioned corpus report — so the
    // schema being "current" also means being true of what the engine emits.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let committed = std::fs::read_to_string(root.join("schemas/report.schema.json"))
        .expect("schemas/report.schema.json exists — run `cargo xtask gen-schema`");
    assert_eq!(
        committed,
        kndo_core::report_schema(),
        "\nthe envelope's shape changed. If deliberate, run `cargo xtask gen-schema` \
         and commit the result in the same commit; if not, you changed the Report \
         without meaning to.\n"
    );

    let schema: serde_json::Value = serde_json::from_str(&committed).expect("schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");

    let p = fixture();
    let live: serde_json::Value =
        serde_json::from_str(&run(p.root(), false, Threads::Auto).report().to_json())
            .expect("report is JSON");
    let errors: Vec<String> = validator
        .iter_errors(&live)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "a live report validates: {errors:#?}");

    let vite = std::fs::read_to_string(root.join("corpus-findings/vite.report.json"))
        .expect("versioned corpus report exists");
    let vite: serde_json::Value = serde_json::from_str(&vite).expect("corpus report is JSON");
    let errors: Vec<String> = validator
        .iter_errors(&vite)
        .map(|e| e.to_string())
        .collect();
    assert!(
        errors.is_empty(),
        "the corpus report validates: {errors:#?}"
    );
}

#[test]
fn dogfood_zero_means_measured() {
    // The dogfood's second lock: zero findings must mean MEASURED, not un-judged.
    // Every abstention on the repo run is listed here with its written reason; the
    // set must match EXACTLY — a change that silently makes an analysis abstain
    // (cheapening the zero) fails, and so does one that silently starts judging
    // (the accepted entry must be retired deliberately).
    //
    // Accepted abstentions, one shared cause: the repo's only claimed JS today is
    // v1's `action/render.mjs`, whose root is `action.yml` — a file no v2 adapter
    // reads. No roots ⇒ no reachability evidence and no test evidence. These retire
    // with the root swap or a manifest adapter that reads workflow files.
    const ACCEPTED: &[(&str, &str)] = &[
        ("unused", "no root anchors any file in this graph"),
        ("test-only", "no test root anchors any file in this graph"),
        ("untested", "no test root anchors any file in this graph"),
    ];

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let session = kndo::open(repo_root, Config::default()).expect("open repo");
    let snap = session.analyze(RunMode::Full).expect("analyze repo");
    let actual: Vec<(String, String)> = snap
        .abstained
        .iter()
        .map(|a| (a.category.as_str().to_string(), a.reason.to_string()))
        .collect();
    let expected: Vec<(String, String)> = ACCEPTED
        .iter()
        .map(|(c, r)| (c.to_string(), r.to_string()))
        .collect();
    assert_eq!(
        actual, expected,
        "\nthe dogfood abstention set moved. If deliberate, update ACCEPTED with a \
         written reason in the same commit; if not, an analysis started abstaining \
         (or judging) on this repository without you meaning it to.\n"
    );
}

#[test]
fn warm_and_cold_runs_are_byte_identical() {
    let p = fixture();
    let cold = serialized(&run(p.root(), true, Threads::Auto));
    let warm = serialized(&run(p.root(), true, Threads::Auto));
    let uncached = serialized(&run(p.root(), false, Threads::Auto));
    assert_eq!(cold, warm, "second (warm) run must not change a byte");
    assert_eq!(
        cold, uncached,
        "the cache may only change speed, never output"
    );

    let entries = std::fs::read_dir(p.root().join(".kndo/cache/evidence"))
        .expect("evidence cache dir exists")
        .count();
    assert!(
        entries >= 3,
        "the cache actually engaged (one entry per file)"
    );

    let snap = run(p.root(), true, Threads::Auto);
    assert_eq!(
        snap.gate(&GatePolicy {
            fail_on: Some(kndo_contract::finding::Severity::Warning)
        }),
        RunOutcome::FailFindings { at_or_above: 4 },
        "the fixture's known findings: orphan file, dead_one, unused_export, private_dead"
    );
}

#[test]
fn threads_one_and_many_are_byte_identical() {
    let p = fixture();
    let one = serialized(&run(p.root(), false, Threads::Count(1)));
    let many = serialized(&run(p.root(), false, Threads::Count(4)));
    assert_eq!(one, many);
}

#[test]
fn adapter_conformance_fixtures_are_byte_identical() {
    // The harvested regression floor: v1's js fixture corpus (every false-positive
    // hunt it encodes) replayed through the v2 engine, each report pinned byte-for-
    // byte. A diff is either your bug or a deliberate, documented contract change —
    // regenerate with KNDO_CONFORMANCE=overwrite and justify the diff in the PR;
    // the pinned reports GROW as analyses land, which is the point of pinning them.
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../kndo-adapter-ts/tests/fixtures");
    let mut names: Vec<_> = std::fs::read_dir(&fixtures)
        .expect("fixture corpus exists")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(names.len() >= 22, "the harvested corpus is present");

    let overwrite = std::env::var_os("KNDO_CONFORMANCE").is_some_and(|v| v == "overwrite");
    let mut failures = Vec::new();
    for name in &names {
        let dir = fixtures.join(name);
        let session = kndo::open(
            dir.join("project"),
            Config {
                threads: Threads::Auto,
                use_cache: false,
            },
        )
        .expect("open fixture project");
        let report = session
            .analyze(RunMode::Full)
            .expect("analyze fixture")
            .report()
            .to_json();
        let expected_path = dir.join("expected.json");
        if overwrite {
            std::fs::write(&expected_path, &report).expect("write expected");
            continue;
        }
        let expected = std::fs::read_to_string(&expected_path)
            .unwrap_or_else(|_| panic!("{name}/expected.json exists — regenerate deliberately"));
        if report != expected {
            failures.push(name.clone());
        }
    }
    assert!(
        failures.is_empty(),
        "conformance fixtures diverged: {failures:?} — a bug, or a deliberate \
         contract change to regenerate (KNDO_CONFORMANCE=overwrite) and document"
    );
}

#[test]
fn incremental_and_full_assembly_are_byte_identical() {
    // Patch ≡ full, in all three shapes of change: content-only (the surgical path
    // patches the persisted graph in place), a new file and a deleted file (the file
    // set moved, so the patch declines and assembly rebuilds from cached evidence).
    // Every cached run must serialize identically to a from-scratch build.
    let p = fixture();
    run(p.root(), true, Threads::Auto);
    assert!(
        p.root().join(".kndo/cache/graph.bin").is_file(),
        "the graph cache engaged"
    );

    p.file(
        "main.kmock",
        "root-file\nimport ./lib { helper }\ncall helper\nfn local_used\ncall local_used\nfn dead_one\nfn appended_dead\n# a note\n",
    );
    let patched = serialized(&run(p.root(), true, Threads::Auto));
    let from_scratch = serialized(&run(p.root(), false, Threads::Auto));
    assert_eq!(patched, from_scratch, "content-only change: patched ≡ full");

    let snap = run(p.root(), true, Threads::Auto);
    assert!(
        snap.findings
            .iter()
            .any(|f| f.subject.path().as_str() == "main.kmock"
                && f.message.contains("declaration")
                && format!("{:?}", f.subject).contains("appended_dead")),
        "the changed file's new dead symbol is seen through the incremental path"
    );

    p.file("extra.kmock", "fn lonely\n");
    let added = serialized(&run(p.root(), true, Threads::Auto));
    let added_full = serialized(&run(p.root(), false, Threads::Auto));
    assert_eq!(added, added_full, "added file: rebuilt ≡ full");
    assert!(
        added.0.contains("extra.kmock"),
        "the new file joined the graph through the cached path"
    );

    std::fs::remove_file(p.root().join("orphan.kmock")).expect("delete orphan");
    let removed = serialized(&run(p.root(), true, Threads::Auto));
    let removed_full = serialized(&run(p.root(), false, Threads::Auto));
    assert_eq!(removed, removed_full, "deleted file: rebuilt ≡ full");
    assert!(
        !removed.0.contains("orphan.kmock"),
        "the deleted file left the graph"
    );
}
