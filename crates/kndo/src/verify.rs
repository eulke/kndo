//! `kndo plugin verify <component.wasm>` — the public half of the compliance
//! suite, packaged for an author's inner loop. Loads the component through the exact loaders
//! `kndo::open` discovery uses, reports what its descriptor declares (plus lint-grade warnings
//! for the mistakes the docs warn about), then drives every hook for real: the component is
//! dropped project-local into a synthesized fixture project and a genuine `kndo::open` + full
//! check runs over it — the same code path a user's project would exercise, not a mock. What
//! the component contributed comes back from the audit record the run leaves in
//! the fixture's cache.
//!
//! Deliberately *not* a conformance judgment: `verify` cannot know what a component is
//! supposed to do, only whether it loads, what it declares, and what it observably did against
//! a generic fixture. Zero contributions is a note, not a failure — most convention plugins
//! contribute nothing to a project that doesn't match their conventions.

use std::path::Path;

use kndo_core::engine::{ConfigOverrides, RunMode};

/// Which ABI world accepted the component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedKind {
    Adapter,
    Plugin,
    CoverageIngester,
}

impl VerifiedKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VerifiedKind::Adapter => "kndo:adapter",
            VerifiedKind::Plugin => "kndo:plugin",
            VerifiedKind::CoverageIngester => "coverage-ingester",
        }
    }
}

/// Everything `kndo plugin verify` learned — rendered as-is by the CLI, one line per entry.
#[derive(Debug)]
pub struct VerifyReport {
    pub kind: VerifiedKind,
    pub id: String,
    /// The descriptor's declared facts, pre-rendered (`"activation: ..."` etc.).
    pub descriptor: Vec<String>,
    /// Lint-grade notes: legal descriptors the docs warn against (plain-name ids, empty
    /// activation on something meant for global install…). Never fatal.
    pub warnings: Vec<String>,
    /// What the fixture drive observed (files claimed, contributions, diagnostics).
    pub fixture: Vec<String>,
}

/// Verify one component file against the synthesized generic fixture. `Err` = the component
/// doesn't load under either world — the combined per-world errors, same shape
/// `plugin_install`'s probe reports.
/// Why a verification could not be *attempted*.
///
/// Distinct from the report itself, and that distinction is the reason this is typed: a
/// component that loads and contributes nothing produces a `VerifyReport` full of notes, while
/// a component that is not a kndo component at all produces no report. Collapsing both into a
/// string made the two indistinguishable to anything but a human reading prose.
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("--project {0} is not a directory")]
    ProjectNotADirectory(std::path::PathBuf),

    /// All three worlds rejected the file. Every loader's reason is kept: which one the author
    /// *meant* to build is unknowable here, so dropping two of the three would be guessing at
    /// which error they need.
    #[error(
        "not a valid kndo:plugin ({plugin}), coverage-ingester ({coverage}), or kndo:adapter          ({adapter}) component"
    )]
    NotAComponent {
        plugin: String,
        coverage: String,
        adapter: String,
    },

    #[error("{while_}: {source}")]
    Io {
        while_: String,
        #[source]
        source: std::io::Error,
    },
}

impl VerifyError {
    fn io(while_: impl Into<String>) -> impl FnOnce(std::io::Error) -> VerifyError {
        let while_ = while_.into();
        move |source| VerifyError::Io { while_, source }
    }
}

pub fn verify(component: &Path) -> Result<VerifyReport, VerifyError> {
    verify_impl(component, None)
}

/// Verify against the author's own fixture project instead of the synthesized one: `project`
/// is copied whole into a temp root (never mutated — `.git`/`.kndo`/`target`/`node_modules`
/// excluded), the component staged project-local there, and the same full check runs. This is
/// the assertion half of the baseline-then-plugin authoring loop as one command.
pub fn verify_in_project(component: &Path, project: &Path) -> Result<VerifyReport, VerifyError> {
    if !project.is_dir() {
        return Err(VerifyError::ProjectNotADirectory(project.to_path_buf()));
    }
    verify_impl(component, Some(project))
}

fn verify_impl(component: &Path, project: Option<&Path>) -> Result<VerifyReport, VerifyError> {
    match kndo_plugin_api::WasmPlugin::load(component) {
        Ok(plugin) => Ok(verify_plugin(component, &plugin, project)),
        Err(plugin_err) => match kndo_plugin_api::WasmCoverageIngester::load(component) {
            Ok(ingester) => Ok(verify_coverage_ingester(&ingester)),
            Err(coverage_err) => match kndo_plugin_api::WasmAdapter::load(component) {
                Ok(adapter) => Ok(verify_adapter(component, &adapter, project)),
                Err(adapter_err) => Err(VerifyError::NotAComponent {
                    plugin: plugin_err.to_string(),
                    coverage: coverage_err.to_string(),
                    adapter: adapter_err.to_string(),
                }),
            },
        },
    }
}

