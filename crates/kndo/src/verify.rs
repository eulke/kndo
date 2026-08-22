//! `kndo plugin verify <component.wasm>` (RFC 0017 §7) — the public half of the compliance
//! suite, packaged for an author's inner loop. Loads the component through the exact loaders
//! `kndo::open` discovery uses, reports what its descriptor declares (plus lint-grade warnings
//! for the mistakes the docs warn about), then drives every hook for real: the component is
//! dropped project-local into a synthesized fixture project and a genuine `kndo::open` + full
//! check runs over it — the same code path a user's project would exercise, not a mock. What
//! the component contributed comes back from the RFC 0017 §7 audit record the run leaves in
//! the fixture's cache.
//!
//! Deliberately *not* a conformance judgment: `verify` cannot know what a component is
//! supposed to do, only whether it loads, what it declares, and what it observably did against
//! a generic fixture. Zero contributions is a note, not a failure — most convention plugins
//! contribute nothing to a project that doesn't match their conventions.

use std::path::Path;

use kndo_core::engine::{CheckRequest, ConfigOverrides, RunMode};

/// Which world (docs/contracts/wasm-abi.md §4/§5) accepted the component.
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

/// Verify one component file. `Err` = the component doesn't load under either world — the
/// combined per-world errors, same shape `plugin_install`'s probe reports.
pub fn verify(component: &Path) -> Result<VerifyReport, String> {
    match kndo_plugin_api::WasmPlugin::load(component) {
        Ok(plugin) => Ok(verify_plugin(component, &plugin)),
        Err(plugin_err) => match kndo_plugin_api::WasmAdapter::load(component) {
            Ok(adapter) => Ok(verify_adapter(component, &adapter)),
            Err(adapter_err) => Err(format!(
                "not a valid kndo:plugin ({plugin_err}) or kndo:adapter ({adapter_err}) \
                 component"
            )),
        },
    }
}

fn verify_plugin(component: &Path, plugin: &kndo_plugin_api::WasmPlugin) -> VerifyReport {
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

    let fixture = drive_fixture(component, &[], Some(d.id.as_str()));
    VerifyReport {
        kind: VerifiedKind::Plugin,
        id: d.id.to_string(),
        descriptor,
        warnings,
        fixture,
    }
}

fn verify_adapter(component: &Path, adapter: &kndo_plugin_api::WasmAdapter) -> VerifyReport {
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
    let fixture = drive_fixture(component, &samples, None);
    VerifyReport {
        kind: VerifiedKind::Adapter,
        id: d.id.to_string(),
        descriptor,
        warnings,
        fixture,
    }
}

/// The hook drive: a temp fixture project with the component dropped project-local
/// (unconditional activation, RFC 0003 §3), `sample_files` synthesized from the adapter's own
/// globs (empty for plugins), one real full check, then the audit record read back.
fn drive_fixture(
    component: &Path,
    sample_files: &[String],
    plugin_id: Option<&str>,
) -> Vec<String> {
    let dir = match fixture_dir() {
        Ok(d) => d,
        Err(e) => return vec![format!("fixture drive skipped: temp dir failed ({e})")],
    };
    let out = drive_fixture_in(dir.as_path(), component, sample_files, plugin_id);
    let _ = std::fs::remove_dir_all(&dir);
    out
}

fn drive_fixture_in(
    root: &Path,
    component: &Path,
    sample_files: &[String],
    plugin_id: Option<&str>,
) -> Vec<String> {
    if let Err(e) = stage_fixture(root, component, sample_files) {
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
        out.push(contribution_line(&engine, id));
    }
    out
}

/// The component into `.kndo/plugins/`, a generic manifest (so manifest-driven activation and
/// content reads have something real to see), and the adapter's own glob-derived samples.
fn stage_fixture(root: &Path, component: &Path, sample_files: &[String]) -> Result<(), String> {
    let plugins_dir = root.join(".kndo").join("plugins");
    std::fs::create_dir_all(&plugins_dir)
        .and_then(|()| std::fs::copy(component, plugins_dir.join("verify.wasm")))
        .map_err(|e| format!("staging the component failed ({e})"))?;
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

fn contribution_line(engine: &kndo_core::engine::Engine, id: &str) -> String {
    match engine
        .doctor()
        .plugin_contributions
        .iter()
        .find(|c| c.id == id)
    {
        Some(c) => format!(
            "contributed {} root(s), {} edge(s), {} annotation(s) on the generic fixture \
             (zero is normal for a convention plugin the fixture doesn't match)",
            c.roots, c.edges, c.annotations
        ),
        None => "no contribution record — the plugin's graph-mutation hooks did not run \
                 (mutates graph: false, or the round was skipped)"
            .to_string(),
    }
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
