//! `--format agent` — a token-frugal, line-oriented plain-text rendering of [`RunResult`],
//! optimized for LLM context windows (contracts/output-schema.md §9). Renders core-side, like
//! JSON (RFC 0001 §2, contracts §5): every frontend — CLI today, `kndo serve`/MCP tomorrow —
//! emits byte-identical agent text, never reconstructed per-frontend.
//!
//! Diff modes render `new:`/`fixed:` blocks instead of `findings:` (output-schema §9's own
//! example), with one numbering sequence running across both. No `budget:` line: delta budgets
//! depend on health scoring, which doesn't exist yet (M4). Findings carrying a `related`
//! evidence chain (first populated by `cyclic`) render it as indented `evidence:` lines — the
//! format "can never carry information absent from the JSON" (output-schema §9), and `related`
//! now IS in the JSON. `remediation` still isn't — no `fix:` lines. Diff mode's NEW findings
//! aren't visually split by `delta_origin` here; output-schema §9's own example shows one flat
//! `new:` block, `delta_origin` traveling on each line's JSON-equivalent data only.
//! `next:` names only commands that work today (`--format json`). `kndo explain` still doesn't
//! exist (deferred — see `engine::Finding`'s doc on `related`/`remediation`), so it's not offered
//! as if it did; the navigation verbs (`kndo used-by`, …) now do, via [`render_query`].
//!
//! Suppressed findings are never listed here either — matching, marked findings are already
//! absent from `RunResult.findings`/`fixed` by the time this module sees them — only appended to
//! the result line as `| suppressed N inline, M config`, and only when non-zero.
//!
//! [`render_query`] renders the navigation-verb envelope (output-schema §9: "numbered entries of
//! `[selector] kind path:line` plus the verb's specifics … same `more:`/`next:` discipline").

use crate::engine::{Finding, RunResult, KNDO_VERSION};
use crate::query::{NeighborEntry, QNodeRef};
use crate::query_envelope::{QueryResult, ResultEntry};
use crate::vocab::Confidence;

const GROUP_ORDER: [&str; 4] = ["defect", "waste", "risk", "hygiene"];
const AGENT_FORMAT_VERSION: u32 = 1;

pub fn render(result: &RunResult) -> String {
    if result.mode == "staged" || result.mode == "diff" {
        return render_diff(result);
    }

    let mut out = String::new();
    out.push_str(&header(result));
    out.push('\n');
    out.push_str(&result_line(result));
    out.push('\n');

    if !result.findings.is_empty() {
        out.push_str("findings:\n");
        let mut n = 0usize;
        for group in ordered_groups(&result.findings) {
            let mut in_group: Vec<&Finding> = result
                .findings
                .iter()
                .filter(|f| f.group == group)
                .collect();
            sort_findings(&mut in_group);
            for f in in_group {
                n += 1;
                out.push_str(&finding_line(n, f));
                out.push('\n');
                push_evidence(&mut out, f);
            }
        }
    }

    // Elision is always explicit (RFC 0009 §5, extended here by the same principle): this
    // renderer never caps or paginates, so it is always "none" — but the line is never
    // skipped, so a reader never has to guess whether truncation happened silently.
    out.push_str("more: none\n");
    out.push_str("next: kndo check --format json\n");
    out
}

/// Diff modes' rendering — output-schema §9's own example: `new:`/`fixed:` blocks sharing one
/// running number sequence (new findings numbered first, fixed continuing after), instead of
/// `findings:`.
fn render_diff(result: &RunResult) -> String {
    let mut out = String::new();
    out.push_str(&header(result));
    out.push('\n');
    out.push_str(&diff_result_line(result));
    out.push('\n');

    let mut n = 0usize;
    if !result.findings.is_empty() {
        out.push_str("new:\n");
        for group in ordered_groups(&result.findings) {
            let mut in_group: Vec<&Finding> = result
                .findings
                .iter()
                .filter(|f| f.group == group)
                .collect();
            sort_findings(&mut in_group);
            for f in in_group {
                n += 1;
                out.push_str(&finding_line(n, f));
                out.push('\n');
                push_evidence(&mut out, f);
            }
        }
    }
    if !result.fixed.is_empty() {
        out.push_str("fixed:\n");
        for group in ordered_groups(&result.fixed) {
            let mut in_group: Vec<&Finding> =
                result.fixed.iter().filter(|f| f.group == group).collect();
            sort_findings(&mut in_group);
            for f in in_group {
                n += 1;
                out.push_str(&finding_line(n, f));
                out.push('\n');
                push_evidence(&mut out, f);
            }
        }
    }

    out.push_str("more: none\n");
    out.push_str("next: kndo check --format json\n");
    out
}