/// The coverage world has no graph hooks and no fixture to drive — verification is
/// descriptor sanity plus one smoke call: `ingest_coverage` over empty bytes must return
/// (not trap), the cheapest proof the export is callable at all.
fn verify_coverage_ingester(ingester: &kndo_plugin_api::WasmCoverageIngester) -> VerifyReport {
    use kndo_core::plugin::Plugin as _;
    let d = ingester.descriptor();

    let mut descriptor = vec![format!("id: {}", d.id), format!("version: {}", d.version)];
    push_list(&mut descriptor, "detection", &d.detection);
    push_list(&mut descriptor, "report paths", &d.requested_file_access);
    push_rules(
        &mut descriptor,
        "activation",
        &describe_rules(&d.activation),
    );
    push_list(&mut descriptor, "dependencies", &d.dependencies);

    let mut warnings = Vec::new();
    if d.requested_file_access.is_empty() {
        warnings.push(
            "empty `requested-file-access`: the host will never hand this ingester a \
             report unless kndo.toml points `[plugins.<id>] report` at one"
                .to_string(),
        );
    }
    let mut fixture = Vec::new();
    let mut sink = kndo_core::coverage::CoverageSink::default();
    ingester.ingest_coverage(
        &kndo_core::adapter::ProjectPath("verify.smoke".into()),
        &[],
        &mut sink,
    );
    fixture.push("ingest-coverage(empty) returned without trapping".to_string());

    VerifyReport {
        kind: VerifiedKind::CoverageIngester,
        id: d.id.to_string(),
        descriptor,
        warnings,
        fixture,
    }
}

fn verify_plugin(
    component: &Path,
    plugin: &kndo_plugin_api::WasmPlugin,
    project: Option<&Path>,
) -> VerifyReport {
    use kndo_core::plugin::Plugin as _;
    let d = plugin.descriptor();

    let mut descriptor = vec![format!("id: {}", d.id), format!("version: {}", d.version)];
    push_list(&mut descriptor, "detection", &d.detection);
    push_rules(
        &mut descriptor,
        "activation",
        &describe_rules(&d.activation),
    );
    push_list(&mut descriptor, "dependencies", &d.dependencies);
    push_list(&mut descriptor, "file access", &d.requested_file_access);
    descriptor.push(format!("mutates graph: {}", plugin.mutates_graph()));
    // Declared rules — what this component MAY assert as findings.
    for rule in plugin.rules() {
        descriptor.push(format!("rule: {} — {}", rule.name, rule.description));
    }

    let mut warnings = Vec::new();
    if !d.id.contains('/') {
        warnings.push(format!(
            "id `{}` is a plain name, not a fetchable coordinate (github.com/<owner>/<repo>) — \
             it can only be hand-dropped into .kndo/plugins/ and can never be the target of \
             another component's `dependencies` entry",
            d.id
        ));
    }
    if d.activation.is_empty() {
        warnings.push(
            "empty `activation`: a globally installed copy will never self-activate — only \
             project-local drops or another component's `dependencies` can run it"
                .to_string(),
        );
    }

    let fixture = drive_fixture(component, &[], Some(d.id.as_str()), project);
    VerifyReport {
        kind: VerifiedKind::Plugin,
        id: d.id.to_string(),
        descriptor,
        warnings,
        fixture,
    }
}

fn verify_adapter(
    component: &Path,
    adapter: &kndo_plugin_api::WasmAdapter,
    project: Option<&Path>,
) -> VerifyReport {
    use kndo_core::adapter::LanguageAdapter as _;
    let d = adapter.descriptor();

    let mut descriptor = vec![
        format!("id: {}", d.id),
        format!("grammar version: {}", d.grammar_version),
        format!("facts schema version: {}", d.facts_schema_version),
    ];
    push_list(&mut descriptor, "file globs", &d.file_globs);
    push_list(&mut descriptor, "manifest globs", &d.manifest_globs);
    push_rules(
        &mut descriptor,
        "activation",
        &describe_rules(&d.activation),
    );
    push_list(&mut descriptor, "dependencies", &d.dependencies);

    let mut warnings = Vec::new();
    if d.file_globs.is_empty() {
        warnings.push(
            "empty `file_globs`: this adapter can never claim a file — it will load and then \
             do nothing"
                .to_string(),
        );
    }
    if d.activation.is_empty() {
        warnings.push(
            "empty `activation`: a globally installed copy will never self-activate — only \
             project-local drops or another component's `dependencies` can run it"
                .to_string(),
        );
    }

    let samples: Vec<String> = d.file_globs.iter().map(|g| sample_for_glob(g)).collect();
    let fixture = drive_fixture(component, &samples, None, project);
    VerifyReport {
        kind: VerifiedKind::Adapter,
        id: d.id.to_string(),
        descriptor,
        warnings,
        fixture,
    }
}

