//! The `kndo` command: the first facade frontend. Everything here is presentation —
//! parsing the invocation, rendering a [`kndo::Report`], mapping [`kndo::RunOutcome`]
//! to an exit code — and every fact it prints comes from the facade, never derived
//! locally. The logic lives in this library so tests (and the dogfood's own
//! reachability) exercise it through imports; `main` stays one call deep.

use clap::{Parser, Subcommand, ValueEnum};
use kndo::{Categories, Config, GatePolicy, Mode, Report, RunMode, RunOutcome, Severity, Threads};
use std::path::PathBuf;

mod config;
mod git;

#[derive(Parser)]
#[command(
    name = "kndo",
    version,
    about = "Deterministic dead-code and health analysis"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Analyze a project and report its findings (the default)
    Check(CheckArgs),
    /// Accept the current findings as the baseline future runs diff against
    Baseline(RunArgs),
    /// Project health only — the same measurement `check` reports, as one block
    Health(RunArgs),
    /// Write the kndo.toml template (and, with --hook, a pre-commit gate)
    Init(InitArgs),
    /// What kndo sees here: composition, config, cache, baseline
    Doctor(InitPath),
    /// Search the graph for nodes by name (exact > prefix > substring)
    Find(QueryArgs),
    /// One node in full: reach, keepers preview, findings on it
    Describe(QueryArgs),
    /// What a node depends on: imports and referenced names, resolved
    Uses(QueryArgs),
    /// What keeps a node alive — the deletion question, with sites
    #[command(name = "used-by")]
    UsedBy(QueryArgs),
    /// Why is this alive: the shortest root-to-node path
    Trace(QueryArgs),
    /// What transitively depends on a node; --if-deleted simulates the removal
    Impact(QueryArgs),
    /// Everything behind one finding id: the finding and its subject described
    Explain(QueryArgs),
}

#[derive(clap::Args)]
struct QueryArgs {
    /// Selectors (`path`, `path#name`, `path#Owner.member`) — or, for `find`,
    /// search patterns. Each input gets its own result.
    #[arg(required = true)]
    inputs: Vec<String>,
    /// Project root (defaults to the current directory)
    #[arg(long)]
    root: Option<PathBuf>,
    /// Listing cap (default 50); elision is always reported
    #[arg(long)]
    limit: Option<u32>,
    /// find: keep only this kind (a symbol kind, or `file`)
    #[arg(long)]
    kind: Option<String>,
    /// find: keep only this reachability color
    #[arg(long, value_enum)]
    reach: Option<ReachArg>,
    /// trace: root set to trace from (default: production, then test, tooling)
    #[arg(long, value_enum)]
    roots: Option<RootsArg>,
    /// trace: directed form — shortest path from each input TO this node
    #[arg(long, value_name = "selector")]
    to: Option<String>,
    /// impact: also simulate the removal and report the reachability flips
    #[arg(long)]
    if_deleted: bool,
    /// json (piped default) or agent (terminal default)
    #[arg(long, value_enum)]
    format: Option<QueryFormat>,
}

#[derive(Debug, ValueEnum, Clone, Copy)]
enum RootsArg {
    Production,
    Test,
    Tooling,
}

impl RootsArg {
    fn into_core(self) -> kndo::query::RootSet {
        use kndo::query::RootSet as R;
        match self {
            RootsArg::Production => R::Production,
            RootsArg::Test => R::Test,
            RootsArg::Tooling => R::Tooling,
        }
    }
}

#[derive(Debug, ValueEnum, Clone, Copy)]
enum ReachArg {
    Production,
    TestOnly,
    ToolingOnly,
    Unreachable,
}

impl ReachArg {
    fn into_core(self) -> kndo::query::ReachColor {
        use kndo::query::ReachColor as C;
        match self {
            ReachArg::Production => C::Production,
            ReachArg::TestOnly => C::TestOnly,
            ReachArg::ToolingOnly => C::ToolingOnly,
            ReachArg::Unreachable => C::Unreachable,
        }
    }
}

/// The query renderings: the agent text IS the readable one, so a terminal gets
/// it and a pipe gets JSON; `human` earns a colored form only when someone
/// needs more than the agent grammar gives.
#[derive(Debug, ValueEnum, Clone, Copy)]
enum QueryFormat {
    Json,
    Agent,
}

