//! ABI compliance, end to end: the reference guests are built FRESH from source
//! (cargo + the `wit-component` library — the same no-special-access path a third
//! party uses), componentized in-process, and driven through real engine sessions.
//! Nothing here loads a checked-in binary — that is the compat matrix's job, whose
//! question is yesterday's bytes against today's host; this suite's question is
//! that the ABI's whole surface WORKS.

use kndo_core::{CacheLocation, Config, RunMode, Session, Snapshot, Threads};
use kndo_host_wasm::WasmExtension;
use kndo_testkit::TempProject;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

fn guests_workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../abi/guests")
}

/// Build every reference guest once per test process and componentize on demand.
/// The guests' own out-of-tree workspace and target dir are used as-is — cargo's
/// own locking makes concurrent invocations safe.
fn built_guests() -> &'static PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let dir = guests_workspace();
        let status = Command::new("cargo")
            .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
            // Cross-target guest build: instrumentation flags from the host
            // environment must not leak into a target that cannot link them.
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .current_dir(&dir)
            .status()
            .expect("invoking cargo for the guest build");
        assert!(status.success(), "reference guest build failed");
        dir.join("target/wasm32-unknown-unknown/release")
    })
}

fn component(name: &str) -> PathBuf {
    let core = built_guests().join(format!("{name}.wasm"));
    let bytes = std::fs::read(&core).expect("guest core module exists");
    let component = wit_component::ComponentEncoder::default()
        .module(&bytes)
        .expect("core module attaches")
        .encode()
        .expect("componentizes");
    let out = built_guests().join(format!("{name}.component.wasm"));
    // Tests run in parallel and several want the same component: temp-then-
    // rename, so a concurrent loader sees the old bytes or the new bytes,
    // never a torn module.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = built_guests().join(format!(
        ".{name}.component.{}-{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::write(&tmp, component).expect("write component");
    std::fs::rename(&tmp, &out).expect("publish component");
    out
}

fn kmini_session(
    p: &TempProject,
    cache: CacheLocation,
    conduct: Vec<Box<dyn kndo_core::Extension>>,
) -> Session {
    let adapter = WasmExtension::load(&component("kmini_adapter")).expect("kmini adapter loads");
    let mut extensions: Vec<Box<dyn kndo_core::Extension>> = vec![Box::new(adapter)];
    extensions.extend(conduct);
    Session::open(
        p.root(),
        Config {
            threads: Threads::Auto,
            cache,
            ..Config::default()
        },
        extensions,
    )
    .expect("open")
}

fn finding_on(snap: &Snapshot, needle: &str) -> Vec<String> {
    snap.findings
        .iter()
        .filter(|f| format!("{:?}", f.subject).contains(needle))
        .map(|f| format!("{} {}", f.category.as_str(), f.message))
        .collect()
}

#[test]
fn the_wasm_adapter_world_is_a_first_class_language() {
    let p = TempProject::new();
    p.file(
        "kmini.pkg",
        "name kit\nentry lib.kmini\ndep probe-framework\n",
    );
    p.file(
        "lib.kmini",
        "pub fn shared\nfn helper\ncall helper\n# a note\n",
    );
    p.file(
        "app.kmini",
        "entry\nuse ./lib shared\ncall shared\nuse kit\ncall from_part\nfn local_dead\n",
    );
    p.file("app_part.kmini", "fn from_part\n");
    p.file("orphan.kmini", "fn floats\n");

    let snap = kmini_session(&p, CacheLocation::Off, Vec::new())
        .analyze(RunMode::Full)
        .expect("analyze");
    assert_eq!(snap.graph.files.len(), 4, "every .kmini file claimed");

    // Extraction, manifest roots, resolution (relative AND bare-package through
    // the guest-rebuilt ResolveContext), unit mates — each visible in a verdict.
    assert!(
        finding_on(&snap, "local_dead")
            .iter()
            .any(|f| f.starts_with("unused")),
        "a private dead symbol in an entry file is judged: {:#?}",
        snap.findings
    );
    assert!(
        finding_on(&snap, "orphan.kmini")
            .iter()
            .any(|f| f.starts_with("unused")),
        "an unreachable file is judged"
    );
    assert!(
        finding_on(&snap, "helper").is_empty(),
        "a referenced private symbol is kept"
    );
    assert!(
        finding_on(&snap, "shared").is_empty(),
        "the imported symbol is kept through guest-side resolution"
    );
    assert!(
        finding_on(&snap, "from_part").is_empty(),
        "a symbol used only by a file that sees it is kept through wasm sees"
    );
    assert!(
        finding_on(&snap, "app_part.kmini").is_empty(),
        "the mate file itself is reachable through the unit edge"
    );

    // The manifest's dependency names feed activation — through the same wasm
    // manifest pipeline.
    let witness = kndo_testkit::MockExtension::scripted(
        kndo_core::ExtensionSpec::builder("test:witness", 1)
            .conduct(
                kndo_core::Activation::AnyRule(vec![
                    kndo_core::ActivationRule::ManifestDependency("probe-framework".into()),
                ]),
                kndo_core::MutatesGraph::No,
            )
            .build(),
    );
    let snap = kmini_session(&p, CacheLocation::Off, vec![Box::new(witness)])
        .analyze(RunMode::Full)
        .expect("analyze with witness");
    assert_eq!(
        snap.contributions.len(),
        1,
        "the kmini.pkg `dep` line activated the witness: {:#?}",
        snap.contributions
    );
}