/// The hook drive: a temp fixture project with the component dropped project-local
/// (unconditional activation), one real full check, then the audit record read
/// back. The fixture is either synthesized (a generic manifest plus `sample_files` derived
/// from the adapter's own globs) or, with `project` set, a copy of the author's own.
fn drive_fixture(
    component: &Path,
    sample_files: &[String],
    plugin_id: Option<&str>,
    project: Option<&Path>,
) -> Vec<String> {
    // A `TempDir`: unique by construction (this is a plain function, safe to call
    // concurrently) and removed on drop, including on an early return from below.
    let dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => return vec![format!("fixture drive skipped: temp dir failed ({e})")],
    };
    drive_fixture_in(dir.path(), component, sample_files, plugin_id, project)
}

fn drive_fixture_in(
    root: &Path,
    component: &Path,
    sample_files: &[String],
    plugin_id: Option<&str>,
    project: Option<&Path>,
) -> Vec<String> {
    if let Err(e) = prepare_fixture(root, component, sample_files, project) {
        return vec![format!("fixture drive skipped: {e}")];
    }
    // The fixture directory is a fresh throwaway temp dir per drive — nothing here ever
    // benefits from a warm cache; `plugin_contributions` reads straight off this run's own
    // `RunResult` now, so a cache isn't even needed for that anymore either.
    let mut engine = match crate::open(
        root,
        ConfigOverrides {
            use_cache: false,
            threads: Some(1),
            ..ConfigOverrides::default()
        },
    ) {
        Ok(e) => e,
        Err(e) => return vec![format!("fixture drive failed: kndo::open ({e})")],
    };
    let result = engine.check(RunMode::Full);
    let mut out = run_report(&result, sample_files);
    if let Some(id) = plugin_id {
        out.extend(contribution_lines(&result, id));
    }
    out
}

/// Populate the fixture root (the author's copied project, or the synthesized generic one)
/// and stage the component project-local.
fn prepare_fixture(
    root: &Path,
    component: &Path,
    sample_files: &[String],
    project: Option<&Path>,
) -> Result<(), VerifyError> {
    match project {
        Some(source) => copy_tree(source, root)?,
        None => synthesize_fixture(root, sample_files)?,
    }
    stage_component(root, component)
}

/// The component into `.kndo/plugins/` — the tier where activation is unconditional.
fn stage_component(root: &Path, component: &Path) -> Result<(), VerifyError> {
    let plugins_dir = root.join(".kndo").join("plugins");
    std::fs::create_dir_all(&plugins_dir)
        .and_then(|()| std::fs::copy(component, plugins_dir.join("verify.wasm")))
        .map(|_| ())
        .map_err(VerifyError::io("staging the component"))
}

/// The generic fixture: a manifest (so manifest-driven activation and content reads have
/// something real to see) plus the adapter's own glob-derived samples.
fn synthesize_fixture(root: &Path, sample_files: &[String]) -> Result<(), VerifyError> {
    let _ = std::fs::write(
        root.join("package.json"),
        "{\n  \"name\": \"kndo-verify-fixture\",\n  \"version\": \"0.0.0\"\n}\n",
    );
    for name in sample_files {
        if let Some(parent) = Path::new(name).parent() {
            let _ = std::fs::create_dir_all(root.join(parent));
        }
        let _ = std::fs::write(root.join(name), "hello\n");
    }
    Ok(())
}

/// Copy the author's fixture into the temp root — their directory is never mutated (`verify`
/// must be safe to point at a real project). Housekeeping trees are skipped: `.git`,
/// `.kndo` (a stale cache or resident components would contaminate the drive), `target`,
/// `node_modules`.
fn copy_tree(source: &Path, dest: &Path) -> Result<(), VerifyError> {
    let entries = std::fs::read_dir(source)
        .map_err(VerifyError::io(format!("reading {}", source.display())))?;
    for entry in entries {
        copy_dir_entry(entry, dest)?;
    }
    Ok(())
}

