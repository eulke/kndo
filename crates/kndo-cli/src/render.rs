//! Human rendering (RFC 0009) — everything a human sees in the terminal. Binds only this
//! frontend (contracts §5): the core returns data (`RunResult`), this renders it; the CLI is
//! pure presentation.
//!
//! Implemented: group sections in fixed order (defect, waste, risk, hygiene, then any
//! additive/future group RFC 0005 doesn't know about yet), one line per finding
//! (`<glyph> <category>[:<subject>] <path:line> <message> [(confidence)] [id]`), the
//! quiet-success line, semantic per-group color.
//!
//! Deliberately not implemented, simplified rather than silently wrong:
//! - RFC 0009 §4's three-tier capability ladder (rich TTY / basic TTY / no TTY / `TERM=dumb`)
//!   collapses to one on/off switch (`RenderOptions::color`) driving *both* color and glyph
//!   richness — real terminals vary more than that, but this never renders something
//!   unreadable, only less decorated than the richest tier could be.
//! - Width-based column truncation and the below-60-columns two-line fallback (§4) — lines
//!   are never truncated here.
//! - The health block and diff-mode NEW/FIXED sections (§5) — no health scoring (M4) or diff
//!   mode (M2) exists yet to render.

use kndo::engine::{Finding, RunResult};
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

const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";
const BLUE: &str = "\x1b[34m";
const RESET: &str = "\x1b[0m";

pub fn render(result: &RunResult, opts: &RenderOptions) -> String {
    let baseline_suffix = baseline_suffix(result);

    if result.findings.is_empty() {
        return format!(
            "kndo · clean · {} files ({} claimed, {} symbols, {} deps, {} edges) · {}ms{baseline_suffix}\n",
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
            "kndo · {} findings{baseline_suffix}\n",
            result.findings.len()
        );
    }

    let mut out = String::new();
    if let Some(b) = &result.baseline {
        out.push_str(&format!("baseline: {} acknowledged\n\n", b.acknowledged));
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