#[test]
fn markers_timing_and_rules_cross_the_wire() {
    // The evidence the contract grew in M8.a, end to end through a component:
    // a marker the guest's rules root (`@test`), one they exempt (`@keep`), and
    // an import's moment — a load-time loop is a hazard, a lazy one is not.
    // Every judgment here is the host's: the guest reported syntax and data.
    use kndo_core::query::{Answer, Outcome, Request, Verb};
    let p = TempProject::new();
    p.file("kmini.pkg", "name kit\nentry app.kmini\n");
    p.file(
        "app.kmini",
        "entry\nuse ./lib shared\ncall shared\n@keep\nfn parked\nfn stale\n@test\nfn check\n\
         lazy use ./late later\ncall later\n",
    );
    p.file("lib.kmini", "pub fn shared\nuse ./app\n");
    p.file("late.kmini", "pub fn later\nlazy use ./app\n");
    let snap = kmini_session(&p, CacheLocation::Off, Vec::new())
        .analyze(RunMode::Full)
        .expect("analyze");
    assert!(
        finding_on(&snap, "stale")
            .iter()
            .any(|f| f.starts_with("unused")),
        "the unmarked dead symbol is still judged: {:#?}",
        snap.findings
    );
    assert!(
        finding_on(&snap, "parked").is_empty() && finding_on(&snap, "check").is_empty(),
        "the exempted and the test-rooted symbols are kept: {:#?}",
        snap.findings
    );
    assert!(
        finding_on(&snap, "later").is_empty(),
        "a lazy import still reaches and binds: {:#?}",
        snap.findings
    );
    let cycles: Vec<String> = snap
        .findings
        .iter()
        .filter(|f| f.category.as_str() == "cyclic")
        .map(|f| format!("{:?} {}", f.subject, f.message))
        .collect();
    assert_eq!(
        cycles.len(),
        1,
        "one load-time loop, app ↔ lib: {cycles:#?}"
    );
    assert!(
        cycles[0].contains("app.kmini") && !cycles[0].contains("late.kmini"),
        "the lazy loop through late.kmini is no hazard: {cycles:#?}"
    );
    let keepers = |selector: &str| -> Vec<String> {
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
    };
    assert_eq!(keepers("app.kmini#parked"), ["exempt"]);
    assert_eq!(keepers("app.kmini#check"), ["dispatch:test"]);
}

#[test]
fn wasm_extraction_is_deterministic_and_cache_transparent() {
    let p = TempProject::new();
    p.file("kmini.pkg", "name kit\nentry lib.kmini\n");
    p.file("lib.kmini", "pub fn shared\nfn dead\n# note\n");
    p.file("app.kmini", "entry\nuse ./lib shared\ncall shared\n");

    let report = |cache: CacheLocation| {
        let snap = kmini_session(&p, cache, Vec::new())
            .analyze(RunMode::Full)
            .expect("analyze");
        (snap.graph.to_json(), snap.report().to_json())
    };
    let cold = report(CacheLocation::InTree);
    let warm = report(CacheLocation::InTree);
    let uncached = report(CacheLocation::Off);
    assert_eq!(cold, warm, "wasm evidence rides the cache byte-identically");
    assert_eq!(cold, uncached, "the cache changes speed, never output");
}

