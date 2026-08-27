//! kndo CLI — a frontend over the `kndo` distribution crate, nothing more.
//!
//! Pure presentation: argument parsing, exit codes, human rendering. Which
//! languages exist is the distribution crate's knowledge — this binary never
//! names one. If code here needs a graph fact, that is a core PR adding it to `RunResult`,
//! never a deeper import.

use std::io::IsTerminal;
use std::process::ExitCode;

use kndo::{
    BaselineOp, BaselineResult, ConfigOverrides, Engine, RunMode, Severity, SCHEMA_VERSION,
};

mod nav;
mod render;

const USAGE: &str = "kndo — find what your codebase no longer needs

usage: kndo [command] [flags]

commands
  check            analyze the project (the default: bare `kndo` = `kndo check`)
  health           health score with per-category breakdown (--by-package)
  baseline         acknowledge current findings (.kndo/baseline.json; --update to refresh)
  doctor           what kndo sees: adapters, cache, plugins, config
  plugin           install | list | remove | new | build | wit | verify
  init             write kndo.toml (--hook also installs the pre-commit hook)
  find|describe|uses|used-by|trace|impact   graph navigation verbs (JSON envelopes)
  query            batched navigation requests from stdin (one JSON per line)

check flags
  --staged         analyze what `git commit` would commit, vs HEAD
  --diff <ref>     analyze the change vs merge-base(<ref>, HEAD)
  --fail-on <sev>  exit 1 at/above: error | warning (diff default) | info | none (full default)
  --format <f>     human (tty default) | json (piped default) | agent | sarif
  --quiet | --verbose | --no-cache | --threads <n> | --color <auto|always|never>

kndo --version   ·   full docs: docs/ in the repository
";

fn main() -> ExitCode {
    // The Rust runtime starts every process with SIGPIPE ignored, so a write to a pipe whose
    // reader already exited surfaces as an EPIPE error — which `println!` turns into a panic
    // with a backtrace on stderr. kndo's output is *designed* to be piped (`kndo check | jq`,
    // `kndo doctor | grep`, the json-when-piped default), so the conventional Unix
    // filter behavior is the correct one: restore the default disposition and let the process
    // die silently with signal 13 (exit 141 in a shell) the way grep, cat, and git do.
    // Windows has no SIGPIPE; writes there keep their normal error path.
    #[cfg(unix)]
    // SAFETY: `signal` with SIG_DFL only resets a handler, before any other thread exists.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    // `--help` anywhere wins over everything else (asking for help must never
    // trigger an analysis run, whatever else is on the line).
    if args.iter().any(|a| a == "--help" || a == "-h")
        || args.first().map(String::as_str) == Some("help")
    {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!(
                "kndo {} (schema {})",
                env!("CARGO_PKG_VERSION"),
                SCHEMA_VERSION
            );
            ExitCode::SUCCESS
        }
        Some("check") => check(&args[1..]),
        Some("baseline") => baseline_cmd(&args[1..]),
        Some("doctor") => doctor_cmd(),
        Some("plugin") => plugin_cmd(&args[1..]),
        Some("health") => health_cmd(&args[1..]),
        Some("init") => init_cmd(&args[1..]),
        Some("find") => nav::find_cmd(&args[1..]),
        Some("describe") => nav::describe_cmd(&args[1..]),
        Some("uses") => nav::uses_cmd(&args[1..]),
        Some("used-by") => nav::used_by_cmd(&args[1..]),
        Some("trace") => nav::trace_cmd(&args[1..]),
        Some("impact") => nav::impact_cmd(&args[1..]),
        Some("query") => nav::query_cmd(),
        // Bare flags with no subcommand (`kndo --format json`) are an implicit `check`, same
        // as no arguments at all — `kndo` = `kndo check`.
        Some(s) if s.starts_with('-') => check(&args),
        None => check(&args),
        Some(other) => {
            eprintln!(
                "kndo: unknown command `{other}` (check, health, baseline, doctor, plugin, init, find, describe, uses, used-by, trace, impact, query, help, --version)"
            );
            ExitCode::from(2)
        }
    }
}

const KNDO_TOML_TEMPLATE: &str = r#"# kndo.toml — everything here is optional; every setting already has the default shown.
# Written by `kndo init`. Full reference: docs/ in the repository.

# [project]
# roots = ["src", "packages/*"]          # default: auto (git ls-files minus ignores)
# exclude = ["**/generated/**"]

# [analysis]
# skip = []                              # categories or category:subject, e.g. ["unused:enum-member"]
# min-confidence = "possible"            # report floor; raise to "probable" to hide the
#                                        # speculative tier (--verbose always shows everything)

# [analysis.duplicate]
# min-tokens = 50

# [analysis.crap]
# threshold = 30

# [performance]
# threads = 0                            # 0 = physical cores; --threads flag wins

# [delta]                                # diff-mode gate budgets
# max-health-drop = 0.0
# max-net-findings = 0

# [[rule]]                               # per-path overrides
# paths = ["examples/**"]
# skip = ["unused"]

# [plugins.gate]                         # opt plugin findings into the exit-code gate
# "github.com/acme/some-plugin" = "warning"        # gate this plugin's rules, capped at warning
# "github.com/acme/some-plugin/noisy-rule" = "off" # per-rule override wins

