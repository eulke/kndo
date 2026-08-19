//! Human rendering (RFC 0009) — everything a human sees in the terminal. Binds only this
//! frontend (contracts §5): the core returns data (`RunResult`), this renders it; the CLI is
//! pure presentation.
//!
//! Implemented: group sections in fixed order (defect, waste, risk, hygiene, then any
//! additive/future group RFC 0005 doesn't know about yet), one line per finding
//! (`<glyph> <category>[:<subject>] <path:line> <message> [(confidence)] [id]`), the
//! quiet-success line, semantic per-group color.
//!
//! Diff modes (`--staged`/`--diff`) render a NEW/FIXED split instead (RFC 0006 §3): NEW splits
//! further by `delta_origin` (introduced vs derived), FIXED is flat, and the header states the
//! net.
//!
//! Suppressed findings (inline `kndo:allow` pragmas, contracts §2.1) are never listed — matched
//! findings are marked, not deleted, so they're already absent from `RunResult.findings` by the
//! time this module sees it; only the count surfaces, in the header suffix and (full mode,
//! non-quiet) a `suppressed: N inline, M config` line, and only when non-zero.
//!
//! Deliberately not implemented, simplified rather than silently wrong:
//! - RFC 0009 §4's three-tier capability ladder (rich TTY / basic TTY / no TTY / `TERM=dumb`)
//!   collapses to one on/off switch (`RenderOptions::color`) driving *both* color and glyph
//!   richness — real terminals vary more than that, but this never renders something
//!   unreadable, only less decorated than the richest tier could be.
//! - Width-based column truncation and the below-60-columns two-line fallback (§4) — lines
//!   are never truncated here.
//! - The health/budget block (§5) — no health scoring exists yet (M4), so there's nothing to
//!   render; the diff header's `net` count is the only summary today.
//! - The `└ cause: …` evidence line under a derived finding — `RunResult`/`Finding` don't carry
//!   the `related` evidence chain yet (deferred, see `engine::Finding`'s doc), so there is
//!   nothing to render; `delta_origin` alone is still shown.

use kndo::engine::{DeltaOrigin, Finding, RunResult};
use kndo::vocab::Confidence;

pub struct RenderOptions {
    pub color: bool,
    pub quiet: bool,
}

const GROUP_ORDER: [&str; 4] = ["defect", "waste", "risk", "hygiene"];

fn baseline_suffix(result: &RunResult) -> String {
    match &result.baseline {
        Some(b) => format!(" · baseline: {} acknowledged", b.acknowledged),
        None => String::new(),
    }
}

/// Only shown when something is actually suppressed — `suppressed` is always present
/// (unlike `baseline`), but a silent `· suppressed: 0` on every clean run would be noise.
fn suppressed_suffix(result: &RunResult) -> String {
    let total = result.suppressed.inline + result.suppressed.config;
    if total == 0 {
        String::new()
    } else {
        format!(" · suppressed: {total}")
    }
}

const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";
const BLUE: &str = "\x1b[34m";
const RESET: &str = "\x1b[0m";

