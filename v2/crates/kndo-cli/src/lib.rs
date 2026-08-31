//! The `kndo` command: the first facade frontend. Everything here is presentation —
//! parsing the invocation, rendering a [`kndo::Report`], mapping [`kndo::RunOutcome`]
//! to an exit code — and every fact it prints comes from the facade, never derived
//! locally. The logic lives in this library so tests (and the dogfood's own
//! reachability) exercise it through imports; `main` stays one call deep.

use clap::{Parser, Subcommand, ValueEnum};
use kndo::{Config, GatePolicy, Report, RunMode, RunOutcome, Severity, Threads};
use std::path::PathBuf;

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
    Check(RunArgs),
    /// Accept the current findings as the baseline future runs diff against
    Baseline(RunArgs),
}

#[derive(clap::Args)]
struct RunArgs {
    /// Project root (defaults to the current directory)
    path: Option<PathBuf>,
    /// Emit the full report as JSON instead of text
    #[arg(long)]
    json: bool,
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

pub fn run_args<I, T>(args: I) -> CliOutput
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
    run(cli)
}

pub fn run(cli: Cli) -> CliOutput {
    let command = cli.command.unwrap_or(Command::Check(RunArgs {
        path: None,
        json: false,
        no_cache: false,
        threads: None,
        fail_on: FailOn::Warning,
    }));
    match command {
        Command::Check(args) => check(args),
        Command::Baseline(args) => baseline(args),
    }
}

fn open_and_analyze(args: &RunArgs) -> Result<(kndo::Session, kndo::Snapshot), kndo::Refusal> {
    let root = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
    let config = Config {
        threads: match args.threads {
            Some(n) if n > 0 => Threads::Count(n),
            _ => Threads::Auto,
        },
        use_cache: !args.no_cache,
    };
    let session = kndo::open(root, config)?;
    let snapshot = session.analyze(RunMode::Full)?;
    Ok((session, snapshot))
}

fn refused(refusal: kndo::Refusal) -> CliOutput {
    let code = RunOutcome::Refused(refusal.clone()).exit_code();
    CliOutput {
        stdout: String::new(),
        stderr: format!("kndo: refused: {refusal}\n"),
        code,
    }
}

fn check(args: RunArgs) -> CliOutput {
    let (_, snapshot) = match open_and_analyze(&args) {
        Ok(pair) => pair,
        Err(refusal) => return refused(refusal),
    };
    let report = snapshot.report();
    let code = snapshot.gate(&args.fail_on.policy()).exit_code();
    let stdout = if args.json {
        let mut json = report.to_json();
        json.push('\n');
        json
    } else {
        render_text(&report)
    };
    CliOutput {
        stdout,
        stderr: String::new(),
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
/// engine), then what moved against the baseline, then the run's own honesty —
/// abstentions, suppressions, diagnostics.
fn render_text(report: &Report) -> String {
    let mut out = String::new();
    for finding in &report.findings {
        out.push_str(&render_finding(finding));
    }
    let run = &report.run;
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
        finding.subject.render(),
        finding.message
    )
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