# [plugins.coverage-lcov]                # per-plugin options (coverage ingesters)
# report = "coverage/lcov.info"          # replaces the well-known paths; globs allowed
# max-age = "7d"                         # freshness override ("12h" and bare days work too)
"#;

const PRE_COMMIT_HOOK: &str = "#!/bin/sh\nexec kndo check --staged --fail-on warning\n";

/// `kndo init`: write a minimal kndo.toml and a .gitignore entry, and offer the pre-commit
/// hook. Deliberately not an `Engine` method — the `Engine` trait doesn't list
/// `init` alongside `check`/`baseline`/`doctor`, and this command does no analysis at all, just
/// project scaffolding, so there's nothing for the analysis facade to own.
///
/// "Offer" is read literally: writing directly into `.git/hooks/pre-commit` unprompted could
/// silently clobber an existing hook (or a hook manager's own file) — a hard-to-reverse,
/// surprising action for a tool to take on its own. Default behavior only *prints* the
/// recommended hook and how to install it; `--hook` opts into actually writing it, and even
/// then only when `.git/hooks/pre-commit` doesn't already exist.
fn init_cmd(args: &[String]) -> ExitCode {
    let install_hook = args.iter().any(|a| a == "--hook");
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("kndo: cannot determine working directory: {e}");
            return ExitCode::from(2);
        }
    };

    let toml_path = cwd.join("kndo.toml");
    if toml_path.is_file() {
        println!("kndo.toml: already exists, left untouched");
    } else {
        match std::fs::write(&toml_path, KNDO_TOML_TEMPLATE) {
            Ok(()) => println!("kndo.toml: written"),
            Err(e) => {
                eprintln!("kndo: failed to write kndo.toml: {e}");
                return ExitCode::from(2);
            }
        }
    }

    match ensure_gitignore_entry(&cwd) {
        Ok(GitignoreOutcome::AlreadyPresent) => println!(".gitignore: .kndo/ already present"),
        Ok(GitignoreOutcome::Appended) => println!(".gitignore: added .kndo/"),
        Err(e) => {
            eprintln!("kndo: failed to update .gitignore: {e}");
            return ExitCode::from(2);
        }
    }

    let hook_path = cwd.join(".git/hooks/pre-commit");
    if !install_hook {
        println!("pre-commit hook: not installed (recommended — install with `kndo init --hook`, or add manually):");
        println!("  {}", PRE_COMMIT_HOOK.lines().last().unwrap());
    } else if !cwd.join(".git").is_dir() {
        println!(
            "pre-commit hook: skipped — {} is not a git repository",
            cwd.display()
        );
    } else if hook_path.is_file() {
        eprintln!(
            "kndo: .git/hooks/pre-commit already exists — refusing to overwrite it; add this line yourself:"
        );
        eprintln!("  {}", PRE_COMMIT_HOOK.lines().last().unwrap());
        return ExitCode::from(2);
    } else {
        if let Some(parent) = hook_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("kndo: failed to create .git/hooks: {e}");
                return ExitCode::from(2);
            }
        }
        if let Err(e) = std::fs::write(&hook_path, PRE_COMMIT_HOOK) {
            eprintln!("kndo: failed to write .git/hooks/pre-commit: {e}");
            return ExitCode::from(2);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&hook_path) {
                let mut perms = meta.permissions();
                perms.set_mode(perms.mode() | 0o111);
                let _ = std::fs::set_permissions(&hook_path, perms);
            }
        }
        println!("pre-commit hook: installed at .git/hooks/pre-commit");
    }

    ExitCode::SUCCESS
}

enum GitignoreOutcome {
    AlreadyPresent,
    Appended,
}

/// Idempotent: only appends `.kndo/` when no line already matches it exactly, and creates the
/// file if the project has none yet.
fn ensure_gitignore_entry(root: &std::path::Path) -> std::io::Result<GitignoreOutcome> {
    let path = root.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == ".kndo/") {
        return Ok(GitignoreOutcome::AlreadyPresent);
    }
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(".kndo/\n");
    std::fs::write(&path, updated)?;
    Ok(GitignoreOutcome::Appended)
}