#[derive(clap::Args)]
struct InitPath {
    /// Project root (defaults to the current directory)
    path: Option<PathBuf>,
}

#[derive(clap::Args)]
struct InitArgs {
    /// Project root (defaults to the current directory)
    path: Option<PathBuf>,
    /// Also install .git/hooks/pre-commit running `kndo check --staged`
    #[arg(long)]
    hook: bool,
}

#[derive(clap::Args)]
struct CheckArgs {
    #[command(flatten)]
    run: RunArgs,
    /// Report format; unset, KNDO_FORMAT decides, else human on a terminal and
    /// json when piped
    #[arg(long, value_enum)]
    format: Option<Format>,
    /// Analyze what `git commit` would commit, against HEAD
    #[arg(long, conflicts_with = "diff")]
    staged: bool,
    /// Analyze the worktree against merge-base(<ref>, HEAD)
    #[arg(long, value_name = "ref")]
    diff: Option<String>,
    /// Judge only these categories (comma-separated, repeatable)
    #[arg(
        long,
        value_delimiter = ',',
        value_name = "categories",
        conflicts_with = "skip"
    )]
    only: Vec<String>,
    /// Judge everything except these categories (comma-separated, repeatable)
    #[arg(long, value_delimiter = ',', value_name = "categories")]
    skip: Vec<String>,
    /// Human render: print the verdict line only
    #[arg(long, conflicts_with = "verbose")]
    quiet: bool,
    /// Human render: also print the run's phase timings
    #[arg(long)]
    verbose: bool,
    /// Color the human render (default: on a terminal, unless NO_COLOR)
    #[arg(long, value_enum, value_name = "when")]
    color: Option<ColorChoice>,
}

/// `--color` in the universal spelling. `auto` and absent are the same
/// judgment: color when stdout is a terminal and `NO_COLOR` is not set.
#[derive(Debug, ValueEnum, Clone, Copy)]
enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[derive(clap::Args)]
struct RunArgs {
    /// Project root (defaults to the current directory)
    path: Option<PathBuf>,
    /// Ignore and bypass the on-disk caches for this run
    #[arg(long)]
    no_cache: bool,
    /// Worker threads (defaults to all cores)
    #[arg(long)]
    threads: Option<usize>,
    /// Lowest severity that fails the run (default: warning)
    #[arg(long, value_enum)]
    fail_on: Option<FailOn>,
}

/// The renderings `check` offers. `human` is this frontend's presentation; the
/// other three are core's render contracts, byte-identical from any frontend.
#[derive(Debug, ValueEnum, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Format {
    Human,
    Json,
    Agent,
    Sarif,
}

#[derive(Debug, ValueEnum, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum FailOn {
    Error,
    Warning,
    Info,
    Never,
}

impl FailOn {
    fn policy(self) -> GatePolicy {
        let fail_on = match self {
            FailOn::Error => Some(Severity::Error),
            FailOn::Warning => Some(Severity::Warning),
            FailOn::Info => Some(Severity::Info),
            FailOn::Never => None,
        };
        GatePolicy { fail_on }
    }
}

/// One invocation, every outcome folded in: what to print where, and the process
/// exit code — 0 pass, 1 findings at or above the gate, 2 refused or unusable
/// invocation (clap's own convention for the latter).
pub struct CliOutput {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

/// What the host process knows beyond the arguments: the facts that decide the
/// default format. The binary reads them once at the edge; carrying them as data
/// keeps the library deterministic and every path testable. No `Default` on
/// purpose — there is no honest default terminal, so the caller states both.
pub struct Host {
    /// Whether stdout is a terminal — humans get text, pipes get json.
    pub tty: bool,
    /// The `KNDO_FORMAT` environment value, if set. The `--format` flag wins.
    pub format_env: Option<String>,
    /// `NO_COLOR` is present and non-empty (no-color.org) — suppresses color
    /// unless `--color always` explicitly asks.
    pub no_color: bool,
}

pub fn run_args<I, T>(args: I, host: Host) -> CliOutput
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => {
            let rendered = err.render().to_string();
            let code = err.exit_code();
            return if code == 0 {
                // --help and --version are successful conversations, not errors.
                CliOutput {
                    stdout: rendered,
                    stderr: String::new(),
                    code,
                }
            } else {
                CliOutput {
                    stdout: String::new(),
                    stderr: rendered,
                    code,
                }
            };
        }
    };
    run(cli, host)
}