/// Output-schema §9's result-line health segment: `health 82.4 -> 84.1 (B)` when a previous
/// score exists (stored snapshot in full mode, the computed "before" side in diff modes),
/// `health 84.1 (B)` otherwise.
fn append_health(line: String, result: &RunResult) -> String {
    match &result.health {
        Some(h) => match &h.previous {
            Some(prev) => format!(
                "{line} | health {:.1} -> {:.1} ({})",
                prev.score, h.score, h.grade
            ),
            None => format!("{line} | health {:.1} ({})", h.score, h.grade),
        },
        None => line,
    }
}

fn diff_result_line(result: &RunResult) -> String {
    let net = result.findings.len() as i64 - result.fixed.len() as i64;
    let base = append_health(
        format!(
            "result: {} new, {} fixed, net {net:+}",
            result.findings.len(),
            result.fixed.len()
        ),
        result,
    );
    let with_baseline = match &result.baseline {
        Some(b) => format!("{base} | baseline {} acknowledged", b.acknowledged),
        None => base,
    };
    append_suppressed(with_baseline, result)
}

fn header(result: &RunResult) -> String {
    format!(
        "kndo {KNDO_VERSION} agent-format {AGENT_FORMAT_VERSION} | mode {} | cache {} | {}ms",
        result.mode,
        result.cache_status(),
        result.duration_ms
    )
}

fn result_line(result: &RunResult) -> String {
    let findings = append_health(
        if result.findings.is_empty() {
            "result: clean".to_string()
        } else {
            format!("result: {} findings", result.findings.len())
        },
        result,
    );
    let with_baseline = match &result.baseline {
        Some(b) => format!("{findings} | baseline {} acknowledged", b.acknowledged),
        None => findings,
    };
    append_suppressed(with_baseline, result)
}

/// `suppressed` is always present (unlike `baseline`), but only appended when non-zero — a
/// silent `| suppressed 0` on every clean run would just be token noise for an agent consumer.
fn append_suppressed(line: String, result: &RunResult) -> String {
    let total = result.suppressed.inline + result.suppressed.config;
    if total == 0 {
        line
    } else {
        format!(
            "{line} | suppressed {} inline, {} config",
            result.suppressed.inline, result.suppressed.config
        )
    }
}

fn ordered_groups(findings: &[Finding]) -> Vec<&str> {
    let mut groups: Vec<&str> = findings
        .iter()
        .map(|f| f.group.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    groups.sort_by_key(|g| {
        GROUP_ORDER
            .iter()
            .position(|k| k == g)
            .unwrap_or(GROUP_ORDER.len())
    });
    groups
}

fn sort_findings(findings: &mut [&Finding]) {
    findings.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then_with(|| path_key(a).cmp(path_key(b)))
            .then_with(|| span_key(a).cmp(&span_key(b)))
    });
}

fn path_key(f: &Finding) -> &str {
    f.location.path.as_ref().map(|p| p.0.as_str()).unwrap_or("")
}

fn span_key(f: &Finding) -> (u32, u32) {
    f.location.range.map(|r| r.start).unwrap_or((0, 0))
}

/// `N. [id] <category> <subject_kind> <path:line> <name> [(confidence)]` — output-schema §9's
/// literal grammar (space-separated, not `category:subject`; that colon form is RFC 0009's
/// human-rendering convention, a different renderer with different column economics).
fn finding_line(n: usize, f: &Finding) -> String {
    let path = match (&f.location.path, f.location.range) {
        (Some(p), Some(range)) => format!("{}:{}", p.0, range.start.0),
        (Some(p), None) => p.0.to_string(),
        (None, _) => "-".to_string(),
    };
    let name = f.location.symbol.as_deref().unwrap_or("-");
    let confidence = if f.confidence == Confidence::Certain {
        String::new()
    } else {
        format!(" ({})", confidence_str(f.confidence))
    };
    format!(
        "{n}. [{}] {} {} {path} {name}{confidence}",
        f.id, f.category, f.subject_kind
    )
}

