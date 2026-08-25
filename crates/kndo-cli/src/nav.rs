//! Navigation-verb subcommands — `kndo find`/`describe`/`uses`/`used-by`/`trace`, and
//! the batched `kndo query` JSONL interface. Pure argument parsing + dispatch + exit codes, like
//! every other command in this crate: all graph work happens behind
//! `Engine::query`/`query_batch`.

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;

use kndo::query_envelope::{QueryFlags, QueryRequest, QueryResult, Verb};

use crate::render;

/// A query run is capped at 1000 requests — a runaway-generation guard, not a
/// normal-use limit; `kndo query` reports how many lines it stopped short by rather than
/// silently dropping them.
const MAX_QUERY_BATCH: usize = 1000;

#[cfg_attr(test, derive(Debug))]
struct NavArgs {
    selectors: Vec<String>,
    flags: QueryFlags,
    format: Option<String>,
}

/// Nav verbs use `--color` for the reachability-color filter (`find --color
/// unreachable|test-only|…`), not for terminal-color control like `check`'s `--color
/// always|never` — the flag name belongs to `find`'s filter, and no nav verb
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
            "--split-by-color" => {} // by_color is always computed; flag accepted for parity
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

    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("kndo: cannot determine working directory: {e}");
            return ExitCode::from(2);
        }
    };
    let engine = match kndo::open(&cwd, crate::base_config_overrides()) {
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
                    flags: parsed.flags.into(),
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

    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("kndo: cannot determine working directory: {e}");
            return ExitCode::from(2);
        }
    };
    let engine = match kndo::open(&cwd, crate::base_config_overrides()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("kndo: {e}");
            return ExitCode::from(2);
        }
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
        assert_eq!(parsed.flags.kind.as_deref(), Some("function"));
        assert_eq!(parsed.flags.color.as_deref(), Some("unreachable"));
        assert_eq!(parsed.flags.lang.as_deref(), Some("rust"));
        assert_eq!(parsed.flags.edges.as_deref(), Some("imports"));
        assert_eq!(parsed.flags.roots.as_deref(), Some("production"));
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
        assert_eq!(parsed.flags.kind.as_deref(), Some("method"));
        assert_eq!(parsed.flags.color.as_deref(), Some("test-only"));
        assert_eq!(parsed.flags.lang.as_deref(), Some("go"));
        assert_eq!(parsed.flags.edges.as_deref(), Some("references"));
        assert_eq!(parsed.flags.roots.as_deref(), Some("test"));
        assert_eq!(parsed.format.as_deref(), Some("agent"));
    }

    #[test]
    fn numeric_flags_parse_and_reject_non_numbers() {
        let parsed =
            parse_nav_args(&args(&["--depth", "3", "--max-paths", "2", "--limit", "5"])).unwrap();
        assert_eq!(parsed.flags.depth, Some(3));
        assert_eq!(parsed.flags.max_paths, Some(2));
        assert_eq!(parsed.flags.limit, Some(5));
        for flag in ["--depth", "--max-paths", "--limit"] {
            let err = parse_nav_args(&args(&[flag, "abc"])).unwrap_err();
            assert!(err.contains(flag), "{err}");
            assert!(err.contains("not a number"), "{err}");
        }
    }

    #[test]
    fn pair_flag_splits_on_comma_and_accumulates() {
        let parsed =
            parse_nav_args(&args(&["--pair", "a.rs,b.rs", "--pair", "c.rs,d.rs"])).unwrap();
        assert_eq!(
            parsed.flags.pairs,
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
        assert!(parsed.flags.transitive);
        assert!(parsed.flags.if_deleted);
        assert!(parsed.flags.all);
    }

    #[test]
    fn unknown_flag_and_missing_value_are_errors() {
        let err = parse_nav_args(&args(&["--nope"])).unwrap_err();
        assert!(err.contains("unknown flag `--nope`"), "{err}");
        let err = parse_nav_args(&args(&["--kind"])).unwrap_err();
        assert!(err.contains("--kind needs a value"), "{err}");
    }

    #[test]
    fn flags_and_selectors_interleave() {
        let parsed = parse_nav_args(&args(&["foo", "--kind", "class", "bar", "--all"])).unwrap();
        assert_eq!(parsed.selectors, vec!["foo", "bar"]);
        assert_eq!(parsed.flags.kind.as_deref(), Some("class"));
        assert!(parsed.flags.all);
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