pub fn run(cli: Cli, host: Host) -> CliOutput {
    let command = cli.command.unwrap_or(Command::Check(CheckArgs {
        run: RunArgs {
            path: None,
            no_cache: false,
            threads: None,
            fail_on: None,
        },
        format: None,
        staged: false,
        diff: None,
        only: Vec::new(),
        skip: Vec::new(),
        quiet: false,
        verbose: false,
        color: None,
    }));
    match command {
        Command::Check(args) => check(args, &host),
        Command::Baseline(args) => baseline(args),
        Command::Health(args) => health(args, &host),
        Command::Init(args) => init(args),
        Command::Doctor(args) => doctor(args),
        Command::Find(args) => query_verb(kndo::query::Verb::Find, args, &host),
        Command::Describe(args) => query_verb(kndo::query::Verb::Describe, args, &host),
        Command::Uses(args) => query_verb(kndo::query::Verb::Uses, args, &host),
        Command::UsedBy(args) => query_verb(kndo::query::Verb::UsedBy, args, &host),
        Command::Trace(args) => query_verb(kndo::query::Verb::Trace, args, &host),
        Command::Impact(args) => query_verb(kndo::query::Verb::Impact, args, &host),
        Command::Explain(args) => query_verb(kndo::query::Verb::Explain, args, &host),
    }
}

/// One door for all four verbs: analyze (cache-warm this is cheap), build the
/// Request, render the Response. Exit codes speak per-input truth: 0 every
/// input answered, 1 something was not found, 2 an input errored (ambiguity
/// included) — the worst individual status, so scripting semantics survive
/// batching inputs.
fn query_verb(verb: kndo::query::Verb, args: QueryArgs, host: &Host) -> CliOutput {
    let root = args.root.clone().unwrap_or_else(|| PathBuf::from("."));
    let run = RunArgs {
        path: None,
        no_cache: false,
        threads: None,
        fail_on: None,
    };
    let snapshot = match analyze_at(&root, &run, true, &Categories::All) {
        Ok(snapshot) => snapshot,
        Err(refusal) => return refused(refusal),
    };
    let request = kndo::query::Request {
        verb,
        inputs: args.inputs.clone(),
        options: kndo::query::Options {
            limit: args.limit,
            kind: args.kind.clone(),
            color: args.reach.map(ReachArg::into_core),
            roots: args.roots.map(RootsArg::into_core),
            if_deleted: args.if_deleted,
            to: args.to.clone(),
        },
    };
    let response = snapshot.query(&request);
    let mut code = response
        .results
        .iter()
        .map(|r| match r {
            kndo::query::Outcome::Ok { .. } => 0,
            kndo::query::Outcome::NotFound { .. } => 1,
            kndo::query::Outcome::Error { .. } => 2,
        })
        .max()
        .unwrap_or(0);
    // `trace` with no path is scripting truth: the node is NOT reachable that
    // way — exit 1, matching not-found's tier, without failing its siblings.
    if verb == kndo::query::Verb::Trace {
        for outcome in &response.results {
            if let kndo::query::Outcome::Ok {
                answer: kndo::query::Answer::Trace(t),
            } = outcome
                && t.path.is_none()
            {
                code = code.max(1);
            }
        }
    }
    let format = args.format.unwrap_or({
        if host.tty {
            QueryFormat::Agent
        } else {
            QueryFormat::Json
        }
    });
    let stdout = match format {
        QueryFormat::Agent => response.to_agent(),
        QueryFormat::Json => {
            let mut json = response.to_json();
            json.push('\n');
            json
        }
    };
    CliOutput {
        stdout,
        stderr: String::new(),
        code,
    }
}