fn copy_dir_entry(
    entry: std::io::Result<std::fs::DirEntry>,
    dest: &Path,
) -> Result<(), VerifyError> {
    let entry = entry.map_err(VerifyError::io(format!(
        "reading an entry of {}",
        dest.display()
    )))?;
    if is_housekeeping(&entry.file_name()) {
        return Ok(());
    }
    copy_entry(&entry, &dest.join(entry.file_name()))
}

/// Trees that must not ride into the fixture copy: VCS state, a stale kndo cache or resident
/// components, and build output.
fn is_housekeeping(name: &std::ffi::OsStr) -> bool {
    [".git", ".kndo", "target", "node_modules"]
        .iter()
        .any(|s| name == std::ffi::OsStr::new(s))
}

fn copy_entry(entry: &std::fs::DirEntry, to: &Path) -> Result<(), VerifyError> {
    let from = entry.path();
    let file_type = entry
        .file_type()
        .map_err(VerifyError::io(format!("stat of {}", to.display())))?;
    if file_type.is_dir() {
        std::fs::create_dir_all(to)
            .map_err(VerifyError::io(format!("creating {}", to.display())))?;
        copy_tree(&from, to)
    } else if file_type.is_file() {
        std::fs::copy(&from, to)
            .map(|_| ())
            .map_err(VerifyError::io(format!("copying {}", from.display())))
    } else {
        // Symlinks are skipped: a fixture project shouldn't need them, and following one
        // could escape the tree being copied.
        Ok(())
    }
}

fn run_report(result: &kndo_core::engine::RunResult, sample_files: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for info in &result.adapters {
        if info.files > 0 {
            out.push(format!(
                "adapter {} claimed {} file(s)",
                info.id, info.files
            ));
        }
    }
    if !sample_files.is_empty() {
        out.push(format!(
            "synthesized {} sample file(s) from the declared globs: {}",
            sample_files.len(),
            sample_files.join(", ")
        ));
    }
    out.push(format!(
        "full check completed: {} finding(s), {} diagnostic(s)",
        result.findings.len(),
        result.diagnostics.len()
    ));
    for diag in &result.diagnostics {
        out.push(format!("diagnostic: {}", diag.message));
    }
    out
}

fn contribution_lines(result: &kndo_core::engine::RunResult, id: &str) -> Vec<String> {
    let Some(c) = result
        .plugin_contributions
        .iter()
        .find(|c| c.id == id)
        .cloned()
    else {
        return vec![
            "no contribution record — the plugin's graph-mutation hooks did not run \
             (mutates graph: false, or the round was skipped)"
                .to_string(),
        ];
    };
    let mut out = vec![format!(
        "contributed {} root(s), {} edge(s), {} annotation(s) on the fixture \
         (zero is normal for a convention plugin the fixture doesn't match)",
        c.roots, c.edges, c.annotations
    )];
    // The author kit's answer to "why did I contribute 0" — the misses the graph contract
    // keeps silent, spelled out one per line.
    for miss in &c.dropped {
        out.push(format!("dropped: {miss}"));
    }
    out
}

fn describe_rules(rules: &[kndo_core::plugin::ActivationRule]) -> Vec<String> {
    rules.iter().map(|r| r.describe()).collect()
}

fn push_list<S: AsRef<str>>(out: &mut Vec<String>, label: &str, items: &[S]) {
    if items.is_empty() {
        out.push(format!("{label}: (none)"));
    } else {
        out.push(format!(
            "{label}: {}",
            items
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
}

fn push_rules(out: &mut Vec<String>, label: &str, described: &[String]) {
    push_list(out, label, described);
}

/// One concrete filename matching `glob`, for the fixture: the glob's final path segment with
/// every `*` made literal (`**/*.kdemo` → `sample.kdemo`, `**/BUILD` → `BUILD`).
fn sample_for_glob(glob: &str) -> String {
    let last = glob.rsplit('/').next().unwrap_or(glob);
    if last.contains('*') {
        last.replace('*', "sample")
    } else {
        last.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::sample_for_glob;

    #[test]
    fn glob_samples_become_concrete_filenames() {
        assert_eq!(sample_for_glob("**/*.kdemo"), "sample.kdemo");
        assert_eq!(sample_for_glob("*.mock"), "sample.mock");
        assert_eq!(sample_for_glob("**/BUILD"), "BUILD");
        assert_eq!(sample_for_glob("src/**/*.vue"), "sample.vue");
    }
}
