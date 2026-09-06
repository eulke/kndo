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
    let committed = std::fs::read_to_string(kndo_gates::release_workflow_path())
        .expect("the generated release workflow exists — run `cargo xtask gen-ci`");
    assert_eq!(
        committed,
        kndo_gates::render_release(),
        "\n.github/workflows/v2-release.yml drifted from kndo-gates' release table — \
         run `cargo xtask gen-ci` and commit the result\n"
    );
}

use kndo_core::{
    CacheLocation, Categories, Config, GatePolicy, Mode, RunMode, RunOutcome, Session, Snapshot,
    Threads,
};
use kndo_testkit::{MockAdapter, TempProject};

fn run(root: &std::path::Path, cache: CacheLocation, threads: Threads) -> Snapshot {
    let session = Session::open(
        root,
        Config {
            threads,
            cache,
            ..Config::default()
        },
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
    // runs. The subject is this repository (the root `.ignore` keeps fixture corpora,
    // measurement records and vendored source out): every analysis judges it, and a
    // finding here is a bug in ours to fix or dead code of ours to delete — never an
    // entry to allowlist.
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let session = kndo::open(
        repo_root,
        // Cache off: this gate's subject is findings, not caching (the
        // byte-identity gates own that), and two cache-on dogfood runs over
        // the same repo root race each other's .kndo/cache in the parallel
        // test harness.
        Config {
            cache: CacheLocation::Off,
            ..Config::default()
        },
    )
    .expect("open repo");
    let snap = session.analyze(RunMode::Full).expect("analyze repo");
    assert!(
        snap.findings.is_empty(),
        "kndo-on-kndo must report nothing; got {:#?}",
        snap.findings
    );
    assert!(
        !snap.graph.files.is_empty(),
        "the default adapters claim this repository's own source — an empty claim \
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
    let live: serde_json::Value = serde_json::from_str(
        &run(p.root(), CacheLocation::Off, Threads::Auto)
            .report()
            .to_json(),
    )
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
    // One accepted abstention: `crap` needs a coverage report, which is run
    // input, never part of the tree — this repository ships none, so the
    // analysis cannot judge the dogfood run and says so. Every other analysis
    // judges it (the Rust adapter's Cargo.toml roots anchor this repository's
    // own crates), so the zero above is fully measured — any OTHER entry
    // returning here is an analysis that quietly stopped judging.
    const ACCEPTED: &[(&str, &str)] = &[("crap", "no coverage report ingested this run")];

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let session = kndo::open(
        repo_root,
        // Cache off: this gate's subject is findings, not caching (the
        // byte-identity gates own that), and two cache-on dogfood runs over
        // the same repo root race each other's .kndo/cache in the parallel
        // test harness.
        Config {
            cache: CacheLocation::Off,
            ..Config::default()
        },
    )
    .expect("open repo");
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

    // The same lock for the diagnostics channel: the repo run emits none, so any
    // that appear (a parse failure, a pragma problem, a clamped span) are a
    // regression to explain, never background noise.
    let diagnostics = snap.report().diagnostics;
    assert!(
        diagnostics.is_empty(),
        "the dogfood run emits no diagnostics; got {diagnostics:#?}"
    );
}

#[test]
fn warm_and_cold_runs_are_byte_identical() {
    let p = fixture();
    let cold = serialized(&run(p.root(), CacheLocation::InTree, Threads::Auto));
    let warm = serialized(&run(p.root(), CacheLocation::InTree, Threads::Auto));
    let uncached = serialized(&run(p.root(), CacheLocation::Off, Threads::Auto));
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

    let snap = run(p.root(), CacheLocation::InTree, Threads::Auto);
    assert_eq!(
        snap.gate(&GatePolicy {
            fail_on: Some(kndo_contract::finding::Severity::Warning)
        }),
        RunOutcome::FailFindings { at_or_above: 4 },
        "the fixture's known findings: orphan file, dead_one, unused_export, private_dead"
    );
}

/// The diff modes' contract: two pinned trees of one project share the
/// project's cache. Entries are content-addressed and keyed by everything that
/// could change them, so a copy of a tree analyzed against the original's cache
/// reads it — no new evidence entry for identical content, the graph patched
/// rather than rebuilt — and reports the very bytes an uncached run reports;
/// a changed file adds exactly its own entries.
#[test]
fn a_shared_cache_is_read_and_warmed_across_trees() {
    let original = fixture();
    run(original.root(), CacheLocation::InTree, Threads::Auto);
    let shared = original.root().join(".kndo/cache");
    let entries = || {
        std::fs::read_dir(shared.join("evidence"))
            .expect("evidence cache dir exists")
            .count()
    };
    let warmed = entries();
    assert!(warmed >= 3, "the original's run wrote one entry per file");

    let copy = fixture();
    let over_shared = run(
        copy.root(),
        CacheLocation::At(shared.clone()),
        Threads::Auto,
    );
    assert_eq!(
        entries(),
        warmed,
        "identical content over the shared cache adds no evidence entry"
    );
    assert_eq!(
        over_shared.timings.extract,
        std::time::Duration::ZERO,
        "the copy's run patched the persisted graph: nothing left to extract"
    );
    assert_eq!(
        serialized(&over_shared),
        serialized(&run(copy.root(), CacheLocation::Off, Threads::Auto)),
        "a shared cache may only change speed, never output"
    );
    assert!(
        !copy.root().join(".kndo").exists(),
        "nothing is written into the tree that borrowed the cache"
    );

    copy.file(
        "lib.kmock",
        "pub fn helper\npub fn unused_export\nfn private_dead\nfn extra\n",
    );
    let changed = run(
        copy.root(),
        CacheLocation::At(shared.clone()),
        Threads::Auto,
    );
    assert_eq!(
        entries(),
        warmed + 1,
        "a changed file adds exactly its own entry"
    );
    assert_eq!(
        serialized(&changed),
        serialized(&run(copy.root(), CacheLocation::Off, Threads::Auto)),
    );
}

/// A pinned side is a persisted analysis. The comparison a run composes from
/// one must be byte-identical to the comparison composed from a fresh analysis
/// of the same tree; a side is found only under the identity that produced it
/// — the same judgment scope, the same tree — and never through a cache that
/// is off.
#[test]
fn a_pinned_base_side_reports_the_bytes_of_a_fresh_one() {
    let base = fixture();
    let current = fixture();
    // The change heals one finding: the private dead function leaves lib.kmock.
    current.file("lib.kmock", "pub fn helper\npub fn unused_export\n");
    let session = |root: &std::path::Path, cache: CacheLocation, categories: Categories| {
        Session::open(
            root,
            Config {
                cache,
                categories,
                ..Config::default()
            },
            vec![Box::new(MockAdapter::new())],
        )
        .expect("open session")
    };
    let fresh = session(base.root(), CacheLocation::Off, Categories::All)
        .analyze(RunMode::Full)
        .expect("analyze")
        .pinned_side();
    let mut composed_fresh = session(current.root(), CacheLocation::Off, Categories::All)
        .analyze(RunMode::Full)
        .expect("analyze");
    composed_fresh.against(&fresh, Mode::Diff);

    let store = TempProject::new();
    let shared = store.root().join("cache");
    let pinning = session(
        base.root(),
        CacheLocation::At(shared.clone()),
        Categories::All,
    );
    assert!(pinning.pinned("tree-a").is_none(), "nothing pinned yet");
    pinning.pin("tree-a", &fresh);
    let reader = session(
        current.root(),
        CacheLocation::At(shared.clone()),
        Categories::All,
    );
    let found = reader
        .pinned("tree-a")
        .expect("found under the same identity");
    let mut composed_pinned = reader.analyze(RunMode::Full).expect("analyze");
    composed_pinned.against(&found, Mode::Diff);
    assert_eq!(
        serialized(&composed_fresh),
        serialized(&composed_pinned),
        "a pinned side may only change the work, never a byte of the report"
    );
    let report = composed_pinned.report();
    assert_eq!(
        report.fixed.len(),
        1,
        "the comparison saw the healed finding: {}",
        report.to_json()
    );

    let narrowed = session(
        current.root(),
        CacheLocation::At(shared.clone()),
        Categories::Only(vec![kndo_contract::vocab::Category::UNUSED]),
    );
    assert!(
        narrowed.pinned("tree-a").is_none(),
        "another judgment scope is another identity"
    );
    assert!(
        reader.pinned("tree-b").is_none(),
        "another tree is another side"
    );
    let off = session(base.root(), CacheLocation::Off, Categories::All);
    off.pin("tree-a", &fresh);
    assert!(
        off.pinned("tree-a").is_none(),
        "a cache that is off pins nothing and reads nothing"
    );
    assert!(!base.root().join(".kndo").exists());
}

#[test]
fn threads_one_and_many_are_byte_identical() {
    let p = fixture();
    let one = serialized(&run(p.root(), CacheLocation::Off, Threads::Count(1)));
    let many = serialized(&run(p.root(), CacheLocation::Off, Threads::Count(4)));
    assert_eq!(one, many);
}

/// The harvested fixture corpora and the floor each must hold — one list, read by
/// every gate that replays fixtures.
fn fixture_corpora() -> Vec<(std::path::PathBuf, usize)> {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    vec![
        (manifest.join("../kndo-adapter-ts/tests/fixtures"), 25),
        (manifest.join("../kndo-adapter-rust/tests/fixtures"), 26),
        (manifest.join("../kndo-adapter-go/tests/fixtures"), 8),
        (manifest.join("../kndo-adapter-java/tests/fixtures"), 6),
        (manifest.join("../kndo-adapter-kotlin/tests/fixtures"), 5),
        (manifest.join("../kndo-adapter-swift/tests/fixtures"), 4),
        (manifest.join("../kndo-adapter-python/tests/fixtures"), 7),
        (manifest.join("../kndo-adapter-html/tests/fixtures"), 1),
        (manifest.join("../kndo-adapter-css/tests/fixtures"), 4),
        (manifest.join("../kndo-apple/tests/fixtures"), 1),
    ]
}

/// Every fixture directory, sorted, with its corpus floor asserted; `f` gets the
/// fixture's name, its directory and the snapshot of one uncached run.
fn for_each_fixture(mut f: impl FnMut(&str, &std::path::Path, &kndo::Snapshot)) {
    for (fixtures, floor) in fixture_corpora() {
        let mut names: Vec<_> = std::fs::read_dir(&fixtures)
            .expect("fixture corpus exists")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert!(names.len() >= floor, "the harvested corpus is present");
        for name in &names {
            let dir = fixtures.join(name);
            let session = kndo::open(
                dir.join("project"),
                Config {
                    threads: Threads::Auto,
                    cache: CacheLocation::Off,
                    ..Config::default()
                },
            )
            .expect("open fixture project");
            let snapshot = session.analyze(RunMode::Full).expect("analyze fixture");
            f(name, &dir, &snapshot);
        }
    }
}

#[test]
fn adapter_conformance_fixtures_are_byte_identical() {
    // The harvested regression floor: v1's fixture corpora (every false-positive
    // hunt they encode) replayed through the v2 engine, each report pinned byte-for-
    // byte. A diff is either your bug or a deliberate, documented contract change —
    // regenerate with KNDO_CONFORMANCE=overwrite and justify the diff in the PR;
    // the pinned reports GROW as analyses land, which is the point of pinning them.
    let overwrite = std::env::var_os("KNDO_CONFORMANCE").is_some_and(|v| v == "overwrite");
    let mut failures = Vec::new();
    for_each_fixture(|name, dir, snapshot| {
        let report = snapshot.report().to_json();
        let expected_path = dir.join("expected.json");
        if overwrite {
            std::fs::write(&expected_path, &report).expect("write expected");
            return;
        }
        let expected = std::fs::read_to_string(&expected_path)
            .unwrap_or_else(|_| panic!("{name}/expected.json exists — regenerate deliberately"));
        if report != expected {
            failures.push(name.to_string());
        }
    });
    assert!(
        failures.is_empty(),
        "conformance fixtures diverged: {failures:?} — a bug, or a deliberate \
         contract change to regenerate (KNDO_CONFORMANCE=overwrite) and document"
    );
}

#[test]
fn fixture_expectations_hold() {
    // A fixture's claims are data (`expectations.toml`, see kndo_testkit::expectations):
    // what must be reported, what must stay alive, and the gaps the tree cannot close
    // yet. Checked beside the byte pin, so a comment can never contradict a pin — and
    // a known gap fails the day it closes, so the ledger cannot rot.
    use kndo_testkit::expectations::{Expectations, Reported};
    let mut failures: Vec<String> = Vec::new();
    for_each_fixture(|name, dir, snapshot| {
        let path = dir.join("expectations.toml");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => {
                failures.push(format!("{name}: expectations.toml is missing"));
                return;
            }
        };
        let expectations = match Expectations::parse(&text) {
            Ok(e) => e,
            Err(e) => {
                failures.push(format!("{name}: expectations.toml does not parse: {e}"));
                return;
            }
        };
        if expectations.dead.is_empty()
            && expectations.alive.is_empty()
            && expectations.known_gap.is_empty()
        {
            failures.push(format!("{name}: expectations.toml claims nothing"));
            return;
        }
        let report = snapshot.report();
        let reported: Vec<Reported> = report.findings.iter().map(Reported::of).collect();
        let exists = |raw: &str| kndo::query::selector_exists(&snapshot.graph, raw);
        for v in expectations.check(&reported, &exists) {
            failures.push(format!("{name}: {v}"));
        }
    });
    assert!(
        failures.is_empty(),
        "fixture expectations violated:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn contract_changes_are_loud() {
    // A change to a pinned report, to the contract fingerprint or to the graph
    // semantics version is a contract change, and a contract change is loud: the
    // same range of commits appends to DECISIONS.md and names what it moved. The
    // range comes from CI (KNDO_LOUD_RANGE, the pull request's base..head) or is
    // the last commit; a checkout too shallow to diff cannot vouch and says so.
    let range = std::env::var("KNDO_LOUD_RANGE").unwrap_or_else(|_| "HEAD~1..HEAD".to_string());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let git = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    if git(&["rev-parse", "--verify", "--quiet", "HEAD~1"]).is_none()
        && !std::env::var_os("KNDO_LOUD_RANGE").is_some()
    {
        // A single-commit history (a fresh shallow clone) has no range to judge.
        return;
    }
    let changed = git(&["diff", "--name-only", &range])
        .unwrap_or_else(|| panic!("git diff over {range} — the checkout must hold the range"));
    let changed: Vec<&str> = changed.lines().collect();
    let fixture_of = |path: &str| -> Option<(String, String)> {
        // crates/<crate>/tests/fixtures/<name>/expected.json
        let parts: Vec<&str> = path.split('/').collect();
        (parts.len() == 6
            && parts[0] == "crates"
            && parts[2] == "tests"
            && parts[3] == "fixtures"
            && parts[5] == "expected.json")
            .then(|| (parts[1].to_string(), parts[4].to_string()))
    };
    let mut loud: Vec<(String, Vec<String>)> = Vec::new();
    for path in &changed {
        if let Some((krate, name)) = fixture_of(path) {
            loud.push((
                format!("{krate}/{name}"),
                vec![
                    name.clone(),
                    format!("{krate} fixtures"),
                    "every conformance fixture".into(),
                ],
            ));
        }
        if *path == "crates/kndo-contract/fingerprint.txt" {
            loud.push((
                "the contract fingerprint".into(),
                vec!["fingerprint".into()],
            ));
        }
    }
    let semantics =
        git(&["diff", &range, "--", "crates/kndo-core/src/graph.rs"]).unwrap_or_default();
    if semantics
        .lines()
        .any(|l| l.starts_with(['+', '-']) && l.contains("pub const GRAPH_SEMANTICS_VERSION"))
    {
        loud.push((
            "GRAPH_SEMANTICS_VERSION".into(),
            vec!["GRAPH_SEMANTICS_VERSION".into(), "graph semantics".into()],
        ));
    }
    if loud.is_empty() {
        return;
    }
    let added: String = git(&["diff", &range, "--", "DECISIONS.md"])
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .map(|l| l[1..].to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let unspoken: Vec<&str> = loud
        .iter()
        .filter(|(_, mentions)| {
            !mentions
                .iter()
                .any(|m| added.contains(&m.to_ascii_lowercase()))
        })
        .map(|(what, _)| what.as_str())
        .collect();
    assert!(
        unspoken.is_empty(),
        "contract changes in {range} that DECISIONS.md does not name: {unspoken:?} — a \
         pinned report, the fingerprint or the graph semantics moved; append the entry \
         (name each fixture, or `<crate> fixtures`, or `every conformance fixture`)"
    );
}

#[test]
fn incremental_and_full_assembly_are_byte_identical() {
    // Patch ≡ full, in all three shapes of change: content-only (the surgical path
    // patches the persisted graph in place), a new file and a deleted file (the file
    // set moved, so the patch declines and assembly rebuilds from cached evidence).
    // Every cached run must serialize identically to a from-scratch build.
    let p = fixture();
    run(p.root(), CacheLocation::InTree, Threads::Auto);
    assert!(
        p.root().join(".kndo/cache/graph.bin").is_file(),
        "the graph cache engaged"
    );

    p.file(
        "main.kmock",
        "root-file\nimport ./lib { helper }\ncall helper\nfn local_used\ncall local_used\nfn dead_one\nfn appended_dead\n# a note\n",
    );
    let patched = serialized(&run(p.root(), CacheLocation::InTree, Threads::Auto));
    let from_scratch = serialized(&run(p.root(), CacheLocation::Off, Threads::Auto));
    assert_eq!(patched, from_scratch, "content-only change: patched ≡ full");

    let snap = run(p.root(), CacheLocation::InTree, Threads::Auto);
    assert!(
        snap.findings
            .iter()
            .any(|f| f.subject.path().as_str() == "main.kmock"
                && f.message.contains("declaration")
                && format!("{:?}", f.subject).contains("appended_dead")),
        "the changed file's new dead symbol is seen through the incremental path"
    );

    p.file("extra.kmock", "fn lonely\n");
    let added = serialized(&run(p.root(), CacheLocation::InTree, Threads::Auto));
    let added_full = serialized(&run(p.root(), CacheLocation::Off, Threads::Auto));
    assert_eq!(added, added_full, "added file: rebuilt ≡ full");
    assert!(
        added.0.contains("extra.kmock"),
        "the new file joined the graph through the cached path"
    );

    std::fs::remove_file(p.root().join("orphan.kmock")).expect("delete orphan");
    let removed = serialized(&run(p.root(), CacheLocation::InTree, Threads::Auto));
    let removed_full = serialized(&run(p.root(), CacheLocation::Off, Threads::Auto));
    assert_eq!(removed, removed_full, "deleted file: rebuilt ≡ full");
    assert!(
        !removed.0.contains("orphan.kmock"),
        "the deleted file left the graph"
    );
}

#[test]
fn builtin_conduct_proofs() {
    // Every built-in conducting extension ships with the baseline-then-plugin
    // proof the authoring docs demand of anyone else: the run WITHOUT it
    // establishes what fires, the run WITH it changes exactly what it claims to
    // change, and the contribution is reported in full. Closed over the conduct
    // subset of `default_extensions()`: a coordinate shipped without its proof
    // here fails, the same posture that makes the conduct gates arguments of
    // `.conduct()` — an extension nothing asserts is one nothing notices
    // breaking, and the cost is measured in findings that silently return.
    const PROVEN: &[&str] = &[
        "kndo:coverage-lcov",
        "kndo:coverage-cobertura",
        "kndo:coverage-jacoco",
        "kndo:coverage-go",
        "kndo:interface-builder",
        "kndo:info-plist",
    ];
    let shipped: Vec<String> = kndo::default_extensions()
        .iter()
        .filter(|e| e.spec().declares_conduct())
        .map(|e| e.spec().coordinate().to_string())
        .collect();
    assert_eq!(
        shipped, PROVEN,
        "\nthe default conducting set moved. Every built-in coordinate needs its \
         baseline-then-plugin proof added to this gate in the same commit.\n"
    );

    // The proof's two runs over one fixture: the stock composition minus every
    // conducting extension, then the stock composition.
    let baseline_then_plugins = |fixture: std::path::PathBuf| {
        let config = || Config {
            threads: Threads::Auto,
            cache: CacheLocation::Off,
            ..Config::default()
        };
        let extraction_only: Vec<Box<dyn kndo::Extension>> = kndo::default_extensions()
            .into_iter()
            .filter(|e| !e.spec().declares_conduct())
            .collect();
        let without = Session::open(&fixture, config(), extraction_only)
            .expect("open baseline session")
            .analyze(RunMode::Full)
            .expect("analyze baseline");
        let with = kndo::open(&fixture, config())
            .expect("open stock session")
            .analyze(RunMode::Full)
            .expect("analyze with plugins");
        assert!(!with.graph.files.is_empty(), "the fixture is measured");
        assert_eq!(
            without.contributions.len(),
            0,
            "the baseline run carries no plugin"
        );
        (without, with)
    };
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let contributions = |snap: &Snapshot| -> Vec<(String, u32, u32)> {
        for c in &snap.contributions {
            assert!(
                c.dropped.is_empty() && !c.content_budget_cut,
                "every contribution lands whole: {c:#?}"
            );
        }
        snap.contributions
            .iter()
            .map(|c| (c.coordinate.to_string(), c.roots, c.findings))
            .collect()
    };
    // `category name` — a finding's identity as the proofs below spell it.
    let label = |f: &kndo::Finding| -> String {
        let name = match &f.subject {
            kndo::Subject::Symbol { selector, .. } => selector.render(),
            other => format!("{other:?}"),
        };
        format!("{} {name}", f.category.as_str())
    };

    // kndo:coverage-lcov — the lcov fixture's `neverRan` finding exists only
    // because coverage was ingested: uncovered ⇒ Certain, on the function.
    let (without, with) = baseline_then_plugins(
        fixtures.join("../kndo-adapter-ts/tests/fixtures/coverage-lcov/project"),
    );
    let never_ran = |snap: &Snapshot| {
        snap.findings
            .iter()
            .filter(|f| {
                f.category.as_str() == "untested"
                    && f.confidence == kndo_contract::vocab::Confidence::Certain
                    && format!("{:?}", f.subject).contains("neverRan")
            })
            .count()
    };
    assert_eq!(
        never_ran(&without),
        0,
        "without the ingester, no coverage verdict"
    );
    assert_eq!(
        never_ran(&with),
        1,
        "with it, the uncovered function is Certain"
    );
    assert_eq!(
        without.findings.len(),
        with.findings.len() - 1,
        "the plugin's whole effect is that one finding — nothing else moved"
    );
    // Every ingester is always on and asserts no graph facts and no findings
    // of its own; a conduct extension whose activation rules match nothing is
    // no row at all.
    let ingesters = || -> Vec<(String, u32, u32)> {
        [
            "kndo:coverage-lcov",
            "kndo:coverage-cobertura",
            "kndo:coverage-jacoco",
            "kndo:coverage-go",
        ]
        .iter()
        .map(|c| (c.to_string(), 0, 0))
        .collect()
    };
    assert_eq!(contributions(&with), ingesters());

    // kndo:coverage-cobertura, kndo:coverage-jacoco, kndo:coverage-go — each
    // fixture carries the one report its producer wrote (coverage.py, the jacoco
    // Maven plugin, `go test -coverprofile`), spelled the producer's way: a file
    // name under a source root, a package and source name, an import path. With
    // the ingester the function no test ran is `untested` and Certain; without
    // it the graph's file-level Probable stands or nothing does. Nothing else
    // moves.
    let ingested = |fixture: &str, gone: &[&str], added: &[&str]| {
        let (without, with) = baseline_then_plugins(fixtures.join(fixture));
        let before: Vec<String> = without.findings.iter().map(label).collect();
        let after: Vec<String> = with.findings.iter().map(label).collect();
        let mut left: Vec<&str> = before
            .iter()
            .filter(|l| !after.contains(l))
            .map(String::as_str)
            .collect();
        left.sort_unstable();
        let mut arrived: Vec<&str> = after
            .iter()
            .filter(|l| !before.contains(l))
            .map(String::as_str)
            .collect();
        arrived.sort_unstable();
        assert_eq!(
            left, gone,
            "{fixture}: before {before:#?}\nafter {after:#?}"
        );
        assert_eq!(
            arrived, added,
            "{fixture}: before {before:#?}\nafter {after:#?}"
        );
        assert_eq!(contributions(&with), ingesters(), "{fixture}");
    };
    ingested(
        "../kndo-adapter-python/tests/fixtures/coverage-cobertura/project",
        &["untested File { path: ProjectPath(\"src/dark.py\") }"],
        &["crap classify", "untested never_run"],
    );
    ingested(
        "../kndo-adapter-java/tests/fixtures/coverage-jacoco/project",
        &[],
        &[
            "untested Classify.neverRan(int)",
            "untested Dark.untouched(int)",
        ],
    );
    ingested(
        "../kndo-adapter-go/tests/fixtures/coverage-gocover/project",
        &[],
        &["untested NeverRan"],
    );

    // kndo:interface-builder + kndo:info-plist — Alamofire's example targets in
    // shape, with Xcode's own artifacts verbatim: the iOS storyboard names
    // `MasterViewController` and `DetailViewController` and connects the
    // `titleImageView` outlet, the watchKit storyboard names
    // `HostingController`, the extension's plist names `ExtensionDelegate`.
    // Without the plugins every one of them is dead code; with them the roots
    // land on exactly those, their members leave `internal-only` with them
    // (a rooted owner is used from outside the graph's sight), and the SwiftUI
    // preview nothing names stays reported.
    let (without, with) =
        baseline_then_plugins(fixtures.join("../kndo-apple/tests/fixtures/apple-bundles/project"));
    let before: Vec<String> = without.findings.iter().map(label).collect();
    let after: Vec<String> = with.findings.iter().map(label).collect();
    let mut gone: Vec<&str> = before
        .iter()
        .filter(|l| !after.contains(l))
        .map(String::as_str)
        .collect();
    gone.sort_unstable();
    assert_eq!(
        gone,
        [
            "internal-only DetailViewController.request",
            "internal-only MasterViewController.detailViewController",
            "internal-only MasterViewController.titleImageView",
            "unused ExtensionDelegate",
            "unused HostingController",
            "unused MasterViewController",
        ],
        "the artifacts' names, and only those, stop being dead:\nbefore {before:#?}\nafter {after:#?}"
    );
    assert!(
        after.iter().all(|l| before.contains(l)),
        "a root can only keep something alive, never accuse: {after:#?}"
    );
    assert!(
        after.contains(&"unused ContentView_Previews".to_string()),
        "unrelated dead code stays reported: {after:#?}"
    );
    let mut expected = ingesters();
    expected.push(("kndo:interface-builder".to_string(), 4, 0));
    expected.push(("kndo:info-plist".to_string(), 1, 0));
    assert_eq!(
        contributions(&with),
        expected,
        "two classes, one outlet and one watchKit controller from the documents; \
         one delegate from the plist"
    );
}

#[test]
fn extension_dependency_implication() {
    // A plugin named in another plugin's `dependencies` activates even when its own
    // rules never match — the only path for a plugin whose framework is an INDIRECT
    // dependency (a company framework that uses Express internally is never
    // `express` in its users' manifests). No plugin we ship uses it, and it must
    // exist anyway; that is exactly what makes it easy to delete by accident, so
    // this gate holds it in place. B and C carry `AnyRule([])` — they can NEVER
    // self-activate; their contributions in the report ARE the implication working,
    // C transitively. D's unmatched rule proves activation is not "everything runs".
    use kndo_contract::evidence::RootKind;
    use kndo_contract::vocab::{Confidence, ProjectPath};
    use kndo_core::{
        Activation, ActivationRule, ConductSeverity, ConductTarget, Extension, ExtensionSpec,
        MutatesGraph,
    };
    use kndo_testkit::MockExtension;

    // The probe every conducting mock runs: one scoped read, reported as a
    // finding — what the content view let it see IS the assertion.
    let probing = |spec: ExtensionSpec, reads: &'static str| {
        MockExtension::scripted(spec).on_report(move |_, content, out| {
            let path = ProjectPath::new(reads);
            let message = match content.read(&path) {
                Some(bytes) => format!("read {} bytes", bytes.len()),
                None => "read denied".to_string(),
            };
            out.finding(
                "probe",
                ConductSeverity::Info,
                ConductTarget::File(path),
                Confidence::Probable,
                message,
            );
        })
    };

    let p = fixture();
    let extensions: Vec<Box<dyn Extension>> = vec![
        Box::new(MockAdapter::new()),
        Box::new(probing(
            ExtensionSpec::builder("test:framework-a", 1)
                .conduct(
                    Activation::AnyRule(vec![ActivationRule::FileExists("*.kmock".into())]),
                    MutatesGraph::No,
                )
                .dependencies(&["test:middleware-b"])
                .requested_file_access(&["main.kmock"])
                .rule("probe", "reports what the content view let it see")
                .build(),
            "main.kmock",
        )),
        // No declared file access: its probe read must come back denied — the
        // content view is deny-by-default, budgeted, never ambient. And the
        // hand-written empty rule list IS the dependency-only posture.
        Box::new(probing(
            ExtensionSpec::builder("test:middleware-b", 1)
                .conduct(Activation::AnyRule(vec![]), MutatesGraph::No)
                .dependencies(&["test:leaf-c"])
                .rule("probe", "reports what the content view let it see")
                .build(),
            "lib.kmock",
        )),
        Box::new(
            MockExtension::scripted(
                ExtensionSpec::builder("test:leaf-c", 1)
                    .conduct(Activation::AnyRule(vec![]), MutatesGraph::Yes)
                    .build(),
            )
            .on_contribute(|_, _, out| {
                out.root(
                    ConductTarget::File(ProjectPath::new("orphan.kmock")),
                    RootKind::Production,
                    Confidence::Certain,
                );
            }),
        ),
        Box::new(MockExtension::scripted(
            ExtensionSpec::builder("test:dormant-d", 1)
                .conduct(
                    Activation::AnyRule(vec![ActivationRule::FileExists("never-*.xyz".into())]),
                    MutatesGraph::No,
                )
                .build(),
        )),
    ];

    let session = Session::open(
        p.root(),
        Config {
            threads: Threads::Auto,
            cache: CacheLocation::Off,
            ..Config::default()
        },
        extensions,
    )
    .expect("open session");
    let snap = session.analyze(RunMode::Full).expect("analyze");
    let report = snap.report();

    let coordinates: Vec<&str> = report
        .plugins
        .iter()
        .map(|c| c.coordinate.as_str())
        .collect();
    assert_eq!(
        coordinates,
        ["test:framework-a", "test:middleware-b", "test:leaf-c"],
        "rule-matched, dependency-implied, transitively implied — and never dormant-d"
    );
    assert!(
        report
            .plugins
            .iter()
            .all(|c| c.dropped.is_empty() && !c.content_budget_cut),
        "every contribution applied cleanly: {:#?}",
        report.plugins
    );

    let probe = |category: &str| {
        snap.findings
            .iter()
            .find(|f| f.category.as_str() == category)
            .unwrap_or_else(|| panic!("{category} reported"))
            .message
            .clone()
    };
    assert!(
        probe("ext:test:framework-a/probe").starts_with("read "),
        "declared access reads the run's own contents"
    );
    assert_eq!(
        probe("ext:test:middleware-b/probe"),
        "read denied",
        "undeclared access is denied, not ambient"
    );

    // C's root keeps orphan.kmock alive as a FILE, so the whole-file accusation is
    // gone — while its private dead symbol is still judged: a plugin root grants
    // reachability, never amnesty.
    use kndo_contract::subject::Subject;
    let orphan_subjects: Vec<&Subject> = snap
        .findings
        .iter()
        .filter(|f| f.subject.path().as_str() == "orphan.kmock")
        .map(|f| &f.subject)
        .collect();
    assert!(
        !orphan_subjects
            .iter()
            .any(|s| matches!(s, Subject::File { .. })),
        "the contributed root reached the graph"
    );
    assert!(
        orphan_subjects
            .iter()
            .any(|s| matches!(s, Subject::Symbol { .. })),
        "unrelated dead code stays reported"
    );

    // Plugin findings are advisory by containment: the gate still counts exactly
    // the four first-party findings, never the two probes.
    assert_eq!(
        snap.gate(&GatePolicy {
            fail_on: Some(kndo_contract::finding::Severity::Info)
        }),
        RunOutcome::FailFindings { at_or_above: 4 },
    );
}

#[test]
fn abi_compat_matrix() {
    // Yesterday's binaries against today's host: the reference components are
    // PINNED under abi/compat/ and deliberately never rebuilt here — rebuilding
    // would test today's source against today's host, and the compat question is
    // the committed bytes. When the WIT evolves pre-freeze, `cargo xtask pin-abi`
    // rebuilds the pins in the SAME commit: the diff is the reviewable record of
    // the break. Each world is driven through a real session to a real verdict —
    // loading is not the promise; contributing is.
    let compat = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../abi/compat");
    let session = |p: &TempProject, conduct: Vec<Box<dyn kndo_core::Extension>>| {
        let adapter = kndo_host_wasm::WasmExtension::load(&compat.join("kmini_adapter.wasm"))
            .expect("the pinned adapter component loads against the HEAD host");
        let mut extensions: Vec<Box<dyn kndo_core::Extension>> = vec![Box::new(adapter)];
        extensions.extend(conduct);
        Session::open(
            p.root(),
            Config {
                threads: Threads::Auto,
                cache: CacheLocation::Off,
                ..Config::default()
            },
            extensions,
        )
        .expect("open")
        .analyze(RunMode::Full)
        .expect("analyze")
    };

    // The adapter world: extraction, manifest roots, guest-side resolution —
    // and the M8.a vocabulary: a marker the guest's rules exempt (`@keep`),
    // one they root (`@test`), and an import's moment (a lazy loop is no
    // hazard; the load-time one through lib.kmini is).
    let p = TempProject::new();
    p.file("kmini.pkg", "name kit\nentry lib.kmini\n");
    p.file(
        "lib.kmini",
        "pub fn shared\nfn helper\ncall helper\nuse ./app\n",
    );
    p.file(
        "app.kmini",
        "entry\nuse ./lib shared\ncall shared\nfn local_dead\n@keep\nfn parked\n@test\nfn check\n\
         lazy use ./late later\ncall later\n",
    );
    p.file("late.kmini", "pub fn later\nlazy use ./app\n");
    p.file("orphan.kmini", "fn floats\n");
    let snap = session(&p, Vec::new());
    let accused: Vec<String> = snap
        .findings
        .iter()
        .map(|f| format!("{} {:?}", f.category.as_str(), f.subject))
        .collect();
    assert!(
        accused.iter().any(|s| s.contains("local_dead"))
            && accused.iter().any(|s| s.contains("orphan.kmini"))
            && !accused.iter().any(|s| s.contains("shared"))
            && !accused.iter().any(|s| s.contains("helper"))
            && !accused.iter().any(|s| s.contains("parked"))
            && !accused.iter().any(|s| s.contains("check"))
            && !accused.iter().any(|s| s.contains("later")),
        "the pinned adapter still drives real reachability, markers and rules: {accused:#?}"
    );
    let cycles: Vec<&String> = accused.iter().filter(|s| s.starts_with("cyclic")).collect();
    assert!(
        cycles.len() == 1 && !cycles[0].contains("late.kmini"),
        "the pinned adapter's load-time loop is the one hazard: {accused:#?}"
    );

    // The plugin world: a contributed root, a scoped read, described drops.
    let p = TempProject::new();
    p.file("app.kmini", "entry\n");
    p.file("wired.kmini", "fn wired_dead\n");
    p.file("config.probe", "sixteen bytes!!\n");
    let plugin = kndo_host_wasm::WasmExtension::load(&compat.join("probe_plugin.wasm"))
        .expect("the pinned plugin component loads against the HEAD host");
    let snap = session(&p, vec![Box::new(plugin)]);
    let contribution = &snap.contributions[0];
    assert_eq!(
        (
            contribution.coordinate.as_str(),
            contribution.roots,
            contribution.findings
        ),
        ("demo:probe", 1, 1),
        "{contribution:#?}"
    );
    assert_eq!(contribution.dropped.len(), 2, "{contribution:#?}");
    assert!(
        snap.findings
            .iter()
            .any(|f| f.category.as_str() == "ext:demo:probe/note"
                && f.message == "config.probe is 16 bytes"),
        "the pinned plugin still probes scoped content: {:#?}",
        snap.findings
    );

    // The ingester world: records from the pinned guest still become a verdict.
    let p = TempProject::new();
    p.file("kmini.pkg", "name kit\nentry lib.kmini\n");
    p.file("lib.kmini", "pub fn covered\npub fn never_ran\n");
    p.file("app.kmini", "entry\nuse ./lib covered\ncall covered\n");
    p.file(
        "lcov.info",
        "SF:lib.kmini\nFN:1,covered\nFN:2,never_ran\nFNDA:3,covered\nFNDA:0,never_ran\nend_of_record\n",
    );
    let ingester = kndo_host_wasm::WasmExtension::load(&compat.join("records_ingester.wasm"))
        .expect("the pinned ingester component loads against the HEAD host");
    let snap = session(&p, vec![Box::new(ingester)]);
    assert!(
        snap.findings
            .iter()
            .any(|f| f.category.as_str() == "untested"
                && f.confidence == kndo_contract::vocab::Confidence::Certain
                && format!("{:?}", f.subject).contains("never_ran")),
        "the pinned ingester's records still assemble into the Certain verdict: {:#?}",
        snap.findings
    );

    // The two-cluster extension: its own format claimed AND its conduct chain.
    let p = TempProject::new();
    p.file(
        "kmini.pkg",
        "name kit\nentry app.kmini\ndep acme-framework\n",
    );
    p.file("app.kmini", "entry\n");
    p.file("routes.acme", "handler index\n");
    p.file("extra.kmini", "fn di_wired\n");
    let acme = kndo_host_wasm::WasmExtension::load(&compat.join("acme_framework.wasm"))
        .expect("the pinned two-cluster component loads against the HEAD host");
    let probe = kndo_host_wasm::WasmExtension::load(&compat.join("probe_plugin.wasm"))
        .expect("the pinned plugin component loads against the HEAD host");
    let snap = session(&p, vec![Box::new(acme), Box::new(probe)]);
    let coordinates: Vec<&str> = snap
        .contributions
        .iter()
        .map(|c| c.coordinate.as_str())
        .collect();
    assert_eq!(
        coordinates,
        ["acme:framework", "demo:probe"],
        "the pinned framework still activates by manifest and still chains its dependency"
    );
    assert!(
        !snap
            .findings
            .iter()
            .any(|f| format!("{:?}", f.subject).contains("extra.kmini")
                || format!("{:?}", f.subject).contains("routes.acme")),
        "both of its clusters still land: {:#?}",
        snap.findings
    );
}

#[test]
fn frontends_import_only_the_facade() {
    // The facade rule as executable law: a frontend's production dependency graph
    // contains exactly one kndo crate — `kndo` itself. Reaching into core, the
    // contract, or an adapter from a frontend is the drift that cost v1 three CLI
    // rewrites. Dev-dependencies may use the testkit: test machinery is not the
    // product graph. A new frontend joins this list, never escapes it.
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for frontend in ["kndo-cli", "kndo-serve"] {
        let path = manifest_dir.join(format!("../{frontend}/Cargo.toml"));
        let text = std::fs::read_to_string(&path).expect("frontend manifest exists");
        let value: toml::Value = text.parse().expect("frontend manifest parses");
        let deps = value
            .get("dependencies")
            .and_then(|d| d.as_table())
            .expect("frontend declares dependencies");
        assert!(
            deps.contains_key("kndo"),
            "{frontend} consumes the facade crate"
        );
        for name in deps.keys() {
            assert!(
                name == "kndo" || !name.starts_with("kndo"),
                "{frontend} depends on `{name}` — frontends import only the facade"
            );
        }
    }
}

#[test]
fn agent_format_matches_its_committed_golden() {
    // The agent format is a versioned contract: agents parse these lines, so their
    // grammar cannot drift silently. The specimen is built by hand to hold every
    // section and every subject shape at once — its job is to pin the FORMAT, not
    // the engine (the conformance fixtures pin that). A diff is either your bug or
    // a deliberate format change: regenerate with KNDO_CONFORMANCE=overwrite, and
    // if the grammar changed meaning, bump AGENT_FORMAT in the same commit.
    use kndo::{
        Abstention, AbstentionReason, AbstentionScope, Category, Confidence, Contribution,
        DiagnosticLevel, ExtensionRun, Finding, LineSpan, ProjectPath, REPORT_SCHEMA, Report,
        ReportDiagnostic, RunInfo, Severity, Span, Subject, SuppressedSummary, SymbolSelector,
        sort_findings,
    };
    use smol_str::SmolStr;

    fn lined(mut finding: Finding, start: u32, end: u32) -> Finding {
        finding.lines = Some(LineSpan { start, end });
        finding
    }

    let mut findings = vec![
        Finding::new(
            Category::UNUSED,
            Severity::Warning,
            Confidence::Certain,
            Subject::File {
                path: ProjectPath::new("src/orphan.py"),
            },
            "",
            "no root anchors this file and no reachable file imports it",
        ),
        lined(
            Finding::new(
                Category::UNUSED,
                Severity::Warning,
                Confidence::Probable,
                Subject::Symbol {
                    path: ProjectPath::new("src/store.py"),
                    selector: SymbolSelector::member("Store", "_drop"),
                    span: Span::new(120, 180),
                },
                "",
                "`Store._drop` is declared but nothing in the project uses it",
            ),
            6,
            9,
        ),
        lined(
            Finding::new(
                Category::INTERNAL_ONLY,
                Severity::Info,
                Confidence::Probable,
                Subject::Symbol {
                    path: ProjectPath::new("src/scope.swift"),
                    selector: SymbolSelector::free("Helper"),
                    span: Span::new(0, 64),
                },
                "",
                "declared `module`-scoped, but every use is within its own file",
            ),
            1,
            3,
        ),
        Finding::new(
            Category::UNUSED,
            Severity::Warning,
            Confidence::Certain,
            Subject::Dependency {
                owner_manifest: ProjectPath::new("package.json"),
                name: SmolStr::new("left-pad"),
            },
            "",
            "declared but never imported by any claimed file",
        ),
    ];
    sort_findings(&mut findings);
    let mut fixed = vec![lined(
        Finding::new(
            Category::UNUSED,
            Severity::Warning,
            Confidence::Certain,
            Subject::Symbol {
                path: ProjectPath::new("src/gone.py"),
                selector: SymbolSelector::free("_gone"),
                span: Span::new(5, 25),
            },
            "",
            "`_gone` is declared but nothing in the project uses it",
        ),
        2,
        2,
    )];
    sort_findings(&mut fixed);
    // The real model over the specimen's own findings, so the golden pins genuine
    // output: the Info and dependency-subject findings must not implicate.
    let health = kndo::Health::measure(
        &findings,
        &kndo::Universe {
            graph_subjects: 40,
            ..Default::default()
        },
        &std::collections::BTreeSet::from([Category::UNUSED]),
    );
    let report = Report {
        run: RunInfo {
            schema: REPORT_SCHEMA,
            mode: kndo::Mode::Full,
            selection: None,
            files_discovered: 12,
            files_claimed: 11,
            extensions: vec![
                ExtensionRun {
                    id: SmolStr::new("kndo:python"),
                    files: 7,
                    published_surface: Default::default(),
                    import_cycles: Default::default(),
                    dependency_scoping: Default::default(),
                    dependency_identity: Default::default(),
                    ladder: Default::default(),
                },
                ExtensionRun {
                    id: SmolStr::new("kndo:swift"),
                    files: 4,
                    published_surface: Default::default(),
                    import_cycles: Default::default(),
                    dependency_scoping: Default::default(),
                    dependency_identity: Default::default(),
                    ladder: Default::default(),
                },
            ],
        },
        health,
        base_health: None,
        findings,
        fixed,
        baselined: 3,
        abstained: vec![Abstention {
            category: Category::UNTESTED,
            reason: AbstentionReason::NoTestRootsAnywhere,
            scope: AbstentionScope::WholeRun,
        }],
        suppressed: SuppressedSummary {
            total: 3,
            by_category: vec![(Category::STALE, 1), (Category::UNUSED, 2)],
        },
        plugins: vec![
            Contribution {
                coordinate: SmolStr::new("kndo:coverage-lcov"),
                roots: 0,
                findings: 0,
                dropped: Vec::new(),
                content_budget_cut: false,
            },
            Contribution {
                coordinate: SmolStr::new("demo:probe"),
                roots: 1,
                findings: 2,
                dropped: vec!["root target `missing.cfg` resolved to nothing".to_string()],
                content_budget_cut: true,
            },
        ],
        diagnostics: vec![ReportDiagnostic {
            path: ProjectPath::new("src/broken.py"),
            level: DiagnosticLevel::Warn,
            message: "parse error: unexpected indent".to_string(),
        }],
    };

    let rendered = report.to_agent();
    let golden_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expected/agent-format.txt");
    if std::env::var_os("KNDO_CONFORMANCE").is_some_and(|v| v == "overwrite") {
        std::fs::write(&golden_path, &rendered).expect("write agent golden");
        return;
    }
    let golden = std::fs::read_to_string(&golden_path)
        .expect("tests/expected/agent-format.txt exists — regenerate deliberately");
    assert_eq!(
        rendered, golden,
        "\nthe agent format's bytes moved. If deliberate, regenerate with \
         KNDO_CONFORMANCE=overwrite and say so in the PR — and if the grammar \
         changed meaning, bump AGENT_FORMAT in the same commit.\n"
    );
}

#[test]
fn query_contract_is_generated_and_pinned() {
    use kndo::query::{Options, Outcome, Request, Verb};

    // The committed schemas are derived, never hand-written.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (file, generated) in [
        (
            "query.request.schema.json",
            kndo_core::query::request_schema(),
        ),
        (
            "query.response.schema.json",
            kndo_core::query::response_schema(),
        ),
    ] {
        let committed = std::fs::read_to_string(root.join("schemas").join(file))
            .unwrap_or_else(|_| panic!("schemas/{file} exists — run `cargo xtask gen-schema`"));
        assert_eq!(
            committed, generated,
            "\nschemas/{file} drifted from the types — run `cargo xtask gen-schema` \
             and commit the result in the same commit.\n"
        );
    }

    let p = fixture();
    let snapshot = run(p.root(), CacheLocation::Off, Threads::Auto);

    // A live response validates against the committed response schema.
    let live = snapshot.query(&Request {
        verb: Verb::UsedBy,
        inputs: vec!["lib.kmock#helper".to_string(), "nope.kmock".to_string()],
        options: Options::default(),
    });
    let schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("schemas/query.response.schema.json")).unwrap(),
    )
    .expect("schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    let value: serde_json::Value = serde_json::from_str(&live.to_json()).expect("response is JSON");
    let errors: Vec<String> = validator
        .iter_errors(&value)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "a live response validates: {errors:#?}");

    // The judge/navigator certificate: `used-by` lists the evidence `unused`
    // counted, so it must come back EMPTY for every symbol `unused` accused —
    // and non-empty for a symbol it kept.
    for finding in &snapshot.findings {
        if finding.category.as_str() != "unused" {
            continue;
        }
        let kndo::Subject::Symbol { path, selector, .. } = &finding.subject else {
            continue;
        };
        let input = format!("{}#{}", path.as_str(), selector.render());
        let response = snapshot.query(&Request {
            verb: Verb::UsedBy,
            inputs: vec![input.clone()],
            options: Options::default(),
        });
        match &response.results[0] {
            Outcome::Ok { answer } => {
                let json = serde_json::to_value(answer).unwrap();
                assert_eq!(
                    json["kept_by"].as_array().map(|k| k.len()),
                    Some(0),
                    "{input}: unused accused it, so used-by must list nothing"
                );
                assert_eq!(json["elided"], 0, "{input}");
            }
            _ => panic!("{input} must resolve to an ok outcome"),
        }
    }
    match &live.results[0] {
        Outcome::Ok { answer } => {
            let json = serde_json::to_value(answer).unwrap();
            assert!(
                json["kept_by"].as_array().is_some_and(|k| !k.is_empty()),
                "helper is kept, used-by must say by what"
            );
        }
        _ => panic!("helper must resolve to an ok outcome"),
    }
    assert!(
        matches!(&live.results[1], Outcome::NotFound { .. }),
        "a bad selector is its own not-found, never its siblings' failure"
    );

    // The agent rendering of a fixed multi-verb script is byte-pinned.
    let script = [
        Request {
            verb: Verb::Find,
            inputs: vec!["helper".to_string(), "zzz_nothing".to_string()],
            options: Options::default(),
        },
        Request {
            verb: Verb::Describe,
            inputs: vec![
                "lib.kmock#unused_export".to_string(),
                "lib.kmock".to_string(),
            ],
            options: Options::default(),
        },
        Request {
            verb: Verb::Uses,
            inputs: vec!["main.kmock".to_string()],
            options: Options::default(),
        },
        Request {
            verb: Verb::UsedBy,
            inputs: vec!["lib.kmock#helper".to_string()],
            options: Options::default(),
        },
        Request {
            verb: Verb::Trace,
            inputs: vec!["lib.kmock#helper".to_string(), "orphan.kmock".to_string()],
            options: Options::default(),
        },
        // The directed form: FROM the entry file TO the kept symbol.
        Request {
            verb: Verb::Trace,
            inputs: vec!["main.kmock".to_string()],
            options: Options {
                to: Some("lib.kmock#helper".to_string()),
                ..Options::default()
            },
        },
        Request {
            verb: Verb::Impact,
            inputs: vec!["lib.kmock".to_string()],
            options: Options {
                if_deleted: true,
                ..Options::default()
            },
        },
    ];
    let rendered: String = script
        .iter()
        .map(|request| snapshot.query(request).to_agent())
        .collect::<Vec<_>>()
        .join("---\n");
    let golden_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expected/query-agent.txt");
    if std::env::var_os("KNDO_CONFORMANCE").is_some_and(|v| v == "overwrite") {
        std::fs::write(&golden_path, &rendered).expect("write query golden");
        return;
    }
    // explain closes the loop finding-id -> subject -> why, asserted live: the
    // id is stable, but quoting it here would duplicate what the fixture pins.
    let unused_id = snapshot
        .findings
        .iter()
        .find(|f| {
            f.category.as_str() == "unused" && matches!(f.subject, kndo::Subject::Symbol { .. })
        })
        .map(|f| f.id.as_str().to_string())
        .expect("the fixture has a symbol unused finding");
    let explained = snapshot.query(&Request {
        verb: Verb::Explain,
        inputs: vec![unused_id.clone(), "kndo-000000000000".to_string()],
        options: Options::default(),
    });
    match &explained.results[0] {
        Outcome::Ok { answer } => {
            let json = serde_json::to_value(answer).unwrap();
            assert_eq!(json["finding"]["id"], unused_id.as_str());
            assert_eq!(
                json["subject"]["kept_by"]["entries"]
                    .as_array()
                    .map(|k| k.len()),
                Some(0),
                "explain of an unused finding shows the empty keeper preview"
            );
        }
        _ => panic!("a real finding id explains"),
    }
    assert!(matches!(&explained.results[1], Outcome::NotFound { .. }));

    let golden = std::fs::read_to_string(&golden_path)
        .expect("tests/expected/query-agent.txt exists — regenerate deliberately");
    assert_eq!(
        rendered, golden,
        "\nthe query agent grammar moved. If deliberate, regenerate with \
         KNDO_CONFORMANCE=overwrite and say so in the PR — and if the grammar \
         changed meaning, bump AGENT_FORMAT in the same commit.\n"
    );
}

