//! The `kndo` command: the first facade frontend. Everything here is presentation —
//! parsing the invocation, rendering a [`kndo::Report`], mapping [`kndo::RunOutcome`]
//! to an exit code — and every fact it prints comes from the facade, never derived
//! locally. The logic lives in this library so tests (and the dogfood's own
//! reachability) exercise it through imports; `main` stays one call deep.

use clap::{Parser, Subcommand, ValueEnum};
use kndo::{Config, GatePolicy, Mode, Report, RunMode, RunOutcome, Severity, Threads};
use std::path::PathBuf;

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
    /// Lowest severity that fails the run
    #[arg(long, value_enum, default_value_t = FailOn::Warning)]
    fail_on: FailOn,
}

/// The renderings `check` offers. `human` is this frontend's presentation; the
/// other three are core's render contracts, byte-identical from any frontend.
#[derive(ValueEnum, Clone, Copy)]
enum Format {
    Human,
    Json,
    Agent,
    Sarif,
}

#[derive(ValueEnum, Clone, Copy)]
enum FailOn {
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
            fail_on: FailOn::Warning,
        },
        format: None,
        staged: false,
        diff: None,
    }));
    match command {
        Command::Check(args) => check(args, &host),
        Command::Baseline(args) => baseline(args),
    }
}

/// Flag > `KNDO_FORMAT` > the terminal. A malformed environment value cannot be
/// a refusal — the invoker of a pipeline did not necessarily set it — so it warns
/// on stderr and falls through to the terminal default. An empty value is unset.
fn resolve_format(flag: Option<Format>, host: &Host) -> (Format, Option<String>) {
    if let Some(format) = flag {
        return (format, None);
    }
    let by_tty = if host.tty {
        Format::Human
    } else {
        Format::Json
    };
    match host.format_env.as_deref() {
        None | Some("") => (by_tty, None),
        Some(raw) => match <Format as ValueEnum>::from_str(raw, true) {
            Ok(format) => (format, None),
            Err(_) => (
                by_tty,
                Some(format!(
                    "kndo: unknown KNDO_FORMAT `{raw}` (human, json, agent, sarif) — choosing by terminal\n"
                )),
            ),
        },
    }
}

fn open_and_analyze(args: &RunArgs) -> Result<(kndo::Session, kndo::Snapshot), kndo::Refusal> {
    let root = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
    let snapshot = analyze_at(&root, args, !args.no_cache)?;
    let config = Config {
        threads: match args.threads {
            Some(n) if n > 0 => Threads::Count(n),
            _ => Threads::Auto,
        },
        use_cache: !args.no_cache,
    };
    let session = kndo::open(root, config)?;
    Ok((session, snapshot))
}

fn analyze_at(
    root: &std::path::Path,
    args: &RunArgs,
    use_cache: bool,
) -> Result<kndo::Snapshot, kndo::Refusal> {
    let config = Config {
        threads: match args.threads {
            Some(n) if n > 0 => Threads::Count(n),
            _ => Threads::Auto,
        },
        use_cache,
    };
    kndo::open(root.to_path_buf(), config)?.analyze(RunMode::Full)
}

/// A diff-mode run: two full analyses over two pinned trees, composed. The base
/// (and, for `--staged`, the index) is materialized by the git edge; scratch trees
/// run cache-off so nothing is written into them. The comparison then rides the
/// baseline mechanism — `Snapshot::against` documents why the baseline file never
/// participates in a tree-vs-tree split.
fn diff_snapshot(args: &RunArgs, comparison: git::Comparison) -> Result<kndo::Snapshot, CliOutput> {
    let root = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
    let mode = match comparison {
        git::Comparison::Staged => Mode::Staged,
        git::Comparison::Against(_) => Mode::Diff,
    };
    let base_tree = comparison.base_tree(&root).map_err(git_failed)?;
    let base = git::materialize(&root, &base_tree).map_err(git_failed)?;
    let base_snapshot = analyze_at(&base.root, args, false).map_err(refused)?;
    let mut snapshot = match comparison.current_tree(&root).map_err(git_failed)? {
        Some(index_tree) => {
            let current = git::materialize(&root, &index_tree).map_err(git_failed)?;
            analyze_at(&current.root, args, false).map_err(refused)?
        }
        None => analyze_at(&root, args, !args.no_cache).map_err(refused)?,
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
    let (format, warning) = resolve_format(args.format, host);
    let comparison = if args.staged {
        Some(git::Comparison::Staged)
    } else {
        args.diff.clone().map(git::Comparison::Against)
    };
    let snapshot = match comparison {
        Some(comparison) => match diff_snapshot(&args.run, comparison) {
            Ok(snapshot) => snapshot,
            Err(failure) => return failure,
        },
        None => match open_and_analyze(&args.run) {
            Ok((_, snapshot)) => snapshot,
            Err(refusal) => return refused(refusal),
        },
    };
    let report = snapshot.report();
    let code = snapshot.gate(&args.run.fail_on.policy()).exit_code();
    // Text documents arrive newline-terminated from their renders; JSON values
    // are framed here.
    let stdout = match format {
        Format::Human => render_text(&report),
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
        stderr: warning.unwrap_or_default(),
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

/// The text report: findings in the facade's canonical order (already sorted by the
/// engine), then what moved against the comparison — the baseline in full mode, the
/// base tree in the diff modes — then the run's own honesty: abstentions,
/// suppressions, diagnostics.
fn render_text(report: &Report) -> String {
    let mut out = String::new();
    for finding in &report.findings {
        out.push_str(&render_finding(finding));
    }
    let run = &report.run;
    match run.mode {
        Mode::Full => {
            if report.findings.is_empty() {
                if report.baselined > 0 {
                    out.push_str(&format!(
                        "no new findings · {} baselined ({} of {} files claimed)\n",
                        report.baselined, run.files_claimed, run.files_discovered
                    ));
                } else {
                    out.push_str(&format!(
                        "no findings ({} of {} files claimed)\n",
                        run.files_claimed, run.files_discovered
                    ));
                }
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
            if !report.fixed.is_empty() {
                out.push_str("fixed by this change:\n");
                for finding in &report.fixed {
                    out.push_str("  ");
                    out.push_str(&render_finding(finding));
                }
            }
        }
    }
    if let Some(health) = &report.health {
        let score = match &report.base_health {
            Some(base) => format!("{} → {}", base.score_text(), health.score_text()),
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
    out
}

fn render_finding(finding: &kndo::Finding) -> String {
    format!(
        "{} {} {}: {}\n",
        finding.severity.as_str(),
        finding.category.as_str(),
        finding.location(),
        finding.message
    )
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