/// `kndo doctor`: plain-text only — the output schema specifies no
/// JSON shape for this command, so `--format` isn't wired here (a deliberate scoping choice,
/// not an oversight; `check`/navigation verbs are where the JSON contract matters).
fn doctor_cmd() -> ExitCode {
    let (cwd, engine) = match open_engine(base_config_overrides()) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let report = engine.doctor();

    println!("project root: {}", report.project_root);
    println!();
    println!("adapters:");
    if report.adapters.is_empty() {
        println!("  (none registered)");
    }
    for a in &report.adapters {
        println!("  {}  (grammar {})", a.id, a.grammar_version);
        println!("    files:     {}", a.file_globs.join(", "));
        println!("    manifests: {}", a.manifest_globs.join(", "));
        if !a.activation.is_empty() {
            println!("    activation: {}", a.activation.join(", "));
        }
        if !a.dependencies.is_empty() {
            println!("    dependencies: {}", a.dependencies.join(", "));
        }
    }
    // The global adapter tier — every `.wasm` candidate the global directory holds,
    // activated or not, same split `plugin_resolution`'s own section below has (`Engine` never
    // sees a candidate that didn't activate). Reason-aware: a global adapter
    // can activate as another component's dependency, not just by its own rules.
    let adapter_resolution = kndo::adapter_resolution(&cwd);
    let global_adapters = kndo::global_adapter_candidates(&cwd);
    if !global_adapters.is_empty() {
        println!();
        println!("global adapter candidates (not necessarily active above):");
        for c in &global_adapters {
            println!("  {} — {}", c.id, activation_status(&c.active));
            if !c.activation.is_empty() {
                println!("    activation: {}", c.activation.join(", "));
            } else {
                println!("    activation: (none declared — never self-activates globally)");
            }
        }
    }
    if !adapter_resolution.missing_dependencies.is_empty() {
        println!();
        println!("missing adapter dependencies (declared by an active adapter, not present):");
        for m in &adapter_resolution.missing_dependencies {
            println!("  {} — required by {}", m.coordinate, m.required_by);
        }
    }
    println!();
    println!("plugins:");
    if report.plugins.is_empty() {
        println!("  (none registered)");
    }
    for p in &report.plugins {
        println!("  {} v{}", p.id, p.version);
        // Every plugin in this section is active, so the rule that FIRED is the answer.
        println!("    active:       {}", p.activated_by);
        if !p.detection.is_empty() {
            println!("    detection:    {}", p.detection.join(", "));
        }
        // The full gate, only where it says something the line above doesn't: with a single
        // declared rule the two are the same string, and printing it twice reads as two facts.
        if p.activation.len() > 1 {
            println!("    activation:   {}", p.activation.join(", "));
        }
        if !p.dependencies.is_empty() {
            println!("    dependencies: {}", p.dependencies.join(", "));
        }
        if !p.requested_file_access.is_empty() {
            println!("    file access:  {}", p.requested_file_access.join(", "));
        }
        // What this plugin MAY assert as findings, before it ever runs.
        for rule in &p.rules {
            println!("    rule: {rule}");
        }
    }
    // The composition layer's own view: every candidate considered — including
    // global ones that did NOT activate, which `Engine` structurally never sees — plus any
    // dependency coordinate an active plugin names that nothing present satisfies.
    let resolution = kndo::plugin_resolution(&cwd);
    let global_candidates: Vec<_> = resolution
        .plugins
        .iter()
        .filter(|p| p.source == kndo::PluginSource::Global)
        .collect();
    if !global_candidates.is_empty() {
        println!();
        println!("global plugin candidates (not necessarily active above):");
        for c in &global_candidates {
            println!(
                "  {} v{} — {}",
                c.id,
                c.version,
                activation_status(&c.active)
            );
            if !c.activation.is_empty() {
                println!("    activation: {}", c.activation.join(", "));
            } else {
                println!("    activation: (none declared — never self-activates globally)");
            }
        }
    }
    if !resolution.missing_dependencies.is_empty() {
        println!();
        println!("missing plugin dependencies (declared by an active plugin, not present):");
        for m in &resolution.missing_dependencies {
            println!("  {} — required by {}", m.coordinate, m.required_by);
        }
    }
    // The audit record: what each plugin actually asserted into the graph on the
    // last run that ran the plugin round — the observable half of the threat model.
    if !report.plugin_contributions.is_empty() {
        println!();
        println!("plugin contributions (last recorded run):");
        for c in &report.plugin_contributions {
            println!(
                "  {} — {} roots, {} edges, {} annotations",
                c.id, c.roots, c.edges, c.annotations
            );
            if !c.dropped.is_empty() {
                println!(
                    "    {} contribution(s) dropped (unresolved targets — kndo plugin \
                     verify lists them)",
                    c.dropped.len()
                );
            }
        }
    }
    println!();
    println!(
        "cache: {}",
        if report.cache_enabled {
            "enabled"
        } else {
            "disabled (--no-cache)"
        }
    );
    if let Some(c) = &report.cache {
        println!("  writable:       {}", c.writable);
        println!(
            "  facts entries:  {} ({} bytes)",
            c.facts_entries, c.facts_bytes
        );
        println!(
            "  graph snapshots: {}",
            if c.graph_snapshots > 0 {
                format!("{} ({} bytes)", c.graph_snapshots, c.graph_snapshot_bytes)
            } else {
                "none".to_string()
            }
        );
    }
    println!();
    if report.baseline_present {
        println!("baseline: present ({} entries)", report.baseline_entries);
    } else {
        println!("baseline: absent (kndo baseline to create one)");
    }

    ExitCode::SUCCESS
}

/// One line of doctor status for a global candidate (adapter or plugin — the
/// vocabulary is shared): why it's running, or the plain fact that it isn't.
fn activation_status(active: &Option<kndo::ActivationReason>) -> String {
    // The reason renders itself (`ActivationReason`'s `Display`) — the same text the JSON
    // envelope's `run.plugins[].activated_by` carries. A second spelling here is how doctor
    // and the envelope would come to disagree about why the same plugin is running.
    match active {
        Some(reason) => format!("active ({reason})"),
        None => "inactive".to_string(),
    }
}

