//! Navigation-verb subcommands — `kndo find`/`describe`/`uses`/`used-by`/`trace`, and
//! the batched `kndo query` JSONL interface. Pure argument parsing + dispatch + exit codes, like
//! every other command in this crate: all graph work happens behind
//! `Engine::query`/`query_batch`.

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;

use kndo::{QueryFlags, QueryRequest, QueryResult, Verb};

use crate::render;

/// A query run is capped at 1000 requests — a runaway-generation guard, not a
/// normal-use limit; `kndo query` reports how many lines it stopped short by rather than
/// silently dropping them.
const MAX_QUERY_BATCH: usize = 1000;

/// Nav verbs use `--color` for the reachability-color filter (`find --color
/// unreachable|test-only|…`), not for terminal-color control like `check`'s `--color
/// always|never` — the flag name belongs to `find`'s filter, and no nav verb
/// needs a terminal-color override of its own; TTY/`NO_COLOR` auto-detection alone decides that.
#[derive(Debug, clap::Parser)]
struct NavArgs {
    selectors: Vec<String>,
    #[arg(long)]
    kind: Option<String>,
    #[arg(long)]
    color: Option<String>,
    #[arg(long)]
    lang: Option<String>,
    #[arg(long)]
    depth: Option<u32>,
    #[arg(long)]
    transitive: bool,
    #[arg(long)]
    edges: Option<String>,
    // `by_color` is always computed regardless of this flag; `--split-by-color` still parses
    // as a no-op rather than erroring, so a command line that includes it keeps working.
    #[arg(long = "split-by-color")]
    split_by_color: bool,
    #[arg(long = "if-deleted")]
    if_deleted: bool,
    #[arg(long)]
    all: bool,
    #[arg(long = "max-paths")]
    max_paths: Option<usize>,
    #[arg(long)]
    roots: Option<String>,
    #[arg(long, value_parser = parse_pair)]
    pair: Vec<(String, String)>,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long)]
    format: Option<String>,
}

/// clap's `value_parser = parse_pair` names this function as a bare, lowercase attribute value
/// — the exact shape `emit_attr_path` (kndo-adapter-rust) deliberately does not treat as a
/// reference, per the measured false-positive rate (78 of 79 candidates on serde alone) that
/// ruled out relaxing that guard.
// kndo:allow unused only visible reference is inside clap's `#[arg(value_parser = ...)]`, a bare lowercase attribute value the Rust adapter deliberately does not scan (measured 78/79 FP rate)
fn parse_pair(raw: &str) -> Result<(String, String), String> {
    raw.split_once(',')
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .ok_or_else(|| format!("--pair `{raw}` must be `A,B`"))
}

/// Every nav verb's own dispatch parses through here, so a typo'd `--kidn` or a missing value
/// on `--depth` is a hard error naming the flag, not a silently-ignored no-op — clap owns the
/// exact wording; every caller of this only depends on the flag name appearing in it.
fn parse_nav_args(args: &[String]) -> Result<NavArgs, String> {
    use clap::Parser;
    NavArgs::try_parse_from(std::iter::once("kndo".to_string()).chain(args.iter().cloned()))
        .map_err(|e| e.to_string())
}

/// Shared setup for every single-shot nav command: open the engine, run one `QueryRequest`,
/// render, and translate the result's status into the exit code.
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

    let (_, engine) = match crate::open_engine(crate::base_config_overrides()) {
        Ok(t) => t,
        Err(code) => return code,
    };

    let result = engine.query(QueryRequest {
        id: None,
        verb,
        selectors: parsed.selectors,
        flags: QueryFlags {
            kind: parsed.kind,
            color: parsed.color,
            lang: parsed.lang,
            depth: parsed.depth,
            transitive: parsed.transitive,
            edges: parsed.edges,
            all: parsed.all,
            max_paths: parsed.max_paths,
            roots: parsed.roots,
            pairs: parsed.pair,
            limit: parsed.limit,
            if_deleted: parsed.if_deleted,
        },
    });

    render_and_print(&result, parsed.format.as_deref());
    exit_code_for_status(result.status())
}