/// `related` evidence, one indented line per entry — same information as the JSON's
/// `related[]`, nothing more (output-schema §9's carry rule).
fn push_evidence(out: &mut String, f: &Finding) {
    for r in &f.related {
        let location = match r.range {
            Some(range) => format!("{}:{}", r.path.0, range.start.0),
            None => r.path.0.to_string(),
        };
        match &r.note {
            Some(note) => out.push_str(&format!("   evidence: {location} {note}\n")),
            None => out.push_str(&format!("   evidence: {location}\n")),
        }
    }
}

fn confidence_str(c: Confidence) -> &'static str {
    match c {
        Confidence::Certain => "certain",
        Confidence::Probable => "probable",
        Confidence::Possible => "possible",
    }
}

/// Navigation verbs (RFC 0007), same grammar family: a header/status pair first, one block per
/// `results[]` entry (numbered only when the request batched more than one selector), the same
/// `more:`/`next:` discipline closing every response.
pub fn render_query(result: &QueryResult) -> String {
    let mut out = format!(
        "kndo {KNDO_VERSION} agent-format {AGENT_FORMAT_VERSION} | verb {} | cache {} | {}ms\n",
        result.verb.as_str(),
        result.cache,
        result.duration_ms
    );
    out.push_str(&format!("status: {}\n\n", result.status()));

    let batched = result.results.len() > 1;
    for (i, entry) in result.results.iter().enumerate() {
        if batched {
            let selector = result.selectors.get(i).map(String::as_str).unwrap_or("?");
            out.push_str(&format!("[{}] {selector}\n", i + 1));
        }
        render_query_entry(&mut out, entry);
        out.push('\n');
    }

    out.push_str("more: none\n");
    out.push_str(&format!(
        "next: kndo {} --format json\n",
        result.verb.as_str()
    ));
    out
}