/// Honest introspection, no analysis run: the composition the session would
/// use (WASM load failures included — an opted-in component never vanishes
/// silently), the config as parsed, and filesystem facts about cache and
/// baseline. Exit 2 only when the session itself cannot open.
fn doctor(args: InitPath) -> CliOutput {
    let root = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
    let session = match kndo::open(root.clone(), Config::default()) {
        Ok(session) => session,
        Err(refusal) => return refused(refusal),
    };
    let mut out = format!(
        "kndo {} · envelope {}
extensions:
",
        env!("CARGO_PKG_VERSION"),
        kndo::REPORT_SCHEMA
    );
    for spec in session.extensions() {
        let suffixes: Vec<&str> = spec.suffixes().iter().map(|s| s.as_str()).collect();
        out.push_str(&format!(
            "- {} v{}{}
",
            spec.coordinate(),
            spec.version(),
            if suffixes.is_empty() {
                String::new()
            } else {
                format!(" · suffixes {}", suffixes.join(","))
            }
        ));
    }
    for diagnostic in session.composition_diagnostics() {
        out.push_str(&format!(
            "- [warn] {}: {}
",
            diagnostic.path.as_str(),
            diagnostic.message
        ));
    }
    match config::load(&root) {
        Err(message) => out.push_str(&format!(
            "config: BROKEN — {message}
"
        )),
        Ok(file) if !root.join("kndo.toml").is_file() => {
            let _ = file;
            out.push_str(
                "config: no kndo.toml (`kndo init` writes one)
",
            );
        }
        Ok(file) => {
            let c = &file.check;
            let mut parts = Vec::new();
            if let Some(v) = c.fail_on.and_then(|f| f.to_possible_value()) {
                parts.push(format!("fail-on {}", v.get_name()));
            }
            if let Some(v) = c.format.and_then(|f| f.to_possible_value()) {
                parts.push(format!("format {}", v.get_name()));
            }
            if !c.only.is_empty() {
                parts.push(format!("only {}", c.only.join(",")));
            }
            if !c.skip.is_empty() {
                parts.push(format!("skip {}", c.skip.join(",")));
            }
            if parts.is_empty() {
                out.push_str(
                    "config: kndo.toml present (all defaults)
",
                );
            } else {
                out.push_str(&format!(
                    "config: kndo.toml · {}
",
                    parts.join(" · ")
                ));
            }
        }
    }
    let cache = root.join(".kndo/cache");
    if cache.is_dir() {
        let (files, bytes) = dir_size(&cache);
        out.push_str(&format!(
            "cache: .kndo/cache · {files} files · {bytes} bytes
"
        ));
    } else {
        out.push_str(
            "cache: none yet
",
        );
    }
    let baseline = root.join(".kndo/baseline.json");
    match std::fs::metadata(&baseline) {
        Ok(meta) => out.push_str(&format!(
            "baseline: .kndo/baseline.json · {} bytes
",
            meta.len()
        )),
        Err(_) => out.push_str(
            "baseline: none (`kndo baseline` accepts the current findings)
",
        ),
    }
    CliOutput {
        stdout: out,
        stderr: String::new(),
        code: 0,
    }
}

fn dir_size(dir: &std::path::Path) -> (u64, u64) {
    let mut files = 0;
    let mut bytes = 0;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (0, 0);
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            let (f, b) = dir_size(&path);
            files += f;
            bytes += b;
        } else if let Ok(meta) = entry.metadata() {
            files += 1;
            bytes += meta.len();
        }
    }
    (files, bytes)
}

/// Refuses to overwrite: an existing kndo.toml (or hook) is someone's work.
fn init(args: InitArgs) -> CliOutput {
    let root = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
    let path = root.join("kndo.toml");
    if path.exists() {
        return CliOutput {
            stdout: String::new(),
            stderr: "kndo: kndo.toml already exists — edit it instead\n".to_string(),
            code: 2,
        };
    }
    if let Err(e) = std::fs::write(&path, config::TEMPLATE) {
        return CliOutput {
            stdout: String::new(),
            stderr: format!("kndo: could not write kndo.toml: {e}\n"),
            code: 2,
        };
    }
    let mut stdout = "wrote kndo.toml\n".to_string();
    if args.hook {
        let hooks = root.join(".git/hooks");
        if !hooks.is_dir() {
            return CliOutput {
                stdout,
                stderr: "kndo: --hook needs a git repository (.git/hooks not found)\n".to_string(),
                code: 2,
            };
        }
        let hook_path = hooks.join("pre-commit");
        if hook_path.exists() {
            return CliOutput {
                stdout,
                stderr: "kndo: .git/hooks/pre-commit already exists — add `kndo check --staged` to it yourself\n"
                    .to_string(),
                code: 2,
            };
        }
        let script = "#!/bin/sh\n# Gate what this commit would commit.\nexec kndo check --staged\n";
        if let Err(e) = std::fs::write(&hook_path, script) {
            return CliOutput {
                stdout,
                stderr: format!("kndo: could not write the pre-commit hook: {e}\n"),
                code: 2,
            };
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o755));
        }
        stdout.push_str("wrote .git/hooks/pre-commit (kndo check --staged)\n");
    }
    CliOutput {
        stdout,
        stderr: String::new(),
        code: 0,
    }
}