pub(crate) fn find_cmd(args: &[String]) -> ExitCode {
    run_one(Verb::Find, args)
}

pub(crate) fn describe_cmd(args: &[String]) -> ExitCode {
    run_one(Verb::Describe, args)
}

pub(crate) fn uses_cmd(args: &[String]) -> ExitCode {
    run_one(Verb::Uses, args)
}

pub(crate) fn used_by_cmd(args: &[String]) -> ExitCode {
    run_one(Verb::UsedBy, args)
}

pub(crate) fn impact_cmd(args: &[String]) -> ExitCode {
    run_one(Verb::Impact, args)
}

pub(crate) fn trace_cmd(args: &[String]) -> ExitCode {
    run_one(Verb::Trace, args)
}

/// `kndo explain <finding-id>` (RFC 0006 §2). Routed through the same `run_one` as every
/// navigation verb — the envelope, the `--format` handling, the `not-found` status and the
/// exit code all come from there, so `explain` cannot end up with its own second convention
/// for any of them. Its "selector" is a finding id rather than a node selector; that is the
/// only difference, and it lives entirely core-side.
pub(crate) fn explain_cmd(args: &[String]) -> ExitCode {
    run_one(Verb::Explain, args)
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
                by_package: false,
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
    flags: QueryFlags,
}

/// The pure half of `kndo query`: each non-blank input line becomes either a `QueryRequest`
/// or a `(line index, message)` parse error, stopping (with the truncation flag set) once the
/// batch cap is reached. Separated from the stdin/engine/stdout plumbing so the line grammar
/// is testable as data-in/data-out.
fn parse_query_lines(
    lines: impl Iterator<Item = String>,
) -> (Vec<QueryRequest>, Vec<(usize, String)>, bool) {
    let mut requests = Vec::new();
    let mut parse_errors: Vec<(usize, String)> = Vec::new();
    let mut truncated = false;

    for (n, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if requests.len() + parse_errors.len() >= MAX_QUERY_BATCH {
            truncated = true;
            break;
        }
        match serde_json::from_str::<QueryLine>(&line) {
            Ok(parsed) => match Verb::parse(&parsed.verb) {
                Some(verb) => requests.push(QueryRequest {
                    id: parsed.id,
                    verb,
                    selectors: parsed.selectors,
                    flags: parsed.flags,
                }),
                None => parse_errors.push((n, format!("unknown verb `{}`", parsed.verb))),
            },
            Err(e) => parse_errors.push((n, format!("malformed request: {e}"))),
        }
    }
    (requests, parse_errors, truncated)
}

