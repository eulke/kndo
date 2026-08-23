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

use kndo_core::engine::{CheckRequest, ConfigOverrides, RunMode};

/// Which ABI world accepted the component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedKind {
    Adapter,
    Plugin,
}

impl VerifiedKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VerifiedKind::Adapter => "kndo:adapter",
            VerifiedKind::Plugin => "kndo:plugin",
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
pub fn verify(component: &Path) -> Result<VerifyReport, String> {
    verify_impl(component, None)
}

/// Verify against the author's own fixture project instead of the synthesized one: `project`
/// is copied whole into a temp root (never mutated — `.git`/`.kndo`/`target`/`node_modules`
/// excluded), the component staged project-local there, and the same full check runs. This is
/// the assertion half of the baseline-then-plugin authoring loop as one command.
pub fn verify_in_project(component: &Path, project: &Path) -> Result<VerifyReport, String> {
    if !project.is_dir() {
        return Err(format!(
            "--project {} is not a directory",
            project.display()
        ));
    }
    verify_impl(component, Some(project))
}

fn verify_impl(component: &Path, project: Option<&Path>) -> Result<VerifyReport, String> {
    match kndo_plugin_api::WasmPlugin::load(component) {
        Ok(plugin) => Ok(verify_plugin(component, &plugin, project)),
        Err(plugin_err) => match kndo_plugin_api::WasmAdapter::load(component) {
            Ok(adapter) => Ok(verify_adapter(component, &adapter, project)),
            Err(adapter_err) => Err(format!(
                "not a valid kndo:plugin ({plugin_err}) or kndo:adapter ({adapter_err}) \
                 component"
            )),
        },
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
    let dir = match fixture_dir() {
        Ok(d) => d,
        Err(e) => return vec![format!("fixture drive skipped: temp dir failed ({e})")],
    };
    let out = drive_fixture_in(dir.as_path(), component, sample_files, plugin_id, project);
    let _ = std::fs::remove_dir_all(&dir);
    out
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
    let mut engine = match crate::open(
        root,
        ConfigOverrides {
            use_cache: true,
            threads: Some(1),
        },
    ) {
        Ok(e) => e,
        Err(e) => return vec![format!("fixture drive failed: kndo::open ({e})")],
    };
    let result = engine.check(CheckRequest {
        mode: RunMode::Full,
    });
    let mut out = run_report(&result, sample_files);
    if let Some(id) = plugin_id {
        out.extend(contribution_lines(&engine, id));
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
) -> Result<(), String> {
    match project {
        Some(source) => copy_tree(source, root)?,
        None => synthesize_fixture(root, sample_files)?,
    }
    stage_component(root, component)
}

/// The component into `.kndo/plugins/` — the tier where activation is unconditional.
fn stage_component(root: &Path, component: &Path) -> Result<(), String> {
    let plugins_dir = root.join(".kndo").join("plugins");
    std::fs::create_dir_all(&plugins_dir)
        .and_then(|()| std::fs::copy(component, plugins_dir.join("verify.wasm")))
        .map(|_| ())
        .map_err(|e| format!("staging the component failed ({e})"))
}

/// The generic fixture: a manifest (so manifest-driven activation and content reads have
/// something real to see) plus the adapter's own glob-derived samples.
fn synthesize_fixture(root: &Path, sample_files: &[String]) -> Result<(), String> {
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
fn copy_tree(source: &Path, dest: &Path) -> Result<(), String> {
    let entries =
        std::fs::read_dir(source).map_err(|e| format!("reading {}: {e}", source.display()))?;
    for entry in entries {
        copy_dir_entry(entry, dest)?;
    }
    Ok(())
}

fn copy_dir_entry(entry: std::io::Result<std::fs::DirEntry>, dest: &Path) -> Result<(), String> {
    let entry = entry.map_err(|e| e.to_string())?;
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

fn copy_entry(entry: &std::fs::DirEntry, to: &Path) -> Result<(), String> {
    let from = entry.path();
    let file_type = entry.file_type().map_err(|e| e.to_string())?;
    if file_type.is_dir() {
        std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
        copy_tree(&from, to)
    } else if file_type.is_file() {
        std::fs::copy(&from, to)
            .map(|_| ())
            .map_err(|e| format!("copying {}: {e}", from.display()))
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

fn contribution_lines(engine: &kndo_core::engine::Engine, id: &str) -> Vec<String> {
    let Some(c) = engine
        .doctor()
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

/// Per-call-unique fixture root under the system temp dir — same reasoning (and shape) as
/// `plugin_install`'s own probe dir: `verify` is a plain function safe to call concurrently.
fn fixture_dir() -> std::io::Result<std::path::PathBuf> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("kndo-verify-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
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