/// `kndo plugin install <coordinate>[@tag] | list | remove <coordinate>` —
/// pure presentation over `kndo::plugin_install`; every policy (checksum, identity binding,
/// dependency closure, conflicts, lockfile) lives there.
fn plugin_cmd(args: &[String]) -> ExitCode {
    let sub = args.first().map(String::as_str).unwrap_or("");
    let rest = args.get(1..).unwrap_or(&[]);
    if matches!(sub, "install" | "list" | "remove") {
        plugin_registry_cmd(sub, rest)
    } else {
        plugin_author_cmd(sub, rest)
    }
}

fn plugin_registry_cmd(sub: &str, rest: &[String]) -> ExitCode {
    match (sub, rest.first()) {
        ("install", Some(spec)) => plugin_install(spec),
        ("remove", Some(spec)) => plugin_remove(spec),
        ("list", None) => plugin_list(),
        _ => plugin_usage(),
    }
}

/// The author-kit half of `kndo plugin`: `new`/`build` scaffold and produce a
/// component, `wit`/`verify` inspect the contract and the result.
fn plugin_author_cmd(sub: &str, rest: &[String]) -> ExitCode {
    match sub {
        "new" => plugin_new(rest),
        "build" => plugin_build(rest),
        _ => plugin_inspect_cmd(sub, rest),
    }
}

fn plugin_inspect_cmd(sub: &str, rest: &[String]) -> ExitCode {
    match sub {
        "wit" => plugin_wit(rest),
        "verify" => plugin_verify(rest),
        _ => plugin_usage(),
    }
}

fn plugin_usage() -> ExitCode {
    eprintln!(
        "kndo: usage: kndo plugin install <github.com/owner/repo[@tag]> | list | remove \
         <coordinate> | new <dir> [--adapter] | build [dir] | wit [plugin|adapter] | verify \
         <component.wasm> [--project <dir>]"
    );
    ExitCode::from(2)
}

/// `kndo plugin new <dir> [--adapter]`: scaffold a component crate with the ABI vendored.
fn plugin_new(rest: &[String]) -> ExitCode {
    let Some(dir) = rest.first() else {
        return plugin_usage();
    };
    let kind = if rest.iter().any(|a| a == "--adapter") {
        kndo::author::ComponentKind::Adapter
    } else {
        kndo::author::ComponentKind::Plugin
    };
    match kndo::author::scaffold(std::path::Path::new(dir), kind) {
        Ok(created) => {
            for rel in &created {
                println!("created {dir}/{rel}");
            }
            println!();
            println!("next: cd {dir} && kndo plugin build");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("kndo: plugin new: {e}");
            ExitCode::from(2)
        }
    }
}