#[test]
fn the_wasm_plugin_world_carries_the_containment_model() {
    let p = TempProject::new();
    p.file("app.kmini", "entry\nfn app_dead\n");
    p.file("wired.kmini", "fn wired_dead\n");
    p.file("orphan.kmini", "fn floats\n");
    p.file("config.probe", "sixteen bytes!!\n");

    let plugin = WasmExtension::load(&component("probe_plugin")).expect("probe plugin loads");
    let session = kmini_session(&p, CacheLocation::InTree, vec![Box::new(plugin)]);
    let snap = session.analyze(RunMode::Full).expect("analyze");

    let contribution = &snap.contributions[0];
    assert_eq!(contribution.coordinate, "demo:probe");
    assert_eq!(
        (contribution.roots, contribution.findings),
        (1, 1),
        "{contribution:#?}"
    );
    assert_eq!(
        contribution.dropped,
        [
            "root not applied: file nowhere.kmini does not resolve in the graph",
            "finding under undeclared rule `ghost`",
        ],
        "misbehavior crosses the boundary as described drops, not as effects"
    );

    // The contributed root reached the graph: wired.kmini is alive as a file,
    // its own private dead symbol still judged.
    assert!(
        !snap
            .findings
            .iter()
            .any(|f| f.subject.path().as_str() == "wired.kmini"
                && matches!(f.subject, kndo_contract::subject::Subject::File { .. })),
        "the wasm-contributed root keeps the file: {:#?}",
        snap.findings
    );
    assert!(
        finding_on(&snap, "wired_dead")
            .iter()
            .any(|f| f.starts_with("unused")),
        "a root grants reachability, never amnesty"
    );

    // The probe read its declared file through the scoped channel.
    let note = snap
        .findings
        .iter()
        .find(|f| f.category.as_str() == "ext:demo:probe/note")
        .expect("the declared-rule finding lands");
    assert_eq!(note.message, "config.probe is 16 bytes");

    // The active plugin declares mutates-graph, so the persisted graph cache is
    // bypassed wholesale — with the cache CONFIGURED on, nothing was stored.
    assert!(
        !p.root().join(".kndo/cache/graph.bin").exists(),
        "an active graph-mutating wasm plugin turns the graph cache off for the run"
    );
    assert!(
        p.root().join(".kndo/cache/evidence").is_dir(),
        "the evidence cache stays on: per-file evidence is plugin-independent"
    );
}

#[test]
fn the_wasm_ingester_world_feeds_untested_like_the_builtin() {
    let p = TempProject::new();
    p.file("kmini.pkg", "name kit\nentry lib.kmini\n");
    p.file("lib.kmini", "pub fn covered\npub fn never_ran\n");
    p.file("app.kmini", "entry\nuse ./lib covered\ncall covered\n");
    p.file(
        "lcov.info",
        "SF:lib.kmini\nFN:1,covered\nFN:2,never_ran\nFNDA:3,covered\nFNDA:0,never_ran\nend_of_record\n",
    );

    let ingester = WasmExtension::load(&component("records_ingester")).expect("ingester loads");
    let with = kmini_session(&p, CacheLocation::Off, vec![Box::new(ingester)])
        .analyze(RunMode::Full)
        .expect("analyze with ingester");
    let without = kmini_session(&p, CacheLocation::Off, Vec::new())
        .analyze(RunMode::Full)
        .expect("analyze without");

    let never_ran_untested = |snap: &Snapshot| {
        snap.findings
            .iter()
            .filter(|f| {
                f.category.as_str() == "untested"
                    && f.confidence == kndo_contract::vocab::Confidence::Certain
                    && format!("{:?}", f.subject).contains("never_ran")
            })
            .count()
    };
    assert_eq!(never_ran_untested(&without), 0);
    assert_eq!(
        never_ran_untested(&with),
        1,
        "records from the wasm guest assemble into the same Certain verdict: {:#?}",
        with.findings
    );
    assert_eq!(with.contributions[0].coordinate, "demo:lcov-records");
}