/// Every relative Markdown link in the tree resolves. A link is its author
/// asserting a path exists, and moving a document means updating what points
/// at it in the same commit. Links only — prose paths carry examples from other
/// repositories and a user's own layout — and code spans and fenced blocks are
/// blanked first: a path inside backticks is quoted, not claimed.
#[test]
fn every_relative_markdown_link_resolves() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root");
    let mut documents = Vec::new();
    markdown_files(&root, &mut documents);
    assert!(
        documents.iter().any(|d| d.ends_with("docs/src/SUMMARY.md")),
        "the docs site is part of the tree this gate reads"
    );
    let mut broken = Vec::new();
    for document in &documents {
        let text = std::fs::read_to_string(document).expect("readable markdown");
        let dir = document.parent().expect("a file has a directory");
        for target in relative_link_targets(&text) {
            if !dir.join(&target).exists() {
                broken.push(format!(
                    "{}: [{target}]",
                    document.strip_prefix(&root).unwrap_or(document).display()
                ));
            }
        }
    }
    assert!(
        broken.is_empty(),
        "relative Markdown links naming nothing:\n{}",
        broken.join("\n")
    );
}

fn markdown_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            // Build output, third-party trees and fixture corpora are not
            // the repository's own claims.
            if matches!(
                name,
                "target" | "node_modules" | "book" | "fixtures" | "vendor"
            ) || name.starts_with('.')
            {
                continue;
            }
            markdown_files(&path, out);
        } else if name.ends_with(".md") {
            out.push(path);
        }
    }
}

