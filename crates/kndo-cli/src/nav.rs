//! Navigation-verb subcommands (RFC 0007) — `kndo find`/`describe`/`uses`/`used-by`/`trace`, and
//! the batched `kndo query` JSONL interface. Pure argument parsing + dispatch + exit codes, like
//! every other command in this crate (contracts §5): all graph work happens behind
//! `Engine::query`/`query_batch`.

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;

use kndo::query_envelope::{QueryFlags, QueryRequest, QueryResult, Verb};

use crate::render;

/// A query run is capped at 1000 requests (RFC 0007 §4.7) — a runaway-generation guard, not a
/// normal-use limit; `kndo query` reports how many lines it stopped short by rather than
/// silently dropping them.
const MAX_QUERY_BATCH: usize = 1000;

struct NavArgs {
    selectors: Vec<String>,
    flags: QueryFlags,
    format: Option<String>,
}

/// Nav verbs use `--color` for RFC 0007 §4.1's reachability-color filter (`find --color
/// unreachable|test-only|…`), not for terminal-color control like `check`'s `--color
/// always|never` (RFC 0009) — the RFC names it that way for `find` specifically and no nav verb
/// needs a terminal-color override of its own; TTY/`NO_COLOR` auto-detection alone decides that.
fn parse_nav_args(args: &[String]) -> Result<NavArgs, String> {
    let mut selectors = Vec::new();
    let mut flags = QueryFlags::default();
    let mut format = None;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--kind" => flags.kind = Some(next_value(&mut it, "--kind")?),
            "--color" => flags.color = Some(next_value(&mut it, "--color")?),
            "--lang" => flags.lang = Some(next_value(&mut it, "--lang")?),
            "--depth" => {
                let raw = next_value(&mut it, "--depth")?;
                flags.depth = Some(
                    raw.parse()
                        .map_err(|_| format!("--depth `{raw}` is not a number"))?,
                );
            }
            "--transitive" => flags.transitive = true,
            "--edges" => flags.edges = Some(next_value(&mut it, "--edges")?),
            "--split-by-color" => {} // by_color is always computed; flag accepted for RFC parity
            "--if-deleted" => flags.if_deleted = true,
            "--all" => flags.all = true,
            "--max-paths" => {
                let raw = next_value(&mut it, "--max-paths")?;
                flags.max_paths = Some(
                    raw.parse()
                        .map_err(|_| format!("--max-paths `{raw}` is not a number"))?,
                );
            }
            "--roots" => flags.roots = Some(next_value(&mut it, "--roots")?),
            "--pair" => {
                let raw = next_value(&mut it, "--pair")?;
                let (a, b) = raw
                    .split_once(',')
                    .ok_or_else(|| format!("--pair `{raw}` must be `A,B`"))?;
                flags.pairs.push((a.to_string(), b.to_string()));
            }
            "--limit" => {
                let raw = next_value(&mut it, "--limit")?;
                flags.limit = Some(
                    raw.parse()
                        .map_err(|_| format!("--limit `{raw}` is not a number"))?,
                );
            }
            "--format" => format = Some(next_value(&mut it, "--format")?),
            s if s.starts_with("--format=") => format = Some(s["--format=".len()..].to_string()),
            s if s.starts_with("--kind=") => flags.kind = Some(s["--kind=".len()..].to_string()),
            s if s.starts_with("--color=") => flags.color = Some(s["--color=".len()..].to_string()),
            s if s.starts_with("--lang=") => flags.lang = Some(s["--lang=".len()..].to_string()),
            s if s.starts_with("--edges=") => flags.edges = Some(s["--edges=".len()..].to_string()),
            s if s.starts_with("--roots=") => flags.roots = Some(s["--roots=".len()..].to_string()),
            s if s.starts_with("--") => return Err(format!("unknown flag `{s}`")),
            positional => selectors.push(positional.to_string()),
        }
    }
    Ok(NavArgs {
        selectors,
        flags,
        format,
    })
}

fn next_value(it: &mut std::slice::Iter<'_, String>, flag: &str) -> Result<String, String> {
    it.next()
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value"))
}

/// Shared setup for every single-shot nav command: open the engine, run one `QueryRequest`,
/// render, and translate the result's status into RFC 0007 §6's exit code.
fn run_one(verb: Verb, args: &[String]) -> ExitCode {
    let parsed = match parse_nav_args(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
    };
    if parsed.selectors.is_empty() {
        eprintln!(
            "kndo: {} needs at least one selector/pattern",
            verb.as_str()
        );
        return ExitCode::from(2);
    }

    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("kndo: cannot determine working directory: {e}");
            return ExitCode::from(2);
        }
    };
    let mut engine = match kndo::open(&cwd, crate::base_config_overrides()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
    };

    let result = engine.query(QueryRequest {
        id: None,
        verb,
        selectors: parsed.selectors,
        flags: parsed.flags,
    });

    render_and_print(&result, parsed.format.as_deref());
    exit_code_for_status(result.status())
}

pub fn find_cmd(args: &[String]) -> ExitCode {
    run_one(Verb::Find, args)
}

pub fn describe_cmd(args: &[String]) -> ExitCode {
    run_one(Verb::Describe, args)
}

pub fn uses_cmd(args: &[String]) -> ExitCode {
    run_one(Verb::Uses, args)
}