#[test]
fn a_two_cluster_extension_speaks_a_language_and_conducts() {
    // The framework case the old taxonomy could not hold in one component:
    // extraction for its own format AND conduct with dependency chaining.
    let p = TempProject::new();
    p.file(
        "kmini.pkg",
        "name kit\nentry app.kmini\ndep acme-framework\n",
    );
    p.file("app.kmini", "entry\n");
    p.file("routes.acme", "handler index\nhandler health\n");
    p.file("extra.kmini", "fn di_wired\n");

    let acme = WasmExtension::load(&component("acme_framework")).expect("acme loads");
    let probe = WasmExtension::load(&component("probe_plugin")).expect("probe loads");
    let snap = kmini_session(
        &p,
        CacheLocation::Off,
        vec![Box::new(acme), Box::new(probe)],
    )
    .analyze(RunMode::Full)
    .expect("analyze");

    // Cluster one, extraction: the .acme file is claimed, its handlers rooted.
    assert!(
        snap.graph
            .files
            .iter()
            .any(|f| f.path.as_str() == "routes.acme"),
        "the two-cluster extension claims its own format"
    );
    assert!(
        finding_on(&snap, "routes.acme").is_empty() && finding_on(&snap, "handler").is_empty(),
        "extraction-rooted route files accuse nothing: {:#?}",
        snap.findings
    );
    // Cluster two, conduct: activated by the manifest dependency name, its root
    // keeps extra.kmini, and the CHAIN activates demo:probe although probe's own
    // FileExists rule was never needed for it.
    let coordinates: Vec<&str> = snap
        .contributions
        .iter()
        .map(|c| c.coordinate.as_str())
        .collect();
    assert_eq!(
        coordinates,
        ["acme:framework", "demo:probe"],
        "manifest-activated framework, dependency-chained probe"
    );
    assert!(
        finding_on(&snap, "extra.kmini").is_empty(),
        "the conduct root keeps the DI-wired file: {:#?}",
        snap.findings
    );
}

/// Drives rude-probe over one claimed file whose CONTENT selects the
/// misbehavior, and asserts the named phase violation surfaces as a described
/// diagnostic on that file — never as data, never as a crash.
fn assert_extraction_violation(file_content: &str, violated_import: &str) {
    let p = TempProject::new();
    p.file("app.rude", file_content);

    let rude = WasmExtension::load(&component("rude_probe")).expect("rude probe loads");
    let session = Session::open(
        p.root(),
        Config {
            threads: Threads::Auto,
            cache: CacheLocation::Off,
            ..Config::default()
        },
        vec![Box::new(rude)],
    )
    .expect("open");
    let snap = session
        .analyze(RunMode::Full)
        .expect("the run never crashes");
    let report = snap.report();
    assert!(
        report.diagnostics.iter().any(|d| {
            d.path.as_str() == "app.rude"
                && d.message.contains("phase contract violation")
                && d.message.contains(violated_import)
        }),
        "the violation is named on the file it happened to: {:#?}",
        report.diagnostics
    );
}

#[test]
fn a_conduct_import_during_extraction_traps_with_a_named_violation() {
    // The hand-rolled tier: rude-probe bypasses the SDK and calls `graph-paths`
    // from its extract export.
    assert_extraction_violation("anything\n", "`graph-paths`");
}

#[test]
fn a_project_enumeration_during_extraction_traps_the_same_way() {
    // `extract` imports NOTHING: evidence caches by file content alone, so the
    // file SET may not influence it — `known-files` must trap exactly like a
    // conduct import, not answer.
    assert_extraction_violation("files\n", "`known-files`");
}

#[test]
fn the_manifest_hook_gets_bytes_and_no_project_surface() {
    // `manifest-dependencies` is bytes-in names-out. Before the manifest phase
    // existed it ran under a bare project store, where `known-files` PASSED the
    // gate and read an empty snapshot — silently wrong data. Now it traps as a
    // named violation, and the engine degrades to no names (activation stays
    // off), never to a lie.
    let p = TempProject::new();
    p.file("app.rude", "anything\n");
    p.file("manifest.rude", "files\n");

    let rude = WasmExtension::load(&component("rude_probe")).expect("rude probe loads");
    // The rude spec declares no manifests, so drive the hook directly: the
    // phase gate is the subject, not the engine's manifest routing.
    let names = kndo_core::Extension::manifest_dependencies(
        &rude,
        &kndo_contract::adapter::SourceFile {
            path: &kndo_contract::vocab::ProjectPath::new("manifest.rude"),
            content: b"files\n",
            region: None,
        },
    )
    .into_iter()
    .map(|d| d.name)
    .collect::<Vec<_>>();
    assert!(
        names.is_empty(),
        "a trapped manifest read degrades to no names, never to data"
    );
}
