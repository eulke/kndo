//! `--format agent` — a token-frugal, line-oriented plain-text rendering of [`RunResult`],
//! optimized for LLM context windows. Renders core-side, like
//! JSON: every frontend — CLI today, `kndo serve`/MCP tomorrow —
//! emits byte-identical agent text, never reconstructed per-frontend.
//!
//! Diff modes render `new:`/`fixed:` blocks instead of `findings:` (the schema's own
//! example), with one numbering sequence running across both. No `budget:` line yet — health
//! scoring landed in M4 and is rendered below; what is still missing is the `[delta]` half,
//! whose rules nothing parses. Findings carrying a `related`
//! evidence chain (populated by `cyclic`) render it as indented `evidence:` lines — the
//! format "can never carry information absent from the JSON", and `related`
//! IS in the JSON. `remediation` isn't — no `fix:` lines. Diff mode's NEW findings
//! aren't visually split by `delta_origin` here; the schema's own example shows one flat
//! `new:` block, `delta_origin` traveling on each line's JSON-equivalent data only.
//! `next:` names only commands that work today (`--format json`). `kndo explain` doesn't
//! exist (see `engine::Finding`'s doc on `related`/`remediation`), so it's not offered
//! as if it did; the navigation verbs (`kndo used-by`, …) do, via [`render_query`].
//!
//! Suppressed findings are never listed here either — matching, marked findings are already
//! absent from `RunResult.findings`/`fixed` by the time this module sees them — only appended to
//! the result line as `| suppressed N inline, M config`, and only when non-zero.
//!
//! [`render_query`] renders the navigation-verb envelope ("numbered entries of
//! `[selector] kind path:line` plus the verb's specifics … same `more:`/`next:` discipline").

use crate::engine::{Finding, RunResult, KNDO_VERSION};
use crate::query::NeighborEntry;
use crate::query_envelope::{QueryResult, ResultEntry};
use crate::vocab::Confidence;

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
            crate::engine::sort_findings_for_display(&mut in_group);
            for f in in_group {
                n += 1;
                out.push_str(&finding_line(n, f));
                out.push('\n');
                push_evidence(&mut out, f);
            }
        }
    }

    // Elision is always explicit (extended here by the same principle): this
    // renderer never caps or paginates, so it is always "none" — but the line is never
    // skipped, so a reader never has to guess whether truncation happened silently.
    out.push_str("more: none\n");
    out.push_str("next: kndo check --format json\n");
    out
}

/// Diff modes' rendering — the schema's own example: `new:`/`fixed:` blocks sharing one
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
            crate::engine::sort_findings_for_display(&mut in_group);
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
            crate::engine::sort_findings_for_display(&mut in_group);
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

/// Output-schema the result-line health segment: `health 82.4 -> 84.1 (B)` when a previous
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

fn ordered_groups(findings: &[Finding]) -> Vec<crate::vocab::Group> {
    let present: std::collections::BTreeSet<crate::vocab::Group> =
        findings.iter().map(|f| f.group).collect();
    crate::vocab::Group::DISPLAY_ORDER
        .into_iter()
        .filter(|g| present.contains(g))
        .collect()
}

/// `N. [id] <category> <subject_kind> <path:line> <name> [(confidence)]` — the agent format's
/// literal grammar (space-separated, not `category:subject`; that colon form is the
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
/// `related[]`, nothing more (output-schema the carry rule).
fn push_evidence(out: &mut String, f: &Finding) {
    for r in &f.related {
        out.push_str(&r.render("   evidence: ", " "));
        out.push('\n');
    }
}

fn confidence_str(c: Confidence) -> &'static str {
    match c {
        Confidence::Certain => "certain",
        Confidence::Probable => "probable",
        Confidence::Possible => "possible",
    }
}