/// `kndo query`: reads JSON Lines from stdin, revalidates the cache once for the
/// whole batch, answers in input order as JSON Lines on stdout — `run` on the first line only.
/// JSON-only by design (no human format — this is the machine/agent transport).
pub(crate) fn query_cmd() -> ExitCode {
    let stdin = std::io::stdin();
    // A human typing `kndo query` at a terminal would just see it hang waiting for stdin
    // — say what the command wants instead.
    if std::io::IsTerminal::is_terminal(&stdin) {
        eprintln!(
            "kndo: query reads JSON request lines from stdin — pipe them in,              e.g. `echo '{{\"verb\":\"find\",\"selectors\":[\"foo*\"]}}' | kndo query`"
        );
        return ExitCode::from(2);
    }
    let (batch_requests, parse_errors, truncated) =
        parse_query_lines(stdin.lock().lines().map_while(Result::ok));

    let (_, engine) = match crate::open_engine(crate::base_config_overrides()) {
        Ok(t) => t,
        Err(code) => return code,
    };

    let results = engine.query_batch(batch_requests);

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut worst_rank = 0u8; // 0 = ok, 1 = not-found, 2 = error — the status ordering
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

    if std::io::stdout().is_terminal() && results.is_empty() && parse_errors.is_empty() {
        eprintln!("kndo: query reads JSON Lines requests from stdin — one JSON request per line");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn positionals_become_selectors_in_order() {
        let parsed = parse_nav_args(&args(&["foo*", "src/lib.rs", "Bar.baz"])).unwrap();
        assert_eq!(parsed.selectors, vec!["foo*", "src/lib.rs", "Bar.baz"]);
        assert!(parsed.format.is_none());
        let empty = parse_nav_args(&[]).unwrap();
        assert!(empty.selectors.is_empty());
    }

    #[test]
    fn valued_flags_space_form() {
        let parsed = parse_nav_args(&args(&[
            "--kind",
            "function",
            "--color",
            "unreachable",
            "--lang",
            "rust",
            "--edges",
            "imports",
            "--roots",
            "production",
            "--format",
            "json",
        ]))
        .unwrap();
        assert_eq!(parsed.kind.as_deref(), Some("function"));
        assert_eq!(parsed.color.as_deref(), Some("unreachable"));
        assert_eq!(parsed.lang.as_deref(), Some("rust"));
        assert_eq!(parsed.edges.as_deref(), Some("imports"));
        assert_eq!(parsed.roots.as_deref(), Some("production"));
        assert_eq!(parsed.format.as_deref(), Some("json"));
    }

    #[test]
    fn valued_flags_equals_form() {
        let parsed = parse_nav_args(&args(&[
            "--kind=method",
            "--color=test-only",
            "--lang=go",
            "--edges=references",
            "--roots=test",
            "--format=agent",
        ]))
        .unwrap();
        assert_eq!(parsed.kind.as_deref(), Some("method"));
        assert_eq!(parsed.color.as_deref(), Some("test-only"));
        assert_eq!(parsed.lang.as_deref(), Some("go"));
        assert_eq!(parsed.edges.as_deref(), Some("references"));
        assert_eq!(parsed.roots.as_deref(), Some("test"));
        assert_eq!(parsed.format.as_deref(), Some("agent"));
    }

    #[test]
    fn numeric_flags_parse_and_reject_non_numbers() {
        let parsed =
            parse_nav_args(&args(&["--depth", "3", "--max-paths", "2", "--limit", "5"])).unwrap();
        assert_eq!(parsed.depth, Some(3));
        assert_eq!(parsed.max_paths, Some(2));
        assert_eq!(parsed.limit, Some(5));
        // clap's own message for a bad numeric value (e.g. "invalid digit found in string")
        // is what surfaces here; every caller only depends on the flag name appearing in the
        // error, which it still does.
        for flag in ["--depth", "--max-paths", "--limit"] {
            let err = parse_nav_args(&args(&[flag, "abc"])).unwrap_err();
            assert!(err.contains(flag), "{err}");
        }
    }

    #[test]
    fn pair_flag_splits_on_comma_and_accumulates() {
        let parsed =
            parse_nav_args(&args(&["--pair", "a.rs,b.rs", "--pair", "c.rs,d.rs"])).unwrap();
        assert_eq!(
            parsed.pair,
            vec![
                ("a.rs".to_string(), "b.rs".to_string()),
                ("c.rs".to_string(), "d.rs".to_string()),
            ]
        );
        let err = parse_nav_args(&args(&["--pair", "no-comma"])).unwrap_err();
        assert!(err.contains("must be `A,B`"), "{err}");
    }

    #[test]
    fn boolean_flags_flip_and_split_by_color_is_accepted_inert() {
        let parsed = parse_nav_args(&args(&[
            "--transitive",
            "--if-deleted",
            "--all",
            "--split-by-color",
        ]))
        .unwrap();
        assert!(parsed.transitive);
        assert!(parsed.if_deleted);
        assert!(parsed.all);
    }

    #[test]
    fn unknown_flag_and_missing_value_are_errors() {
        // clap's own wording ("unexpected argument", "a value is required for") names the
        // offending flag; what every caller actually depends on is the flag name appearing in
        // the error, which both assertions still check.
        let err = parse_nav_args(&args(&["--nope"])).unwrap_err();
        assert!(err.contains("--nope"), "{err}");
        let err = parse_nav_args(&args(&["--kind"])).unwrap_err();
        assert!(err.contains("--kind"), "{err}");
    }

    #[test]
    fn flags_and_selectors_interleave() {
        let parsed = parse_nav_args(&args(&["foo", "--kind", "class", "bar", "--all"])).unwrap();
        assert_eq!(parsed.selectors, vec!["foo", "bar"]);
        assert_eq!(parsed.kind.as_deref(), Some("class"));
        assert!(parsed.all);
    }

    // ------------------------------------------------------------ parse_query_lines

    fn lines(list: &[&str]) -> impl Iterator<Item = String> + use<> {
        list.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn a_valid_line_maps_every_flag_field() {
        let (requests, errors, truncated) = parse_query_lines(lines(&[r#"{
            "id": "q1", "verb": "find", "selectors": ["foo*"],
            "flags": {"kind": "function", "color": "unreachable", "lang": "rust",
                      "depth": 2, "transitive": true, "edges": "imports", "all": true,
                      "max_paths": 3, "roots": "production",
                      "pairs": [["a.rs", "b.rs"]], "limit": 7, "if_deleted": true}
        }"#]));
        assert!(errors.is_empty());
        assert!(!truncated);
        assert_eq!(requests.len(), 1);
        let r = &requests[0];
        assert_eq!(r.id.as_deref(), Some("q1"));
        assert_eq!(r.selectors, vec!["foo*"]);
        let f = &r.flags;
        assert_eq!(f.kind.as_deref(), Some("function"));
        assert_eq!(f.color.as_deref(), Some("unreachable"));
        assert_eq!(f.lang.as_deref(), Some("rust"));
        assert_eq!(f.depth, Some(2));
        assert!(f.transitive);
        assert_eq!(f.edges.as_deref(), Some("imports"));
        assert!(f.all);
        assert_eq!(f.max_paths, Some(3));
        assert_eq!(f.roots.as_deref(), Some("production"));
        assert_eq!(f.pairs, vec![("a.rs".to_string(), "b.rs".to_string())]);
        assert_eq!(f.limit, Some(7));
        assert!(f.if_deleted);
    }

    #[test]
    fn unknown_verb_is_a_line_error_and_other_lines_still_parse() {
        let (requests, errors, _) = parse_query_lines(lines(&[
            r#"{"verb": "levitate", "selectors": ["x"]}"#,
            r#"{"verb": "find", "selectors": ["y"]}"#,
        ]));
        assert_eq!(requests.len(), 1);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, 0);
        assert!(errors[0].1.contains("unknown verb `levitate`"));
    }

    #[test]
    fn malformed_json_reports_the_line_index() {
        let (requests, errors, _) = parse_query_lines(lines(&["not json", r#"{"verb": "find"}"#]));
        assert_eq!(requests.len(), 1);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, 0);
        assert!(errors[0].1.contains("malformed request"));
    }

    #[test]
    fn blank_lines_are_skipped_without_consuming_the_batch_budget() {
        let (requests, errors, truncated) = parse_query_lines(lines(&[
            "",
            "   ",
            r#"{"verb": "find", "selectors": ["a"]}"#,
        ]));
        assert_eq!(requests.len(), 1);
        assert!(errors.is_empty());
        assert!(!truncated);
    }

    #[test]
    fn the_batch_cap_truncates_and_reports_it() {
        let line = r#"{"verb": "find", "selectors": ["a"]}"#;
        let many: Vec<&str> = std::iter::repeat_n(line, MAX_QUERY_BATCH + 1).collect();
        let (requests, errors, truncated) = parse_query_lines(lines(&many));
        assert_eq!(requests.len(), MAX_QUERY_BATCH);
        assert!(errors.is_empty());
        assert!(truncated);
    }

    #[test]
    fn status_rank_orders_ok_not_found_error() {
        assert_eq!(status_rank("ok"), 0);
        assert_eq!(status_rank("not-found"), 1);
        assert_eq!(status_rank("error"), 2);
        assert_eq!(status_rank("anything-else"), 2);
    }
}