fn render_query_entry(out: &mut String, entry: &ResultEntry) {
    match entry {
        ResultEntry::Failed {
            status,
            selector,
            message,
        } => {
            out.push_str(&format!("{status}: {selector} — {message}\n"));
        }
        ResultEntry::Find(r) => {
            for (n, m) in r.matches.iter().enumerate() {
                out.push_str(&format!("{}. {}\n", n + 1, node_line(m)));
            }
            if r.elided > 0 {
                out.push_str(&format!("more: {} elided\n", r.elided));
            }
        }
        ResultEntry::Describe(d) => {
            out.push_str(&format!("node: {}\n", node_line(&d.node)));
            if let Some(decl) = &d.declaration {
                out.push_str(&format!(
                    "declaration: {} visibility={}{}\n",
                    decl.kind,
                    decl.visibility,
                    if decl.exported { " exported" } else { "" }
                ));
            }
            if let Some(file) = &d.file {
                out.push_str(&format!(
                    "file: role={} origin={}\n",
                    file.role, file.origin
                ));
            }
            if let Some(dep) = &d.dependency {
                out.push_str(&format!(
                    "dependency: scopes={} importing_files={} {}\n",
                    dep.manifest_scopes.join(","),
                    dep.importing_files,
                    if dep.used { "used" } else { "unused" }
                ));
            }
            if let Some(pkg) = &d.package {
                out.push_str(&format!(
                    "package: mode={} files={} dependents={}\n",
                    pkg.mode, pkg.files, pkg.dependents
                ));
            }
            out.push_str(&format!(
                "degree: in={} out={}\n",
                sum_degree(&d.degree.in_by_kind),
                sum_degree(&d.degree.out_by_kind)
            ));
            if !d.reached_by_roots.is_empty() {
                out.push_str("reached_by_roots:\n");
                for (n, r) in d.reached_by_roots.iter().enumerate() {
                    out.push_str(&format!("  {}. {}\n", n + 1, node_line(r)));
                }
            }
            if !d.declared_symbols.is_empty() {
                out.push_str("declared_symbols:\n");
                for (n, s) in d.declared_symbols.iter().enumerate() {
                    out.push_str(&format!("  {}. {}\n", n + 1, node_line(s)));
                }
            }
            if !d.findings.is_empty() {
                out.push_str(&format!("findings: {}\n", d.findings.join(", ")));
            }
            if !d.sources.is_empty() {
                out.push_str(&format!("sources: {}\n", d.sources.join(", ")));
            }
        }
        ResultEntry::Neighbors(r) => {
            out.push_str(&format!("node: {}\n", node_line(&r.node)));
            for (n, e) in r.entries.iter().enumerate() {
                out.push_str(&format!("{}. {}\n", n + 1, neighbor_line(e)));
            }
            if r.elided > 0 {
                out.push_str(&format!("more: {} elided\n", r.elided));
            }
        }
        ResultEntry::Impact(r) => {
            out.push_str(&format!("node: {}\n", node_line(&r.node)));
            out.push_str(&format!(
                "affected: {} (production={} test-only={} tooling-only={} unreachable={})\n",
                r.affected.len() + r.elided,
                r.by_color.production,
                r.by_color.test_only,
                r.by_color.tooling_only,
                r.by_color.unreachable
            ));
            for (n, e) in r.affected.iter().enumerate() {
                out.push_str(&format!("{}. {}\n", n + 1, neighbor_line(e)));
            }
            if r.elided > 0 {
                out.push_str(&format!("more: {} elided\n", r.elided));
            }
            if !r.affected_roots.is_empty() {
                out.push_str("affected_roots:\n");
                for (n, root) in r.affected_roots.iter().enumerate() {
                    out.push_str(&format!(
                        "  {}. [{}] {}\n",
                        n + 1,
                        root.kind,
                        node_line(&root.node)
                    ));
                }
                if r.affected_roots_elided > 0 {
                    out.push_str(&format!("  more: {} elided\n", r.affected_roots_elided));
                }
            }
            if let Some(sim) = &r.if_deleted {
                out.push_str("if_deleted:\n");
                out.push_str(&format!(
                    "  newly_unreachable: {}\n",
                    sim.newly_unreachable.len() + sim.newly_unreachable_elided
                ));
                for (n, q) in sim.newly_unreachable.iter().enumerate() {
                    out.push_str(&format!("    {}. {}\n", n + 1, node_line(q)));
                }
                out.push_str(&format!(
                    "  newly_test_only: {}\n",
                    sim.newly_test_only.len() + sim.newly_test_only_elided
                ));
                for (n, q) in sim.newly_test_only.iter().enumerate() {
                    out.push_str(&format!("    {}. {}\n", n + 1, node_line(q)));
                }
                if !sim.freed_dependencies.is_empty() {
                    out.push_str(&format!(
                        "  freed_dependencies: {}\n",
                        sim.freed_dependencies.join(", ")
                    ));
                }
            }
        }
        ResultEntry::Trace(r) => {
            out.push_str(&format!(
                "from: {} to: {}\n",
                node_line(&r.from),
                node_line(&r.to)
            ));
            if r.paths.is_empty() {
                out.push_str("no path\n");
            }
            for (n, path) in r.paths.iter().enumerate() {
                out.push_str(&format!("path {}: {}", n + 1, node_line(&r.from)));
                for hop in &path.hops {
                    out.push_str(&format!(
                        " -[{}, {}]-> {}",
                        hop.via.edge,
                        confidence_str(hop.via.confidence),
                        node_line(&hop.node)
                    ));
                }
                out.push('\n');
            }
            if r.paths_elided > 0 {
                out.push_str(&format!("more: {} elided\n", r.paths_elided));
            }
        }
    }
}

fn node_line(n: &QNodeRef) -> String {
    let loc = match &n.span {
        Some(s) => format!(" {}:{}", s.path, s.start.0),
        None => String::new(),
    };
    format!("[{}] {}{loc}", n.selector, n.kind)
}

fn neighbor_line(e: &NeighborEntry) -> String {
    format!(
        "{} via {} (depth {})",
        node_line(&e.node),
        e.via.edge,
        e.depth
    )
}

