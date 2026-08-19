//! `--format agent` — a token-frugal, line-oriented plain-text rendering of [`RunResult`],
//! optimized for LLM context windows (contracts/output-schema.md §9). Renders core-side, like
//! JSON (RFC 0001 §2, contracts §5): every frontend — CLI today, `kndo serve`/MCP tomorrow —
//! emits byte-identical agent text, never reconstructed per-frontend.
//!
//! Scope for this increment: full-mode rendering only — a `findings:` block, not diff mode's
//! `new:`/`fixed:` split (no diff mode exists yet, M2), so no `budget:` line either (delta
//! budgets are diff-mode-only). No `cause:`/`fix:` evidence lines under a finding: the format
//! "can never carry information absent from the JSON" (output-schema §9), and `RunResult`
//! doesn't carry `related`/`remediation` data yet (deferred, see `engine::Finding`'s doc).
//! `next:` names only commands that work today (`--format json`) — the navigation verbs
//! (`kndo explain`, `kndo used-by`, …) don't exist yet (RFC 0007), so they aren't offered as
//! if they did.

use crate::engine::{Finding, RunResult, KNDO_VERSION};
use crate::vocab::Confidence;

const GROUP_ORDER: [&str; 4] = ["defect", "waste", "risk", "hygiene"];
const AGENT_FORMAT_VERSION: u32 = 1;

pub fn render(result: &RunResult) -> String {
    let mut out = String::new();
    out.push_str(&header(result));
    out.push('\n');
    out.push_str(&result_line(result));
    out.push('\n');

    if !result.findings.is_empty() {
        out.push_str("findings:\n");
        let mut n = 0usize;
        for group in ordered_groups(result) {
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

fn header(result: &RunResult) -> String {
    format!(
        "kndo {KNDO_VERSION} agent-format {AGENT_FORMAT_VERSION} | mode {} | cache cold | {}ms",
        result.mode, result.duration_ms
    )
}

fn result_line(result: &RunResult) -> String {
    if result.findings.is_empty() {
        "result: clean".to_string()
    } else {
        format!("result: {} findings", result.findings.len())
    }
}

fn ordered_groups(result: &RunResult) -> Vec<&str> {
    let mut groups: Vec<&str> = result
        .findings
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

fn confidence_str(c: Confidence) -> &'static str {
    match c {
        Confidence::Certain => "certain",
        Confidence::Probable => "probable",
        Confidence::Possible => "possible",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ProjectPath;
    use crate::engine::{Location, Severity};
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
}