/// `kndo plugin build [dir]`: cargo build + componentize, no wasm tooling knowledge needed.
fn plugin_build(rest: &[String]) -> ExitCode {
    let dir = rest.first().map(String::as_str).unwrap_or(".");
    match kndo::author::build(std::path::Path::new(dir)) {
        Ok(artifact) => {
            println!("built {}", artifact.display());
            println!("next: kndo plugin verify {}", artifact.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("kndo: plugin build: {e}");
            ExitCode::from(2)
        }
    }
}

/// `kndo plugin wit [plugin|adapter]`: print the WIT world this binary was built against —
/// pipe it into `wit/` to retarget an existing crate at this kndo version.
fn plugin_wit(rest: &[String]) -> ExitCode {
    let kind = match rest.first().map(String::as_str) {
        None | Some("plugin") => kndo::author::ComponentKind::Plugin,
        Some("adapter") => kndo::author::ComponentKind::Adapter,
        Some(other) => {
            eprintln!("kndo: plugin wit: unknown world `{other}` (plugin, adapter)");
            return ExitCode::from(2);
        }
    };
    print!("{}", kind.wit());
    ExitCode::SUCCESS
}

/// `kndo plugin verify <component.wasm> [--project <dir>]`: pure presentation
/// over `kndo::verify` — load, descriptor report, warnings, and a real fixture drive
/// (synthesized, or the author's own fixture with `--project`).
fn plugin_verify(rest: &[String]) -> ExitCode {
    let Some((path, project)) = parse_verify_args(rest) else {
        return plugin_usage();
    };
    match run_verify(path, project) {
        Ok(report) => {
            print_verify_report(path, &report);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("kndo: plugin verify: {e}");
            ExitCode::from(2)
        }
    }
}

/// `<component.wasm> [--project <dir>]` — `None` on any other shape.
fn parse_verify_args(rest: &[String]) -> Option<(&str, Option<&str>)> {
    let path = rest.first()?;
    match rest.get(1..).unwrap_or(&[]) {
        [] => Some((path, None)),
        [flag, dir] if flag == "--project" => Some((path, Some(dir))),
        _ => None,
    }
}

fn run_verify(path: &str, project: Option<&str>) -> Result<kndo::verify::VerifyReport, String> {
    match project {
        Some(dir) => {
            kndo::verify::verify_in_project(std::path::Path::new(path), std::path::Path::new(dir))
        }
        None => kndo::verify::verify(std::path::Path::new(path)),
    }
}

fn print_verify_report(path: &str, report: &kndo::verify::VerifyReport) {
    println!("{path}: loads as a {} component", report.kind.as_str());
    print_report_section("descriptor", &report.descriptor);
    if !report.warnings.is_empty() {
        print_report_section("warnings", &report.warnings);
    }
    print_report_section("fixture drive", &report.fixture);
}

fn print_report_section(header: &str, lines: &[String]) {
    println!();
    println!("{header}:");
    for line in lines {
        println!("  {line}");
    }
}

fn plugin_install(spec: &str) -> ExitCode {
    match kndo::plugin_install::install(spec) {
        Ok(report) => {
            print_install_report(&report);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("kndo: plugin install: {e}");
            ExitCode::from(2)
        }
    }
}

fn print_install_report(report: &kndo::plugin_install::InstallReport) {
    for (id, tag) in &report.installed {
        println!("installed {id} {tag}");
    }
    for id in &report.already_present {
        println!("{id}: already installed (no-op)");
    }
    for id in &report.builtin_deps {
        println!("dependency {id}: built into this kndo (no-op)");
    }
    for id in &report.unknown_builtins {
        println!(
            "warning: dependency {id} is not built into this kndo — those conventions \
             won't be analyzed (kndo doctor will keep reporting the gap)"
        );
    }
}

fn plugin_list() -> ExitCode {
    match kndo::plugin_install::list() {
        Ok((dir, managed, unmanaged)) => {
            println!("global plugin directory: {}", dir.display());
            print_plugin_rows(&managed, &unmanaged);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("kndo: plugin list: {e}");
            ExitCode::from(2)
        }
    }
}

fn print_plugin_rows(managed: &[kndo::plugin_install::InstalledPlugin], unmanaged: &[String]) {
    if managed.is_empty() && unmanaged.is_empty() {
        println!("no plugins installed (kndo plugin install <github.com/owner/repo>)");
    }
    for p in managed {
        println!("{} {} ({})", p.id, p.version, p.file);
    }
    for file in unmanaged {
        println!("{file}: hand-installed (not managed by kndo plugin install)");
    }
}

fn plugin_remove(spec: &str) -> ExitCode {
    match kndo::plugin_install::remove(spec) {
        Ok(version) => {
            println!("removed {spec} (was {version})");
            println!(
                "note: anything still depending on it will show as a missing dependency in \
                 kndo doctor — never an error"
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("kndo: plugin remove: {e}");
            ExitCode::from(2)
        }
    }
}

/// `kndo baseline [--update]` (`Engine::baseline`): snapshot the
/// complete current finding set into `.kndo/baseline.json` (committed — a human reviews the
/// diff). Without `--update`, refuses to overwrite an existing baseline: *every* baseline
/// write after the first needs that explicit flag, not just growth specifically, since a bare
/// re-run can't tell growth from shrinkage without diffing first; `--update` covers both cases
/// identically (a full snapshot replace), and fixed entries are auto-dropped by it.
/// All the actual file I/O lives behind `Engine::baseline` — this is
/// purely argument parsing and rendering the outcome, like every other command here.
fn baseline_cmd(args: &[String]) -> ExitCode {
    let op = if args.iter().any(|a| a == "--update") {
        BaselineOp::Update
    } else {
        BaselineOp::Create
    };

    let (_, mut engine) = match open_engine(base_config_overrides()) {
        Ok(t) => t,
        Err(code) => return code,
    };

    match engine.baseline(op) {
        BaselineResult::Written { acknowledged } => {
            println!(
                "kndo: baseline written — {acknowledged} findings acknowledged (.kndo/baseline.json)"
            );
            ExitCode::SUCCESS
        }
        BaselineResult::AlreadyExists => {
            eprintln!(
                "kndo: .kndo/baseline.json already exists — use `kndo baseline --update` to refresh it"
            );
            ExitCode::from(2)
        }
        BaselineResult::WriteFailed(e) => {
            eprintln!("kndo: failed to write .kndo/baseline.json: {e}");
            ExitCode::from(2)
        }
    }
}

#[derive(Debug)]
struct Flags {
    format: Option<String>,
    color: Option<String>,
    quiet: bool,
    verbose: bool,
    no_cache: bool,
    staged: bool,
    diff: Option<String>,
    fail_on: Option<String>,
    threads: Option<String>,
    by_package: bool,
}

fn parse_flags(args: &[String]) -> Result<Flags, String> {
    let mut flags = Flags {
        format: None,
        color: None,
        quiet: false,
        verbose: false,
        no_cache: false,
        staged: false,
        diff: None,
        fail_on: None,
        threads: None,
        by_package: false,
    };
    // A valued flag with no value, and any token kndo doesn't know, are hard errors:
    // a typo'd `--fail-onn warning` silently un-gating CI is
    // worse than any friction rejecting it costs.
    let value = |it: &mut std::slice::Iter<'_, String>, flag: &str| {
        it.next()
            .cloned()
            .ok_or_else(|| format!("{flag} needs a value — see `kndo --help`"))
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--format" => flags.format = Some(value(&mut it, "--format")?),
            "--color" => flags.color = Some(value(&mut it, "--color")?),
            "--quiet" => flags.quiet = true,
            "--verbose" => flags.verbose = true,
            "--no-cache" => flags.no_cache = true,
            "--staged" => flags.staged = true,
            "--diff" => flags.diff = Some(value(&mut it, "--diff")?),
            "--fail-on" => flags.fail_on = Some(value(&mut it, "--fail-on")?),
            "--threads" => flags.threads = Some(value(&mut it, "--threads")?),
            "--by-package" => flags.by_package = true,
            s if s.starts_with("--format=") => {
                flags.format = Some(s["--format=".len()..].to_string())
            }
            s if s.starts_with("--color=") => flags.color = Some(s["--color=".len()..].to_string()),
            s if s.starts_with("--diff=") => flags.diff = Some(s["--diff=".len()..].to_string()),
            s if s.starts_with("--fail-on=") => {
                flags.fail_on = Some(s["--fail-on=".len()..].to_string())
            }
            s if s.starts_with("--threads=") => {
                flags.threads = Some(s["--threads=".len()..].to_string())
            }
            other => {
                return Err(format!(
                    "unknown argument `{other}` — see `kndo --help` for flags"
                ))
            }
        }
    }
    Ok(flags)
}