/// The targets of inline links `[text](target)` that name a path: no scheme,
/// no bare fragment; a fragment or query on a path is stripped. Fenced blocks
/// and code spans are blanked before scanning.
fn relative_link_targets(text: &str) -> Vec<String> {
    let mut prose = String::with_capacity(text.len());
    let mut in_fence = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            prose.push('\n');
            continue;
        }
        if in_fence {
            prose.push('\n');
            continue;
        }
        let mut in_span = false;
        for c in line.chars() {
            if c == '`' {
                in_span = !in_span;
                prose.push(' ');
            } else if in_span {
                prose.push(' ');
            } else {
                prose.push(c);
            }
        }
        prose.push('\n');
    }
    let mut targets = Vec::new();
    let mut rest = prose.as_str();
    while let Some(at) = rest.find("](") {
        let after = &rest[at + 2..];
        let Some(end) = after.find(')') else {
            break;
        };
        let raw = after[..end].trim();
        let raw = raw.split_whitespace().next().unwrap_or("");
        let target = raw.split(['#', '?']).next().unwrap_or("");
        let is_path = !target.is_empty()
            && !raw.starts_with('#')
            && !target.contains("://")
            && !target.starts_with("mailto:");
        if is_path {
            targets.push(target.to_string());
        }
        rest = &after[end + 1..];
    }
    targets
}