/// The health block alone — a full analysis either way (health is derived from the
/// whole judgment), a terminal gets the line, a pipe gets the JSON object. Always
/// exit 0: health is measurement, not a gate.
fn health(args: RunArgs, host: &Host) -> CliOutput {
    let root = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
    let snapshot = match analyze_at(&root, &args, !args.no_cache, &Categories::All) {
        Ok(snapshot) => snapshot,
        Err(refusal) => return refused(refusal),
    };
    let report = snapshot.report();
    let stdout = match &report.health {
        None => {
            "health not measured — reachability abstained (run `kndo check` for why)\n".to_string()
        }
        Some(health) if host.tty => {
            let mut line = format!(
                "health {} · implicated {} of {}",
                health.score_text(),
                health.implicated,
                health.subjects
            );
            for c in &health.by_category {
                line.push_str(&format!(" · {} {}", c.category.as_str(), c.findings));
            }
            line.push('\n');
            line
        }
        Some(health) => {
            let mut json = serde_json::to_string_pretty(health).expect("health serializes");
            json.push('\n');
            json
        }
    };
    CliOutput {
        stdout,
        stderr: String::new(),
        code: 0,
    }
}

/// What one `check` invocation resolved to, from every source it may come from.
struct Effective {
    color: bool,
    format: Format,
    fail_on: FailOn,
    categories: Categories,
}

/// The ONE merge site — no second place ranks these sources: flag >
/// `KNDO_FORMAT` (format only; the environment has no opinion on gates or
/// selection) > `kndo.toml` > built-in default. A malformed environment value
/// cannot be a refusal — the invoker of a pipeline did not necessarily set it —
/// so it warns on stderr and falls through; a malformed kndo.toml DOES refuse,
/// upstream in [`config::load`], because the file is this project's own claim.
fn effective(
    args: &CheckArgs,
    host: &Host,
    file: &config::FileConfig,
) -> Result<(Effective, Option<String>), CliOutput> {
    let mut warning = None;
    let format = args
        .format
        .or_else(|| match host.format_env.as_deref() {
            None | Some("") => None,
            Some(raw) => match <Format as ValueEnum>::from_str(raw, true) {
                Ok(format) => Some(format),
                Err(_) => {
                    warning = Some(format!(
                        "kndo: unknown KNDO_FORMAT `{raw}` (human, json, agent, sarif) — falling through\n"
                    ));
                    None
                }
            },
        })
        .or(file.check.format)
        .unwrap_or(if host.tty { Format::Human } else { Format::Json });
    let fail_on = args
        .run
        .fail_on
        .or(file.check.fail_on)
        .unwrap_or(FailOn::Warning);
    let categories = if !args.only.is_empty() || !args.skip.is_empty() {
        selection_of(&args.only, &args.skip)?
    } else {
        if !file.check.only.is_empty() && !file.check.skip.is_empty() {
            return Err(CliOutput {
                stdout: String::new(),
                stderr:
                    "kndo: kndo.toml sets both `only` and `skip` — they are mutually exclusive\n"
                        .to_string(),
                code: 2,
            });
        }
        selection_of(&file.check.only, &file.check.skip)?
    };
    let color = match args.color {
        Some(ColorChoice::Always) => true,
        Some(ColorChoice::Never) => false,
        Some(ColorChoice::Auto) | None => host.tty && !host.no_color,
    };
    Ok((
        Effective {
            color,
            format,
            fail_on,
            categories,
        },
        warning,
    ))
}