pub fn used_by_cmd(args: &[String]) -> ExitCode {
    run_one(Verb::UsedBy, args)
}

pub fn impact_cmd(args: &[String]) -> ExitCode {
    run_one(Verb::Impact, args)
}

pub fn trace_cmd(args: &[String]) -> ExitCode {
    run_one(Verb::Trace, args)
}

fn render_and_print(result: &QueryResult, format: Option<&str>) {
    let format = crate::resolve_format(format);
    match format.as_str() {
        "json" => println!("{}", result.to_json()),
        "agent" => println!("{}", result.to_agent_format()),
        "human" => {
            let opts = render::RenderOptions {
                color: crate::resolve_color(None),
                verbose: false,
                quiet: false,
            };
            print!("{}", render::render_query(result, &opts));
        }
        other => {
            eprintln!("kndo: unknown --format `{other}` (human, json, agent) — showing json");
            println!("{}", result.to_json());
        }
    }
}

fn exit_code_for_status(status: &str) -> ExitCode {
    ExitCode::from(status_rank(status))
}

// ---------------------------------------------------------------- kndo query (JSONL)

#[derive(serde::Deserialize)]
struct QueryLine {
    id: Option<String>,
    verb: String,
    #[serde(default)]
    selectors: Vec<String>,
    #[serde(default)]
    flags: QueryLineFlags,
}

#[derive(serde::Deserialize, Default)]
struct QueryLineFlags {
    kind: Option<String>,
    color: Option<String>,
    lang: Option<String>,
    depth: Option<u32>,
    #[serde(default)]
    transitive: bool,
    edges: Option<String>,
    #[serde(default)]
    all: bool,
    max_paths: Option<usize>,
    roots: Option<String>,
    #[serde(default)]
    pairs: Vec<(String, String)>,
    limit: Option<usize>,
    #[serde(default)]
    if_deleted: bool,
}

impl From<QueryLineFlags> for QueryFlags {
    fn from(f: QueryLineFlags) -> Self {
        QueryFlags {
            kind: f.kind,
            color: f.color,
            lang: f.lang,
            depth: f.depth,
            transitive: f.transitive,
            edges: f.edges,
            all: f.all,
            max_paths: f.max_paths,
            roots: f.roots,
            pairs: f.pairs,
            limit: f.limit,
            if_deleted: f.if_deleted,
        }
    }
}

/// `kndo query` (RFC 0007 §4.7): reads JSON Lines from stdin, revalidates the cache once for the
/// whole batch, answers in input order as JSON Lines on stdout — `run` on the first line only.
/// JSON-only by design (no human format — this is the machine/agent transport).
pub fn query_cmd() -> ExitCode {
    let stdin = std::io::stdin();
    let mut requests = Vec::new();
    let mut parse_errors: Vec<(usize, String)> = Vec::new();
    let mut truncated = false;

    for (n, line) in stdin.lock().lines().enumerate() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if requests.len() + parse_errors.len() >= MAX_QUERY_BATCH {
            truncated = true;
            break;
        }
        match serde_json::from_str::<QueryLine>(&line) {
            Ok(parsed) => match Verb::parse(&parsed.verb) {
                Some(verb) => requests.push((
                    parsed.id.clone(),
                    QueryRequest {
                        id: parsed.id,
                        verb,
                        selectors: parsed.selectors,
                        flags: parsed.flags.into(),
                    },
                )),
                None => parse_errors.push((n, format!("unknown verb `{}`", parsed.verb))),
            },
            Err(e) => parse_errors.push((n, format!("malformed request: {e}"))),
        }
    }

    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("kndo: cannot determine working directory: {e}");
            return ExitCode::from(2);
        }
    };
    let mut engine = match kndo::open(&cwd, crate::base_config_overrides()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
    };

    let ids: Vec<Option<String>> = requests.iter().map(|(id, _)| id.clone()).collect();
    let batch_requests: Vec<QueryRequest> = requests.into_iter().map(|(_, r)| r).collect();
    let results = engine.query_batch(batch_requests);

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut worst_rank = 0u8; // 0 = ok, 1 = not-found, 2 = error — RFC 0007 §6's ordering
    for (i, result) in results.iter().enumerate() {
        let _ = writeln!(out, "{}", result.to_json_line(i == 0));
        worst_rank = worst_rank.max(status_rank(result.status()));
    }
    for (line_no, message) in &parse_errors {
        eprintln!("kndo: query line {}: {message}", line_no + 1);
        worst_rank = worst_rank.max(2);
    }
    if truncated {
        eprintln!(
            "kndo: query input exceeds {MAX_QUERY_BATCH} requests — remaining lines were not answered"
        );
        worst_rank = worst_rank.max(2);
    }
    let _ = ids; // ids travel inside each QueryRequest/QueryResult already; kept for clarity at the call site

    if std::io::stdout().is_terminal() && results.is_empty() && parse_errors.is_empty() {
        eprintln!("kndo: query reads JSON Lines requests from stdin — see RFC 0007 §4.7");
    }

    ExitCode::from(worst_rank)
}

fn status_rank(status: &str) -> u8 {
    match status {
        "ok" => 0,
        "not-found" => 1,
        _ => 2,
    }
}