#[test]
fn finding_identity_is_unique() {
    // Every subject a file can hold more than once under one spelling carries
    // its position, so no two findings of a run share an identity. Checked
    // over every pinned report — the corpus and every conformance fixture —
    // because a new subject kind that forgets its position would collide
    // there first, silently, and a baseline would then silence findings it
    // never named.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut reports: Vec<std::path::PathBuf> = Vec::new();
    for entry in std::fs::read_dir(root.join("corpus-findings")).expect("corpus-findings") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "json") {
            reports.push(path);
        }
    }
    for krate in std::fs::read_dir(root.join("crates")).expect("crates") {
        let fixtures = krate.expect("crate").path().join("tests/fixtures");
        let Ok(dirs) = std::fs::read_dir(&fixtures) else {
            continue;
        };
        for dir in dirs {
            let expected = dir.expect("fixture").path().join("expected.json");
            if expected.is_file() {
                reports.push(expected);
            }
        }
    }
    assert!(reports.len() > 50, "the pinned reports are where they were");
    let mut collisions: Vec<String> = Vec::new();
    for report in &reports {
        let text = std::fs::read_to_string(report).expect("a pinned report reads");
        let value: serde_json::Value =
            serde_json::from_str(&text).expect("a pinned report is JSON");
        let mut seen: std::collections::BTreeMap<&str, u32> = std::collections::BTreeMap::new();
        for finding in value["findings"].as_array().into_iter().flatten() {
            *seen
                .entry(finding["id"].as_str().unwrap_or(""))
                .or_insert(0) += 1;
        }
        for (id, n) in seen {
            if n > 1 {
                collisions.push(format!("{}: {id} ×{n}", report.display()));
            }
        }
    }
    assert!(
        collisions.is_empty(),
        "findings sharing one identity — a subject kind without its position: {collisions:#?}"
    );
}