/// The user's category narrowing, validated at the frontier: an unknown name is a
/// refused invocation, told apart from a category that judged nothing.
fn selection_of(only: &[String], skip: &[String]) -> Result<Categories, CliOutput> {
    let parse = |names: &[String]| -> Result<Vec<kndo::Category>, CliOutput> {
        names
            .iter()
            .map(|name| {
                kndo::Category::parse(name).ok_or_else(|| CliOutput {
                    stdout: String::new(),
                    stderr: format!(
                        "kndo: unknown category `{name}` (first-party: {})\n",
                        kndo::Category::FIRST_PARTY
                            .iter()
                            .map(|c| c.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    code: 2,
                })
            })
            .collect()
    };
    if !only.is_empty() {
        return Ok(Categories::Only(parse(only)?));
    }
    if !skip.is_empty() {
        return Ok(Categories::Skip(parse(skip)?));
    }
    Ok(Categories::All)
}

fn open_and_analyze(args: &RunArgs) -> Result<(kndo::Session, kndo::Snapshot), kndo::Refusal> {
    let root = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
    let snapshot = analyze_at(&root, args, !args.no_cache, &Categories::All)?;
    let config = Config {
        threads: match args.threads {
            Some(n) if n > 0 => Threads::Count(n),
            _ => Threads::Auto,
        },
        use_cache: !args.no_cache,
        categories: Categories::All,
    };
    let session = kndo::open(root, config)?;
    Ok((session, snapshot))
}

fn analyze_at(
    root: &std::path::Path,
    args: &RunArgs,
    use_cache: bool,
    categories: &Categories,
) -> Result<kndo::Snapshot, kndo::Refusal> {
    let config = Config {
        threads: match args.threads {
            Some(n) if n > 0 => Threads::Count(n),
            _ => Threads::Auto,
        },
        use_cache,
        categories: categories.clone(),
    };
    kndo::open(root.to_path_buf(), config)?.analyze(RunMode::Full)
}

/// A diff-mode run: two full analyses over two pinned trees, composed. The base
/// (and, for `--staged`, the index) is materialized by the git edge; scratch trees
/// run cache-off so nothing is written into them. The comparison then rides the
/// baseline mechanism — `Snapshot::against` documents why the baseline file never
/// participates in a tree-vs-tree split.
fn diff_snapshot(
    args: &RunArgs,
    comparison: git::Comparison,
    categories: &Categories,
) -> Result<kndo::Snapshot, CliOutput> {
    let root = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
    let mode = match comparison {
        git::Comparison::Staged => Mode::Staged,
        git::Comparison::Against(_) => Mode::Diff,
    };
    let base_tree = comparison.base_tree(&root).map_err(git_failed)?;
    let base = git::materialize(&root, &base_tree).map_err(git_failed)?;
    // Both sides judge the same categories, or the diff would report the
    // narrowing, not the change.
    let base_snapshot = analyze_at(&base.root, args, false, categories).map_err(refused)?;
    let mut snapshot = match comparison.current_tree(&root).map_err(git_failed)? {
        Some(index_tree) => {
            let current = git::materialize(&root, &index_tree).map_err(git_failed)?;
            analyze_at(&current.root, args, false, categories).map_err(refused)?
        }
        None => analyze_at(&root, args, !args.no_cache, categories).map_err(refused)?,
    };
    snapshot.against(&base_snapshot, mode);
    Ok(snapshot)
}

fn git_failed(message: String) -> CliOutput {
    CliOutput {
        stdout: String::new(),
        stderr: format!("kndo: git: {message}\n"),
        code: 2,
    }
}

fn refused(refusal: kndo::Refusal) -> CliOutput {
    let code = RunOutcome::Refused(refusal.clone()).exit_code();
    CliOutput {
        stdout: String::new(),
        stderr: format!("kndo: refused: {refusal}\n"),
        code,
    }
}

fn check(args: CheckArgs, host: &Host) -> CliOutput {
    let root = args.run.path.clone().unwrap_or_else(|| PathBuf::from("."));
    let file = match config::load(&root) {
        Ok(file) => file,
        Err(message) => {
            return CliOutput {
                stdout: String::new(),
                stderr: format!("kndo: {message}\n"),
                code: 2,
            };
        }
    };
    let (effective, warning) = match effective(&args, host, &file) {
        Ok(resolved) => resolved,
        Err(failure) => return failure,
    };
    let (format, fail_on, categories) = (effective.format, effective.fail_on, effective.categories);
    let comparison = if args.staged {
        Some(git::Comparison::Staged)
    } else {
        args.diff.clone().map(git::Comparison::Against)
    };
    let snapshot = match comparison {
        Some(comparison) => match diff_snapshot(&args.run, comparison, &categories) {
            Ok(snapshot) => snapshot,
            Err(failure) => return failure,
        },
        None => match analyze_at(&root, &args.run, !args.run.no_cache, &categories) {
            Ok(snapshot) => snapshot,
            Err(refusal) => return refused(refusal),
        },
    };
    let report = snapshot.report();
    let code = snapshot.gate(&fail_on.policy()).exit_code();
    let mut stderr = warning.unwrap_or_default();
    // The presentation flags shape the human render and nothing else — the
    // other formats are byte-pinned contracts, so a flag that cannot bind is
    // said out loud instead of silently ignored.
    if !matches!(format, Format::Human) {
        let given: Vec<&str> = [
            args.quiet.then_some("--quiet"),
            args.verbose.then_some("--verbose"),
            args.color.is_some().then_some("--color"),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !given.is_empty() {
            stderr.push_str(&format!(
                "kndo: {} shape{} the human render only — no effect here\n",
                given.join("/"),
                if given.len() == 1 { "s" } else { "" },
            ));
        }
    }
    let presentation = Presentation {
        color: effective.color,
        quiet: args.quiet,
        verbose: args.verbose,
    };
    // Text documents arrive newline-terminated from their renders; JSON values
    // are framed here.
    let stdout = match format {
        Format::Human => render_text(&report, &snapshot.timings, &presentation),
        Format::Json => {
            let mut json = report.to_json();
            json.push('\n');
            json
        }
        Format::Agent => report.to_agent(),
        Format::Sarif => {
            let mut sarif = report.to_sarif();
            sarif.push('\n');
            sarif
        }
    };
    CliOutput {
        stdout,
        stderr,
        code,
    }
}

fn baseline(args: RunArgs) -> CliOutput {
    let (session, snapshot) = match open_and_analyze(&args) {
        Ok(pair) => pair,
        Err(refusal) => return refused(refusal),
    };
    if let Err(err) = session.write_baseline(&snapshot) {
        return CliOutput {
            stdout: String::new(),
            stderr: format!("kndo: could not write the baseline: {err}\n"),
            code: 2,
        };
    }
    let accepted = snapshot.findings.len();
    CliOutput {
        stdout: format!(
            "baseline written: {accepted} finding{} accepted (.kndo/baseline.json)\n",
            plural(accepted)
        ),
        stderr: String::new(),
        code: 0,
    }
}

/// How the human render presents: semantic ANSI color (severity words, the
/// clean line, health-arrow DIRECTION — never an absolute score, which would
/// smuggle back the judgment bands health deliberately does not have), the
/// one-line `--quiet` contract, and the `--verbose` phases line rendered from
/// the timings that live beside the byte-pinned report.
struct Presentation {
    color: bool,
    quiet: bool,
    verbose: bool,
}

impl Presentation {
    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn severity(&self, severity: kndo::Severity) -> String {
        let code = match severity {
            kndo::Severity::Error => "31",
            kndo::Severity::Warning => "33",
            kndo::Severity::Info => "36",
        };
        self.paint(code, severity.as_str())
    }
}

/// The text report: findings in the facade's canonical order (already sorted by the
/// engine), then the verdict line, then what moved against the comparison — the
/// baseline in full mode, the base tree in the diff modes — then the run's own
/// honesty: abstentions, suppressions, diagnostics. `--quiet` keeps the verdict
/// line alone; the exit code already carries the gate.
fn render_text(report: &Report, timings: &kndo::PhaseTimings, view: &Presentation) -> String {
    let mut out = String::new();
    if !view.quiet {
        for finding in &report.findings {
            out.push_str(&render_finding(finding, view));
        }
    }
    let run = &report.run;
    match run.mode {
        Mode::Full => {
            if report.findings.is_empty() {
                if report.baselined > 0 {
                    out.push_str(&view.paint(
                        "32",
                        &format!(
                            "no new findings · {} baselined ({} of {} files claimed)",
                            report.baselined, run.files_claimed, run.files_discovered
                        ),
                    ));
                } else {
                    out.push_str(&view.paint(
                        "32",
                        &format!(
                            "no findings ({} of {} files claimed)",
                            run.files_claimed, run.files_discovered
                        ),
                    ));
                }
                out.push('\n');
            } else {
                out.push_str(&format!(
                    "{} finding{}",
                    report.findings.len(),
                    plural(report.findings.len())
                ));
                if report.baselined > 0 || !report.fixed.is_empty() {
                    out.push_str(&format!(
                        " · {} baselined · {} fixed since baseline",
                        report.baselined,
                        report.fixed.len()
                    ));
                }
                out.push('\n');
            }
        }
        Mode::Staged | Mode::Diff => {
            let label = if run.mode == Mode::Staged {
                "staged"
            } else {
                "diff"
            };
            out.push_str(&format!(
                "{label}: {} new · {} fixed · {} carried\n",
                report.findings.len(),
                report.fixed.len(),
                report.baselined
            ));
            if !view.quiet && !report.fixed.is_empty() {
                out.push_str("fixed by this change:\n");
                for finding in &report.fixed {
                    out.push_str("  ");
                    out.push_str(&render_finding(finding, view));
                }
            }
        }
    }
    if view.quiet {
        return out;
    }
    if let Some(health) = &report.health {
        let score = match &report.base_health {
            Some(base) => format!("{} → {}", base.score_text(), arrow_head(base, health, view)),
            None => health.score_text(),
        };
        out.push_str(&format!(
            "health {score} · implicated {} of {}",
            health.implicated, health.subjects
        ));
        for c in &health.by_category {
            out.push_str(&format!(" · {} {}", c.category.as_str(), c.findings));
        }
        out.push('\n');
    }
    for abstention in &report.abstained {
        out.push_str(&format!(
            "abstained: {} — {}\n",
            abstention.category.as_str(),
            abstention.reason
        ));
    }
    if report.suppressed.total > 0 {
        out.push_str(&format!(
            "suppressed: {} via kndo:allow\n",
            report.suppressed.total
        ));
    }
    // Contribution anomalies only: a clean contribution is JSON detail, but a
    // dropped assertion or a cut budget is the run being partial, said out loud.
    for contribution in &report.plugins {
        for line in &contribution.dropped {
            out.push_str(&format!("plugin {}: {line}\n", contribution.coordinate));
        }
        if contribution.content_budget_cut {
            out.push_str(&format!(
                "plugin {}: content budget cut — its findings may be partial\n",
                contribution.coordinate
            ));
        }
    }
    for diagnostic in &report.diagnostics {
        out.push_str(&format!(
            "diagnostic {}: {}\n",
            diagnostic.path.as_str(),
            diagnostic.message
        ));
    }
    if view.verbose {
        let ms = |d: std::time::Duration| format!("{:.1}ms", d.as_secs_f64() * 1000.0);
        out.push_str(&format!(
            "phases: discover {} · claim {} · extract {} · assemble {} · analyze {} · total {}\n",
            ms(timings.discover),
            ms(timings.claim),
            ms(timings.extract),
            ms(timings.assemble),
            ms(timings.analyze),
            ms(timings.total()),
        ));
    }
    out
}

/// The current side of a `base → current` health arrow, colored by DIRECTION —
/// a fact derived from the two measured ratios (lower implicated/subjects is
/// better), compared exactly by cross-multiplication.
fn arrow_head(base: &kndo::Health, health: &kndo::Health, view: &Presentation) -> String {
    let current = (health.implicated as u64) * (base.subjects as u64);
    let before = (base.implicated as u64) * (health.subjects as u64);
    let text = health.score_text();
    match current.cmp(&before) {
        std::cmp::Ordering::Less => view.paint("32", &text),
        std::cmp::Ordering::Greater => view.paint("31", &text),
        std::cmp::Ordering::Equal => text,
    }
}

fn render_finding(finding: &kndo::Finding, view: &Presentation) -> String {
    format!(
        "{} {} {}: {}\n",
        view.severity(finding.severity),
        finding.category.as_str(),
        finding.location(),
        finding.message
    )
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