/// `--threads N` > `KNDO_THREADS` env > default physical cores — resolved to a
/// concrete `Option<usize>` here (frontend argument-parsing concern) before it ever reaches
/// `ConfigOverrides`; `None` means "use the default," never "unspecified but pending." `0`
/// (either source) means the same thing explicitly: physical cores.
fn resolve_threads(explicit: Option<&str>, env: Option<&str>) -> Result<Option<usize>, String> {
    let raw = explicit.or(env);
    match raw {
        None => Ok(None),
        Some(s) => {
            let n: usize = s
                .parse()
                .map_err(|_| format!("--threads `{s}` is not a non-negative integer"))?;
            Ok(if n == 0 { None } else { Some(n) })
        }
    }
}

/// The `ConfigOverrides` every subcommand *without its own* `--threads` flag should open an
/// `Engine` with — still respects `KNDO_THREADS` (the env-level override applies
/// everywhere, not just `check`), just without a per-command CLI flag to parse. `check` builds
/// its own via `resolve_threads` instead, since it alone also accepts `--threads` explicitly.
/// A malformed `KNDO_THREADS` degrades to the default (physical cores) rather than failing the
/// command — surfacing a parse error for an env var on every unrelated subcommand would be more
/// surprising than just falling back.
fn base_config_overrides() -> ConfigOverrides {
    let env_threads = std::env::var("KNDO_THREADS").ok();
    let threads = resolve_threads(None, env_threads.as_deref()).unwrap_or(None);
    ConfigOverrides {
        threads,
        ..ConfigOverrides::default()
    }
}

/// The open-engine ritual every command repeats: find the project root, open an `Engine` with
/// the given overrides, and turn either failure into the same `kndo: <msg>` + exit-2 shape.
/// Returns the resolved cwd alongside the engine for the one caller (`doctor_cmd`) that still
/// needs it afterward — every other caller just discards it.
fn open_engine(overrides: ConfigOverrides) -> Result<(std::path::PathBuf, Engine), ExitCode> {
    let cwd = std::env::current_dir().map_err(|e| {
        eprintln!("kndo: cannot determine working directory: {e}");
        ExitCode::from(2)
    })?;
    let engine = kndo::open(&cwd, overrides).map_err(|e| {
        eprintln!("kndo: {e}");
        ExitCode::from(2)
    })?;
    Ok((cwd, engine))
}

/// `--staged` and `--diff <ref>` select `RunMode`; mutually exclusive, checked
/// here rather than left for the engine since "which mode" is entirely a frontend argument-
/// parsing concern. Both trees are fully analyzed and the engine reports the *difference*
/// (`Engine::run_diff`): `findings` carries what the change introduced — each tagged
/// `introduced` when it lands inside a touched file, `derived` when the change flipped it
/// elsewhere — and `fixed` carries what it removed. The `--fail-on` default (below) also
/// reacts to the mode.
fn resolve_mode(flags: &Flags) -> Result<RunMode, String> {
    match (flags.staged, &flags.diff) {
        (true, Some(_)) => Err("--staged and --diff are mutually exclusive".to_string()),
        (true, None) => Ok(RunMode::Staged),
        (false, Some(base)) => Ok(RunMode::Diff { base: base.clone() }),
        (false, None) => Ok(RunMode::Full),
    }
}

/// `--fail-on <severity>`: explicit flag wins; otherwise the default depends on
/// mode — `warning` in diff modes (a pre-commit gate should actually gate), `none` in full mode
/// (exploratory by default — a legacy repo's pre-existing findings shouldn't fail a plain
/// `kndo check`; day-one adoption must be safe). `None` return means "never fail on
/// findings"; `Some(sev)` means "fail if any finding is at least as severe as `sev`".
fn resolve_fail_on(explicit: Option<&str>, mode: &RunMode) -> Result<Option<Severity>, String> {
    let Some(raw) = explicit else {
        return Ok(mode.default_fail_on());
    };
    match raw.to_ascii_lowercase().as_str() {
        "none" => Ok(None),
        "error" => Ok(Some(Severity::Error)),
        "warning" => Ok(Some(Severity::Warning)),
        "info" => Ok(Some(Severity::Info)),
        other => Err(format!(
            "unknown --fail-on `{other}` (none, info, warning, error)"
        )),
    }
}

/// `--format` flag > `KNDO_FORMAT` env > TTY auto-detect (human on TTY, json when piped).
fn resolve_format(explicit: Option<&str>) -> String {
    if let Some(f) = explicit {
        return f.to_string();
    }
    if let Ok(env_format) = std::env::var("KNDO_FORMAT") {
        if !env_format.is_empty() {
            return env_format;
        }
    }
    if std::io::stdout().is_terminal() {
        "human".to_string()
    } else {
        "json".to_string()
    }
}