fn sum_degree(m: &std::collections::HashMap<String, usize>) -> usize {
    m.values().sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ProjectPath;
    use crate::engine::{DeltaOrigin, Location, Severity};
    use smol_str::SmolStr;

    fn finding(category: &str, group: &str, subject_kind: &str, severity: Severity) -> Finding {
        Finding {
            id: format!("kndo-{category}"),
            category: category.to_string(),
            group: group.to_string(),
            subject_kind: subject_kind.to_string(),
            severity,
            confidence: Confidence::Certain,
            message: "example".to_string(),
            location: Location {
                path: Some(ProjectPath(SmolStr::new("src/a.ts"))),
                range: None,
                symbol: Some("thing".to_string()),
                package: None,
            },
            related: Vec::new(),
            delta: None,
            delta_origin: None,
        }
    }

    fn result(findings: Vec<Finding>) -> RunResult {
        RunResult {
            mode: "full".to_string(),
            duration_ms: 42,
            findings,
            ..RunResult::default()
        }
    }

    #[test]
    fn diff_mode_splits_new_and_fixed_with_one_running_number_sequence() {
        let mut introduced = finding("unused", "waste", "function", Severity::Warning);
        introduced.delta = Some(crate::engine::Delta::New);
        introduced.delta_origin = Some(DeltaOrigin::Introduced);

        let mut fixed = finding("test-only", "waste", "function", Severity::Info);
        fixed.delta = Some(crate::engine::Delta::Fixed);

        let out = render(&RunResult {
            mode: "staged".to_string(),
            duration_ms: 7,
            findings: vec![introduced],
            fixed: vec![fixed],
            ..RunResult::default()
        });

        assert!(out.contains("result: 1 new, 1 fixed, net +0"));
        assert!(out.contains("new:\n1. [kndo-unused]"));
        assert!(out.contains("fixed:\n2. [kndo-test-only]"));
        assert!(!out.contains("findings:"));
    }

    #[test]
    fn diff_mode_with_nothing_changed_omits_both_blocks() {
        let out = render(&RunResult {
            mode: "diff".to_string(),
            base_ref: Some("main".to_string()),
            duration_ms: 7,
            ..RunResult::default()
        });
        assert!(out.contains("result: 0 new, 0 fixed, net +0"));
        assert!(!out.contains("new:"));
        assert!(!out.contains("fixed:"));
    }

    #[test]
    fn clean_run_has_no_findings_block() {
        let out = render(&result(vec![]));
        assert!(out.contains("result: clean"));
        assert!(!out.contains("findings:"));
        assert!(out.contains("more: none\n"));
        assert!(out.contains("next: kndo check --format json\n"));
        assert!(out.contains(&format!("agent-format {AGENT_FORMAT_VERSION}")));
        assert!(out.contains("mode full"));
        assert!(out.contains("cache cold"));
        assert!(out.contains("42ms"));
    }

    #[test]
    fn baseline_summary_appends_to_the_result_line_when_present() {
        let mut r = result(vec![]);
        r.baseline = Some(crate::engine::BaselineSummary {
            acknowledged: 412,
            stale: 3,
        });
        let out = render(&r);
        assert!(out.contains("result: clean | baseline 412 acknowledged"));
    }

    #[test]
    fn no_baseline_line_when_no_baseline_exists() {
        let out = render(&result(vec![]));
        assert!(!out.contains("baseline"));
    }

    #[test]
    fn findings_render_numbered_in_group_order() {
        let out = render(&result(vec![
            finding("unused", "waste", "function", Severity::Warning),
            finding("unresolved", "defect", "import", Severity::Error),
        ]));
        assert!(out.contains("result: 2 findings"));
        let unresolved_line = out.lines().find(|l| l.contains("unresolved")).unwrap();
        let unused_line = out.lines().find(|l| l.contains("unused")).unwrap();
        assert!(unresolved_line.starts_with("1. "), "{unresolved_line}");
        assert!(unused_line.starts_with("2. "), "{unused_line}");
    }

    #[test]
    fn finding_line_uses_space_separated_grammar_not_colon_form() {
        let f = finding("unused", "waste", "function", Severity::Warning);
        let line = finding_line(1, &f);
        assert_eq!(line, "1. [kndo-unused] unused function src/a.ts thing");
    }

    #[test]
    fn missing_name_falls_back_to_dash() {
        let mut f = finding("duplicate", "waste", "file", Severity::Info);
        f.location.symbol = None;
        f.location.path = None;
        let line = finding_line(1, &f);
        assert_eq!(line, "1. [kndo-duplicate] duplicate file - -");
    }

    #[test]
    fn sub_certain_confidence_is_appended() {
        let mut f = finding("unused", "waste", "function", Severity::Warning);
        f.confidence = Confidence::Probable;
        let line = finding_line(1, &f);
        assert!(line.ends_with(" (probable)"));
    }

    #[test]
    fn no_ansi_or_glyphs_anywhere() {
        let out = render(&result(vec![finding(
            "unused",
            "waste",
            "function",
            Severity::Warning,
        )]));
        assert!(!out.contains('\x1b'));
        assert!(out.is_ascii());
    }

    #[test]
    fn zero_suppressed_is_omitted_from_the_result_line() {
        let out = render(&result(vec![]));
        assert!(!out.contains("suppressed"));
    }

    #[test]
    fn nonzero_suppressed_is_appended_to_the_result_line() {
        let mut r = result(vec![]);
        r.suppressed = crate::engine::SuppressedSummary {
            inline: 5,
            config: 2,
        };
        let out = render(&r);
        assert!(out.contains("result: clean | suppressed 5 inline, 2 config"));
    }

    #[test]
    fn nonzero_suppressed_is_appended_to_the_diff_result_line() {
        let out = render(&RunResult {
            mode: "staged".to_string(),
            duration_ms: 7,
            suppressed: crate::engine::SuppressedSummary {
                inline: 1,
                config: 0,
            },
            ..RunResult::default()
        });
        assert!(out.contains("result: 0 new, 0 fixed, net +0 | suppressed 1 inline, 0 config"));
    }

    #[test]
    fn describe_agent_output_shows_declaration_and_dependency_blocks() {
        use crate::query::{
            DeclarationInfo, Degree, DependencyInfo, DescribeResult, NodeSpan, QNodeRef,
        };
        use crate::query_envelope::{QueryResult, ResultEntry, Verb};

        let node = |selector: &str, kind: &str| QNodeRef {
            selector: selector.to_string(),
            kind: kind.to_string(),
            color: Some("production".to_string()),
            span: None,
        };
        let query_result = |entry: ResultEntry| QueryResult {
            verb: Verb::Describe,
            selectors: vec!["x".to_string()],
            id: None,
            cache: "warm",
            duration_ms: 3,
            results: vec![entry],
            diagnostics: vec![],
        };

        let symbol = query_result(ResultEntry::Describe(Box::new(DescribeResult {
            node: node("src/billing.js#computeTotal", "function"),
            declaration: Some(DeclarationInfo {
                kind: "function".to_string(),
                span: NodeSpan {
                    path: "src/billing.js".to_string(),
                    start: (1, 1),
                    end: (3, 2),
                },
                exported: true,
                visibility: 1,
            }),
            file: None,
            dependency: None,
            package: None,
            degree: Degree::default(),
            reached_by_roots: vec![],
            findings: vec![],
            sources: vec![],
            declared_symbols: vec![],
            elided: Default::default(),
        })));
        let out = render_query(&symbol);
        assert!(
            out.contains("declaration: function visibility=1 exported"),
            "{out}"
        );

        let dep = query_result(ResultEntry::Describe(Box::new(DescribeResult {
            node: node("dep:left-pad", "dependency"),
            declaration: None,
            file: None,
            dependency: Some(DependencyInfo {
                manifest_scopes: vec!["prod".to_string()],
                importing_files: 1,
                used: true,
            }),
            package: None,
            degree: Degree::default(),
            reached_by_roots: vec![],
            findings: vec![],
            sources: vec![],
            declared_symbols: vec![],
            elided: Default::default(),
        })));
        let out = render_query(&dep);
        assert!(
            out.contains("dependency: scopes=prod importing_files=1 used"),
            "{out}"
        );
    }
}