/// Navigation verbs, same grammar family: a header/status pair first, one block per
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
                out.push_str(&format!("{}. {}\n", n + 1, m));
            }
            if r.elided > 0 {
                out.push_str(&format!("more: {} elided\n", r.elided));
            }
        }
        ResultEntry::Describe(d) => {
            out.push_str(&format!("node: {}\n", d.node));
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
            for m in &d.metrics {
                // `coverage=unmeasured`/`crap=unmeasured`, never a fabricated 0: an absent
                // report means nobody measured, which is not the same as "none covered".
                out.push_str(&format!(
                    "metrics: shape={} line={} cyclomatic={} loc={} tokens={} coverage={} crap={}\n",
                    m.shape_ordinal,
                    m.span.start.0,
                    m.cyclomatic,
                    m.loc,
                    m.token_count,
                    m.coverage
                        .map(|c| format!("{c:.2}"))
                        .unwrap_or_else(|| "unmeasured".to_string()),
                    m.crap
                        .map(|c| format!("{c:.1}"))
                        .unwrap_or_else(|| "unmeasured".to_string()),
                ));
            }
            for g in &d.duplication {
                out.push_str(&format!(
                    "duplication: finding={} members={}\n",
                    g.finding,
                    g.members.len()
                ));
                for (n, m) in g.members.iter().enumerate() {
                    out.push_str(&format!("  {}. {}\n", n + 1, m));
                }
            }
            out.push_str(&format!(
                "degree: in={} out={}\n",
                sum_degree(&d.degree.in_by_kind),
                sum_degree(&d.degree.out_by_kind)
            ));
            if !d.reached_by_roots.is_empty() {
                out.push_str("reached_by_roots:\n");
                for (n, r) in d.reached_by_roots.iter().enumerate() {
                    out.push_str(&format!("  {}. {}\n", n + 1, r));
                }
            }
            if !d.declared_symbols.is_empty() {
                out.push_str("declared_symbols:\n");
                for (n, s) in d.declared_symbols.iter().enumerate() {
                    out.push_str(&format!("  {}. {}\n", n + 1, s));
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
            out.push_str(&format!("node: {}\n", r.node));
            for (n, e) in r.entries.iter().enumerate() {
                out.push_str(&format!("{}. {}\n", n + 1, neighbor_line(e)));
            }
            if r.elided > 0 {
                out.push_str(&format!("more: {} elided\n", r.elided));
            }
        }
        ResultEntry::Impact(r) => {
            out.push_str(&format!("node: {}\n", r.node));
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
                    out.push_str(&format!("  {}. [{}] {}\n", n + 1, root.kind, root.node));
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
                    out.push_str(&format!("    {}. {}\n", n + 1, q));
                }
                out.push_str(&format!(
                    "  newly_test_only: {}\n",
                    sim.newly_test_only.len() + sim.newly_test_only_elided
                ));
                for (n, q) in sim.newly_test_only.iter().enumerate() {
                    out.push_str(&format!("    {}. {}\n", n + 1, q));
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
            out.push_str(&format!("from: {} to: {}\n", r.from, r.to));
            if r.paths.is_empty() {
                out.push_str("no path\n");
            }
            for (n, path) in r.paths.iter().enumerate() {
                out.push_str(&format!("path {}: {}", n + 1, r.from));
                for hop in &path.hops {
                    out.push_str(&format!(
                        " -[{}, {}]-> {}",
                        hop.via.edge,
                        confidence_str(hop.via.confidence),
                        hop.node
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

fn neighbor_line(e: &NeighborEntry) -> String {
    format!("{} via {} (depth {})", e.node, e.via.edge, e.depth)
}

fn sum_degree(m: &rustc_hash::FxHashMap<String, usize>) -> usize {
    m.values().sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ProjectPath;
    use crate::engine::{DeltaOrigin, Location, Severity};
    use smol_str::SmolStr;

    fn parse_group(g: &str) -> crate::vocab::Group {
        match g {
            "defect" => crate::vocab::Group::Defect,
            "waste" => crate::vocab::Group::Waste,
            "risk" => crate::vocab::Group::Risk,
            "hygiene" => crate::vocab::Group::Hygiene,
            "convention" => crate::vocab::Group::Convention,
            other => panic!("unknown test group {other}"),
        }
    }

    fn finding(category: &str, group: &str, subject_kind: &str, severity: Severity) -> Finding {
        Finding {
            advisory: false,
            id: format!("kndo-{category}"),
            category: crate::vocab::Category::new(category),
            group: parse_group(group),
            subject_kind: crate::vocab::SubjectKind::new(subject_kind),
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
            metrics: Vec::new(),
            duplication: Vec::new(),
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
            metrics: Vec::new(),
            duplication: Vec::new(),
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