/// `NO_COLOR` always wins over `auto`; `--color always|never` overrides the TTY auto-detect.
fn resolve_color(explicit: Option<&str>) -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    match explicit {
        Some("always") => true,
        Some("never") => false,
        _ => std::io::stdout().is_terminal(),
    }
}

/// `kndo health`: the health score, per-category breakdown, and trend vs the
/// previous snapshot — a full-mode analysis presented health-first. `--by-package` adds the
/// per-package breakdown (same penalties grouped by package, never a different metric).
/// Never a gate: always exits 0 — budgets are the gating mechanism, configured through the
/// config file.
fn health_cmd(args: &[String]) -> ExitCode {
    let flags = match parse_flags(args) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
    };
    let format = resolve_format(flags.format.as_deref());
    let env_threads = std::env::var("KNDO_THREADS").ok();
    let threads = match resolve_threads(flags.threads.as_deref(), env_threads.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
    };
    let overrides = ConfigOverrides {
        use_cache: !flags.no_cache,
        threads,
        ..ConfigOverrides::default()
    };
    let (_, mut engine) = match open_engine(overrides) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let result = engine.check(RunMode::Full);
    render::diagnostics(&result.diagnostics);
    let Some(health) = &result.health else {
        eprintln!(
            "kndo: health unavailable - the project tree could not be analyzed (see diagnostics above)"
        );
        return ExitCode::from(2);
    };
    match format.as_str() {
        // The health object is the whole payload here — `kndo check --format json` carries
        // the full envelope; this command answers exactly one question.
        "json" => match serde_json::to_string_pretty(health) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("kndo: failed to serialize health: {e}");
                return ExitCode::from(2);
            }
        },
        "human" | "agent" => {
            let opts = render::RenderOptions {
                color: resolve_color(flags.color.as_deref()),
                quiet: flags.quiet,
                verbose: flags.verbose,
                by_package: flags.by_package,
            };
            print!("{}", render::render_health(health, &opts, true));
        }
        other => {
            eprintln!("kndo: unknown --format `{other}` (human, json, agent)");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

fn check(args: &[String]) -> ExitCode {
    let flags = match parse_flags(args) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
    };
    let format = resolve_format(flags.format.as_deref());
    let mode = match resolve_mode(&flags) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
    };
    let fail_on = match resolve_fail_on(flags.fail_on.as_deref(), &mode) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
    };
    let env_threads = std::env::var("KNDO_THREADS").ok();
    let threads = match resolve_threads(flags.threads.as_deref(), env_threads.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
    };

    let overrides = ConfigOverrides {
        use_cache: !flags.no_cache,
        threads,
        // `--verbose` reveals every tier even when the project config raises the
        // `min-confidence` floor; otherwise the file (or the report-everything default)
        // decides.
        min_confidence: flags.verbose.then(ConfigOverrides::verbose_min_confidence),
    };
    let (_, mut engine) = match open_engine(overrides) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let result = engine.check(mode);

    // Diagnostics degrade the run, they don't kill it: report on stderr and
    // continue — findings and diagnostics are not the same thing. stderr carries diagnostics
    // in every format; stdout stays the pure report, JSON included.
    render::diagnostics(&result.diagnostics);

    match format.as_str() {
        "json" => println!("{}", result.to_json()),
        "human" => {
            let opts = render::RenderOptions {
                color: resolve_color(flags.color.as_deref()),
                quiet: flags.quiet,
                verbose: flags.verbose,
                by_package: flags.by_package,
            };
            print!("{}", render::render(&result, &opts));
        }
        "agent" => println!("{}", result.to_agent_format()),
        "sarif" => println!("{}", result.to_sarif()),
        other => {
            eprintln!("kndo: unknown --format `{other}` (human, json, agent, sarif)");
            return ExitCode::from(2);
        }
    }

    if result
        .diagnostics
        .iter()
        .any(|d| d.level == kndo::DiagnosticLevel::Error)
    {
        // The run could not do what was asked (an error-level diagnostic): the
        // exit-2 tier — never let an analysis that didn't run read as a clean pass.
        return ExitCode::from(2);
    }
    if result.fails_at(fail_on) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo::Finding;

    #[test]
    fn gitignore_created_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            ensure_gitignore_entry(dir.path()).unwrap(),
            GitignoreOutcome::Appended
        ));
        let text = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert_eq!(text, ".kndo/\n");
    }

    #[test]
    fn gitignore_entry_appended_to_existing_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "target/").unwrap(); // no trailing newline
        assert!(matches!(
            ensure_gitignore_entry(dir.path()).unwrap(),
            GitignoreOutcome::Appended
        ));
        let text = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert_eq!(text, "target/\n.kndo/\n");
    }

    #[test]
    fn gitignore_entry_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "target/\n.kndo/\n").unwrap();
        assert!(matches!(
            ensure_gitignore_entry(dir.path()).unwrap(),
            GitignoreOutcome::AlreadyPresent
        ));
        let text = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert_eq!(text, "target/\n.kndo/\n"); // unchanged, not duplicated
    }

    fn flags(staged: bool, diff: Option<&str>, fail_on: Option<&str>) -> Flags {
        Flags {
            format: None,
            color: None,
            quiet: false,
            verbose: false,
            no_cache: false,
            staged,
            diff: diff.map(str::to_string),
            fail_on: fail_on.map(str::to_string),
            threads: None,
            by_package: false,
        }
    }

    #[test]
    fn parses_new_flags() {
        let args: Vec<String> = ["--staged", "--fail-on", "error"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let f = parse_flags(&args).unwrap();
        assert!(f.staged);
        assert_eq!(f.fail_on.as_deref(), Some("error"));

        let args: Vec<String> = ["--diff=main", "--fail-on=none"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let f = parse_flags(&args).unwrap();
        assert_eq!(f.diff.as_deref(), Some("main"));
        assert_eq!(f.fail_on.as_deref(), Some("none"));

        let args: Vec<String> = ["--threads", "4"].iter().map(|s| s.to_string()).collect();
        let f = parse_flags(&args).unwrap();
        assert_eq!(f.threads.as_deref(), Some("4"));

        let args: Vec<String> = ["--threads=1"].iter().map(|s| s.to_string()).collect();
        let f = parse_flags(&args).unwrap();
        assert_eq!(f.threads.as_deref(), Some("1"));
    }

    #[test]
    fn unknown_arguments_and_missing_values_are_rejected() {
        // A typo'd flag silently un-gating CI is worse than any friction.
        let args: Vec<String> = vec!["--fail-onn".into(), "warning".into()];
        assert!(parse_flags(&args).unwrap_err().contains("--fail-onn"));
        let args: Vec<String> = vec!["--diff".into()];
        assert!(parse_flags(&args).unwrap_err().contains("needs a value"));
        let args: Vec<String> = vec!["stray".into()];
        assert!(parse_flags(&args).unwrap_err().contains("stray"));
    }

    #[test]
    fn threads_explicit_flag_wins_over_env() {
        assert_eq!(resolve_threads(Some("4"), Some("8")).unwrap(), Some(4));
    }

    #[test]
    fn threads_falls_back_to_env_when_no_flag() {
        assert_eq!(resolve_threads(None, Some("2")).unwrap(), Some(2));
    }

    #[test]
    fn threads_zero_from_either_source_means_default_physical_cores() {
        assert_eq!(resolve_threads(Some("0"), None).unwrap(), None);
        assert_eq!(resolve_threads(None, Some("0")).unwrap(), None);
    }

    #[test]
    fn threads_absent_everywhere_is_the_default() {
        assert_eq!(resolve_threads(None, None).unwrap(), None);
    }

    #[test]
    fn threads_rejects_a_non_numeric_value() {
        assert!(resolve_threads(Some("bogus"), None).is_err());
    }

    #[test]
    fn staged_and_diff_are_mutually_exclusive() {
        assert!(resolve_mode(&flags(true, Some("main"), None)).is_err());
    }

    #[test]
    fn mode_resolves_from_flags() {
        assert_eq!(resolve_mode(&flags(false, None, None)), Ok(RunMode::Full));
        assert_eq!(resolve_mode(&flags(true, None, None)), Ok(RunMode::Staged));
        assert_eq!(
            resolve_mode(&flags(false, Some("main"), None)),
            Ok(RunMode::Diff {
                base: "main".to_string()
            })
        );
    }

    #[test]
    fn fail_on_defaults_differ_by_mode_but_an_explicit_flag_always_wins() {
        assert_eq!(resolve_fail_on(None, &RunMode::Full).unwrap(), None);
        assert_eq!(
            resolve_fail_on(None, &RunMode::Staged).unwrap(),
            Some(Severity::Warning)
        );
        assert_eq!(
            resolve_fail_on(
                None,
                &RunMode::Diff {
                    base: "main".to_string()
                }
            )
            .unwrap(),
            Some(Severity::Warning)
        );
        assert_eq!(
            resolve_fail_on(Some("error"), &RunMode::Full).unwrap(),
            Some(Severity::Error)
        );
        assert_eq!(
            resolve_fail_on(Some("none"), &RunMode::Staged).unwrap(),
            None
        );
        assert!(resolve_fail_on(Some("bogus"), &RunMode::Full).is_err());
    }

    fn finding(severity: Severity) -> Finding {
        Finding {
            advisory: false,
            id: "kndo-000000000000".to_string(),
            category: "unused".into(),
            group: kndo::Group::Waste,
            subject_kind: "symbol".into(),
            severity,
            confidence: kndo::Confidence::Certain,
            message: "example".to_string(),
            location: Default::default(),
            related: Vec::new(),
            rolled_up: None,
            delta: None,
            delta_origin: None,
        }
    }

    fn result_with(findings: Vec<Finding>) -> kndo::RunResult {
        kndo::RunResult {
            findings,
            ..Default::default()
        }
    }

    /// The gate's actual threshold logic (severity ranking, the advisory exemption) lives —
    /// and is unit-tested — core-side on `RunResult::fails_at`; this is the CLI's own half of
    /// the gate: turning that bool into the process exit code.
    #[test]
    fn fails_at_maps_to_the_exit_code() {
        let result = result_with(vec![finding(Severity::Warning)]);
        assert!(!result.fails_at(None));
        assert!(result.fails_at(Some(Severity::Warning)));
        assert!(!result_with(vec![]).fails_at(Some(Severity::Info)));
    }
}