pub fn render(result: &RunResult, opts: &RenderOptions) -> String {
    if result.mode == "staged" || result.mode == "diff" {
        return render_diff(result, opts);
    }

    let baseline_suffix = baseline_suffix(result);
    let suppressed_suffix = suppressed_suffix(result);

    if result.findings.is_empty() {
        return format!(
            "kndo · clean · {} files ({} claimed, {} symbols, {} deps, {} edges) · {}ms{baseline_suffix}{suppressed_suffix}\n",
            result.files_discovered,
            result.files_claimed,
            result.symbols,
            result.dependencies,
            result.edges,
            result.duration_ms
        );
    }

    if opts.quiet {
        return format!(
            "kndo · {} findings{baseline_suffix}{suppressed_suffix}\n",
            result.findings.len()
        );
    }

    let mut out = String::new();
    if let Some(b) = &result.baseline {
        out.push_str(&format!("baseline: {} acknowledged\n\n", b.acknowledged));
    }
    if result.suppressed.inline + result.suppressed.config > 0 {
        out.push_str(&format!(
            "suppressed: {} inline, {} config\n\n",
            result.suppressed.inline, result.suppressed.config
        ));
    }
    let mut groups: Vec<&str> = result
        .findings
        .iter()
        .map(|f| f.group.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    // Fixed triage order first (RFC 0005 rule 4), then any group the taxonomy doesn't name yet
    // — additive, so it must still render, just after the known ones (output-schema §6).
    groups.sort_by_key(|g| {
        GROUP_ORDER
            .iter()
            .position(|k| k == g)
            .unwrap_or(GROUP_ORDER.len())
    });

    for group in groups {
        let mut in_group: Vec<&Finding> = result
            .findings
            .iter()
            .filter(|f| f.group == group)
            .collect();
        sort_findings(&mut in_group);
        render_section(&mut out, group, &in_group, opts);
    }
    out
}

/// Diff modes' rendering (RFC 0006 §3): a one-line header (`N new · M fixed · net ±K`), then
/// `NEW (introduced by this change)`, `NEW (derived, in untouched code)`, and `FIXED` sections
/// — each present only when non-empty, in that fixed order, mirroring the RFC's own example.
fn render_diff(result: &RunResult, opts: &RenderOptions) -> String {
    let net = result.findings.len() as i64 - result.fixed.len() as i64;
    let baseline_suffix = baseline_suffix(result);
    let suppressed_suffix = suppressed_suffix(result);
    let header = format!(
        "kndo · {} · {} new · {} fixed · net {net:+}{baseline_suffix}{suppressed_suffix}\n",
        result.mode,
        result.findings.len(),
        result.fixed.len(),
    );

    if opts.quiet || (result.findings.is_empty() && result.fixed.is_empty()) {
        return header;
    }

    let mut out = header;
    out.push('\n');

    let introduced: Vec<&Finding> = result
        .findings
        .iter()
        .filter(|f| f.delta_origin == Some(DeltaOrigin::Introduced))
        .collect();
    let derived: Vec<&Finding> = result
        .findings
        .iter()
        .filter(|f| f.delta_origin == Some(DeltaOrigin::Derived))
        .collect();

    if !introduced.is_empty() {
        out.push_str("NEW (introduced by this change)\n");
        render_flat(&mut out, &introduced, opts);
    }
    if !derived.is_empty() {
        out.push_str("NEW (derived, in untouched code)\n");
        render_flat(&mut out, &derived, opts);
    }
    if !result.fixed.is_empty() {
        out.push_str("FIXED\n");
        let fixed: Vec<&Finding> = result.fixed.iter().collect();
        render_flat(&mut out, &fixed, opts);
    }
    out
}

/// One finding per line, sorted like every other section (worst severity, then path, then
/// span) but without the group header `render_section` prints — diff mode's sections are
/// `NEW`/`FIXED`, not the taxonomy groups.
fn render_flat(out: &mut String, findings: &[&Finding], opts: &RenderOptions) {
    let mut sorted = findings.to_vec();
    sort_findings(&mut sorted);
    for f in sorted {
        out.push_str("  ");
        out.push_str(&render_finding_line(f, opts));
        out.push('\n');
    }
    out.push('\n');
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

fn render_section(out: &mut String, group: &str, findings: &[&Finding], opts: &RenderOptions) {
    out.push_str(&format!("{} ({})\n", group.to_uppercase(), findings.len()));
    for f in findings {
        out.push_str("  ");
        out.push_str(&render_finding_line(f, opts));
        out.push('\n');
    }
    out.push('\n');
}

fn render_finding_line(f: &Finding, opts: &RenderOptions) -> String {
    let category = if f.subject_kind == "file" {
        f.category.clone()
    } else {
        format!("{}:{}", f.category, f.subject_kind)
    };
    let location = match (&f.location.path, f.location.range) {
        (Some(p), Some(range)) => format!("{}:{}", p.0, range.start.0),
        (Some(p), None) => p.0.to_string(),
        (None, _) => "-".to_string(),
    };
    let confidence = if f.confidence == Confidence::Certain {
        String::new()
    } else {
        format!(" ({})", confidence_str(f.confidence))
    };

    let glyph = glyph(&f.group, opts.color);
    if opts.color {
        format!(
            "{}{glyph}{RESET} {category} {location}  {}{confidence} [{}]",
            color_code(&f.group),
            f.message,
            f.id
        )
    } else {
        format!(
            "{glyph} {category} {location}  {}{confidence} [{}]",
            f.message, f.id
        )
    }
}

fn glyph(group: &str, rich: bool) -> &'static str {
    match (group, rich) {
        ("defect", true) => "✗",
        ("defect", false) => "x",
        ("waste", true) => "◦",
        ("waste", false) => "o",
        ("risk", true) => "▲",
        ("risk", false) => "^",
        ("hygiene", true) => "·",
        ("hygiene", false) => ".",
        (_, true) => "•",
        (_, false) => "?",
    }
}

fn color_code(group: &str) -> &'static str {
    match group {
        "defect" => RED,
        "waste" => YELLOW,
        "risk" => MAGENTA,
        "hygiene" => BLUE,
        _ => "",
    }
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
    use kndo::adapter::ProjectPath;
    use kndo::engine::{Delta, DeltaOrigin, Location, Severity};
    use smol_str::SmolStr;

    fn finding(category: &str, group: &str) -> Finding {
        Finding {
            id: format!("kndo-{category}"),
            category: category.to_string(),
            group: group.to_string(),
            subject_kind: "function".to_string(),
            severity: Severity::Warning,
            confidence: Confidence::Certain,
            message: "example".to_string(),
            location: Location {
                path: Some(ProjectPath(SmolStr::new("src/a.ts"))),
                range: None,
                symbol: Some("thing".to_string()),
                package: None,
            },
            delta: None,
            delta_origin: None,
        }
    }

    fn opts() -> RenderOptions {
        RenderOptions {
            color: false,
            quiet: false,
        }
    }

    #[test]
    fn diff_mode_splits_introduced_derived_and_fixed_sections() {
        let mut introduced = finding("unused", "waste");
        introduced.delta = Some(Delta::New);
        introduced.delta_origin = Some(DeltaOrigin::Introduced);

        let mut derived = finding("test-only", "waste");
        derived.delta = Some(Delta::New);
        derived.delta_origin = Some(DeltaOrigin::Derived);

        let mut fixed = finding("unused", "waste");
        fixed.delta = Some(Delta::Fixed);

        let result = RunResult {
            mode: "staged".to_string(),
            findings: vec![introduced, derived],
            fixed: vec![fixed],
            ..RunResult::default()
        };
        let out = render(&result, &opts());

        assert!(out.starts_with("kndo · staged · 2 new · 1 fixed · net +1\n"));
        let introduced_pos = out.find("NEW (introduced by this change)").unwrap();
        let derived_pos = out.find("NEW (derived, in untouched code)").unwrap();
        let fixed_pos = out.find("FIXED").unwrap();
        assert!(introduced_pos < derived_pos && derived_pos < fixed_pos);
    }

    #[test]
    fn diff_mode_clean_result_is_just_the_header() {
        let result = RunResult {
            mode: "diff".to_string(),
            base_ref: Some("main".to_string()),
            ..RunResult::default()
        };
        let out = render(&result, &opts());
        assert_eq!(out, "kndo · diff · 0 new · 0 fixed · net +0\n");
    }

    #[test]
    fn diff_mode_quiet_is_a_one_liner_even_with_findings() {
        let mut f = finding("unused", "waste");
        f.delta = Some(Delta::New);
        f.delta_origin = Some(DeltaOrigin::Introduced);
        let result = RunResult {
            mode: "staged".to_string(),
            findings: vec![f],
            ..RunResult::default()
        };
        let out = render(
            &result,
            &RenderOptions {
                color: false,
                quiet: true,
            },
        );
        assert_eq!(out, "kndo · staged · 1 new · 0 fixed · net +1\n");
    }

    #[test]
    fn full_mode_is_unaffected_by_the_diff_mode_branch() {
        let result = RunResult {
            mode: "full".to_string(),
            ..RunResult::default()
        };
        let out = render(&result, &opts());
        assert!(out.starts_with("kndo · clean ·"));
    }

    #[test]
    fn zero_suppressed_is_silent_everywhere() {
        let result = RunResult {
            mode: "full".to_string(),
            ..RunResult::default()
        };
        let out = render(&result, &opts());
        assert!(!out.contains("suppressed"));
    }

    #[test]
    fn nonzero_suppressed_shows_in_the_clean_full_mode_header() {
        let result = RunResult {
            mode: "full".to_string(),
            suppressed: kndo::engine::SuppressedSummary {
                inline: 3,
                config: 1,
            },
            ..RunResult::default()
        };
        let out = render(&result, &opts());
        assert!(out.starts_with("kndo · clean ·"));
        assert!(out.contains("suppressed: 4"));
    }

    #[test]
    fn nonzero_suppressed_gets_its_own_line_above_findings() {
        let result = RunResult {
            mode: "full".to_string(),
            findings: vec![finding("unused", "waste")],
            suppressed: kndo::engine::SuppressedSummary {
                inline: 2,
                config: 0,
            },
            ..RunResult::default()
        };
        let out = render(&result, &opts());
        assert!(out.contains("suppressed: 2 inline, 0 config\n\n"));
    }

    #[test]
    fn nonzero_suppressed_shows_in_the_diff_mode_header() {
        let result = RunResult {
            mode: "staged".to_string(),
            suppressed: kndo::engine::SuppressedSummary {
                inline: 1,
                config: 0,
            },
            ..RunResult::default()
        };
        let out = render(&result, &opts());
        assert_eq!(
            out,
            "kndo · staged · 0 new · 0 fixed · net +0 · suppressed: 1\n"
        );
    }
}
