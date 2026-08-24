//! Human rendering — everything a human sees in the terminal. Binds only this
//! frontend: the core returns data (`RunResult`), this renders it; the CLI is
//! pure presentation.
//!
//! Implemented: group sections in fixed order (defect, waste, risk, hygiene, then any
//! additive group the taxonomy doesn't name), one line per finding
//! (`<glyph> <category>[:<subject>] <path:line> <message> [(confidence)] [id]`), the
//! quiet-success line, semantic per-group color.
//!
//! Diff modes (`--staged`/`--diff`) render a NEW/FIXED split instead: NEW splits
//! further by `delta_origin` (introduced vs derived), FIXED is flat, and the header states the
//! net.
//!
//! Suppressed findings (inline `kndo:allow` pragmas) are never listed — matched
//! findings are marked, not deleted, so they're already absent from `RunResult.findings` by the
//! time this module sees it; only the count surfaces, in the header suffix and (full mode,
//! non-quiet) a `suppressed: N inline, M config` line, and only when non-zero.
//!
//! Deliberately not implemented, simplified rather than silently wrong:
//! - The three-tier terminal capability ladder (rich TTY / basic TTY / no TTY / `TERM=dumb`)
//!   collapses to one on/off switch (`RenderOptions::color`) driving *both* color and glyph
//!   richness — real terminals vary more than that, but this never renders something
//!   unreadable, only less decorated than the richest tier could be.
//! - Width-based column truncation and the below-60-columns two-line fallback — lines
//!   are never truncated here.
//! - The budget block — budgets need the config file's [delta] rules, which aren't
//!   wired here; the health half IS rendered: a score/grade line plus per-category
//!   penalty bars (non-zero categories only in `check` output; `kndo health` renders the full
//!   table), and diff mode's header carries the before ──▶ after health line with the
//!   grade-boundary distance on drops.
//! - Findings that carry a `related` evidence chain (first populated by `cyclic`) render it as
//!   indented `└` lines under the finding — role, location, note.

use kndo::analysis::health::Health;
use kndo::engine::{DeltaOrigin, Finding, RunResult};
use kndo::query::{NeighborEntry, QNodeRef};
use kndo::query_envelope::{QueryResult, ResultEntry};
use kndo::vocab::Confidence;

pub(crate) struct RenderOptions {
    pub(crate) color: bool,
    pub(crate) quiet: bool,
    /// Adds the per-phase timing block and cache state to check output. (The
    /// third `--verbose` effect — revealing `possible`-confidence findings — is inert:
    /// no renderer hides findings by confidence, so there is nothing to reveal.)
    pub(crate) verbose: bool,
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

pub(crate) fn render(result: &RunResult, opts: &RenderOptions) -> String {
    if result.mode == "staged" || result.mode == "diff" {
        return render_diff(result, opts);
    }

    let baseline_suffix = baseline_suffix(result);
    let suppressed_suffix = suppressed_suffix(result);

    if result.findings.is_empty() {
        let mut out = format!(
            "kndo · clean · {} files ({} claimed, {} symbols, {} deps, {} edges) · {}ms{baseline_suffix}{suppressed_suffix}\n",
            result.files_discovered,
            result.files_claimed,
            result.symbols,
            result.dependencies,
            result.edges,
            result.duration_ms
        );
        if !opts.quiet {
            if let Some(health) = &result.health {
                out.push_str(&health_score_line(health));
            }
            render_phases(&mut out, result, opts);
        }
        return out;
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
    // Fixed triage order first, then any group the taxonomy doesn't name
    // — additive, so it must still render, just after the known ones.
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
        kndo::engine::sort_findings_for_display(&mut in_group);
        render_section(&mut out, group, &in_group, opts);
    }
    if let Some(health) = &result.health {
        out.push_str(&render_health(health, opts, false));
    }
    render_phases(&mut out, result, opts);
    out
}

/// The `--verbose` block: one line per engine phase (`(phase, µs)` from
/// `RunResult::timings`), worst-first would hide the pipeline shape, so execution order is
/// kept; sub-0.1ms phases still print — "0.0ms" is honest and keeps the block's shape stable
/// across runs. Closes with the cache state (`enabled`/`disabled` + hits — the raw inputs
/// behind the header's warm/cold verdict).
fn render_phases(out: &mut String, result: &RunResult, opts: &RenderOptions) {
    if !opts.verbose || result.timings.is_empty() {
        return;
    }
    out.push_str("\nphases\n");
    for (phase, us) in &result.timings {
        out.push_str(&format!("  {:<22} {:>8.1}ms\n", phase, *us as f64 / 1000.0));
    }
    let total: u64 = result.timings.iter().map(|(_, us)| us).sum();
    out.push_str(&format!(
        "  {:<22} {:>8.1}ms\n",
        "total",
        total as f64 / 1000.0
    ));
    out.push_str(&format!(
        "cache: {}, {} hits\n",
        if result.cache_enabled {
            "enabled"
        } else {
            "disabled"
        },
        result.cache_hits
    ));
}

/// The health block: the score/grade (+trend) line, then one bar line per category —
/// every category when `full_table` (`kndo health`), only penalized ones inside `check`
/// output (a zero-penalty row is reassurance, not triage).
pub(crate) fn render_health(health: &Health, opts: &RenderOptions, full_table: bool) -> String {
    let mut out = health_score_line(health);
    for c in &health.categories {
        if !full_table && c.penalty == 0.0 {
            continue;
        }
        let mut extras: Vec<String> = Vec::new();
        if let Some(n) = c.count {
            extras.push(n.to_string());
        }
        if let Some(t) = c.tokens_duplicated {
            extras.push(format!("{t} tokens"));
        }
        if let Some(l) = c.crapload {
            extras.push(format!("load {l}"));
        }
        if let Some(cov) = &c.coverage {
            extras.push(format!("coverage {cov}"));
        }
        let extra = if extras.is_empty() {
            String::new()
        } else {
            format!("  ({})", extras.join(", "))
        };
        out.push_str(&format!(
            "  {:<20} {}  −{:.1}{extra}\n",
            c.category,
            penalty_bar(c.penalty, opts),
            c.penalty,
        ));
    }
    if full_table && !health.packages.is_empty() {
        out.push_str("\nby package:\n");
        for p in &health.packages {
            out.push_str(&format!(
                "  {:<24} {:>5.1}  {}\n",
                p.package, p.score, p.grade
            ));
        }
    }
    out
}

fn health_score_line(health: &Health) -> String {
    let trend = match &health.previous {
        Some(prev) if prev.score != health.score => {
            let delta = health.score - prev.score;
            let arrow = if delta > 0.0 { "↑" } else { "↓" };
            format!("   {delta:+.1} {arrow} from {:.1}", prev.score)
        }
        Some(_) => "   unchanged".to_string(),
        None => String::new(),
    };
    format!("health   {:.1}  {}{trend}\n", health.score, health.grade)
}

/// The penalty bar: a shape, not a chart — `▁▂▃▄▅▆▇` scaled against the heaviest weight
/// (25), or `#` repetition when decoration is off.
fn penalty_bar(penalty: f64, opts: &RenderOptions) -> String {
    let level = ((penalty / 25.0) * 7.0).ceil().clamp(0.0, 7.0) as usize;
    if opts.color {
        const GLYPHS: [&str; 8] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇"];
        GLYPHS[level].to_string()
    } else {
        format!("{:<7}", "#".repeat(level))
    }
}

/// Diff mode's health line: before ──▶ after with the signed delta, and on a drop the
/// distance to the next grade boundary ("how close is this to becoming a C").
fn health_diff_line(health: &Health) -> String {
    let Some(prev) = &health.previous else {
        return health_score_line(health);
    };
    let delta = health.score - prev.score;
    let arrow = if delta > 0.0 {
        "↑"
    } else if delta < 0.0 {
        "↓"
    } else {
        "="
    };
    let boundary = match delta < 0.0 {
        true => grade_boundary_suffix(health.score, &health.grade),
        false => String::new(),
    };
    format!(
        "health   {:.1} ──▶ {:.1}   {delta:+.1} {arrow}   {}{boundary}\n",
        prev.score, health.score, health.grade
    )
}

fn grade_boundary_suffix(score: f64, grade: &str) -> String {
    let (threshold, next) = match grade {
        "A" => (90.0, "B"),
        "B" => (80.0, "C"),
        "C" => (65.0, "D"),
        "D" => (50.0, "F"),
        _ => return String::new(),
    };
    format!("  ({:.1} from {next})", score - threshold)
}

/// Diff modes' rendering: a one-line header (`N new · M fixed · net ±K`), then
/// `NEW (introduced by this change)`, `NEW (derived, in untouched code)`, and `FIXED` sections
/// — each present only when non-empty, in that fixed order.
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

    let health_line = result
        .health
        .as_ref()
        .map(health_diff_line)
        .unwrap_or_default();

    if opts.quiet || (result.findings.is_empty() && result.fixed.is_empty()) {
        return format!("{header}{health_line}");
    }

    let mut out = format!("{header}{health_line}");
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
    render_phases(&mut out, result, opts);
    out
}

/// One finding per line, sorted like every other section (worst severity, then path, then
/// span) but without the group header `render_section` prints — diff mode's sections are
/// `NEW`/`FIXED`, not the taxonomy groups.
fn render_flat(out: &mut String, findings: &[&Finding], opts: &RenderOptions) {
    let mut sorted = findings.to_vec();
    kndo::engine::sort_findings_for_display(&mut sorted);
    for f in sorted {
        out.push_str("  ");
        out.push_str(&render_finding_line(f, opts));
        out.push('\n');
        render_related(out, f);
    }
    out.push('\n');
}

/// The `related` evidence chain, indented under its finding — one `└` line
/// per entry: location, then the note that explains the hop.
fn render_related(out: &mut String, f: &Finding) {
    for r in &f.related {
        let location = match r.range {
            Some(range) => format!("{}:{}", r.path.0, range.start.0),
            None => r.path.0.to_string(),
        };
        match &r.note {
            Some(note) => out.push_str(&format!("      └ {location} — {note}\n")),
            None => out.push_str(&format!("      └ {location}\n")),
        }
    }
}

fn render_section(out: &mut String, group: &str, findings: &[&Finding], opts: &RenderOptions) {
    out.push_str(&format!("{} ({})\n", group.to_uppercase(), findings.len()));
    for f in findings {
        out.push_str("  ");
        out.push_str(&render_finding_line(f, opts));
        out.push('\n');
        render_related(out, f);
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

/// Navigation verbs: a one-line header (`verb · status · Nms`), then one block per
/// `results[]` entry — numbered only when the request batched more than one selector, matching
/// the agent renderer's discipline (`agent_format::render_query`) but with color and glyphs.
pub(crate) fn render_query(result: &QueryResult, opts: &RenderOptions) -> String {
    let status = result.status();
    let status_color = match status {
        "ok" => "",
        "not-found" => YELLOW,
        _ => RED,
    };
    let mut out = if opts.color {
        format!(
            "kndo · {} · {status_color}{status}{RESET} · {}ms\n",
            result.verb.as_str(),
            result.duration_ms
        )
    } else {
        format!(
            "kndo · {} · {status} · {}ms\n",
            result.verb.as_str(),
            result.duration_ms
        )
    };

    if opts.quiet {
        return out;
    }
    out.push('\n');

    let batched = result.results.len() > 1;
    for (i, entry) in result.results.iter().enumerate() {
        if batched {
            let selector = result.selectors.get(i).map(String::as_str).unwrap_or("?");
            out.push_str(&format!("[{}] {selector}\n", i + 1));
        }
        render_query_entry(&mut out, entry, opts);
        out.push('\n');
    }
    out
}

fn render_query_entry(out: &mut String, entry: &ResultEntry, opts: &RenderOptions) {
    match entry {
        ResultEntry::Failed {
            status,
            selector,
            message,
        } => {
            let color = if opts.color {
                if *status == "not-found" {
                    YELLOW
                } else {
                    RED
                }
            } else {
                ""
            };
            let reset = if opts.color { RESET } else { "" };
            out.push_str(&format!("{color}{status}{reset}: {selector} — {message}\n"));
        }
        ResultEntry::Find(r) => {
            for (n, m) in r.matches.iter().enumerate() {
                out.push_str(&format!("  {}. {}\n", n + 1, node_line(m)));
            }
            if r.elided > 0 {
                out.push_str(&format!("  … {} more (--limit)\n", r.elided));
            }
        }
        ResultEntry::Describe(d) => {
            out.push_str(&format!("  {}\n", node_line(&d.node)));
            if let Some(decl) = &d.declaration {
                out.push_str(&format!(
                    "  declaration: {} · visibility {}{}\n",
                    decl.kind,
                    decl.visibility,
                    if decl.exported { " · exported" } else { "" }
                ));
            }
            if let Some(file) = &d.file {
                out.push_str(&format!("  file: {} · {}\n", file.role, file.origin));
            }
            if let Some(dep) = &d.dependency {
                out.push_str(&format!(
                    "  dependency: {} · {} importing file{} · {}\n",
                    dep.manifest_scopes.join(", "),
                    dep.importing_files,
                    if dep.importing_files == 1 { "" } else { "s" },
                    if dep.used { "used" } else { "unused" }
                ));
            }
            if let Some(pkg) = &d.package {
                out.push_str(&format!(
                    "  package: {} · {} files · {} dependent{}\n",
                    pkg.mode,
                    pkg.files,
                    pkg.dependents,
                    if pkg.dependents == 1 { "" } else { "s" }
                ));
            }
            out.push_str(&format!(
                "  degree: in={} out={}\n",
                d.degree.in_by_kind.values().sum::<usize>(),
                d.degree.out_by_kind.values().sum::<usize>()
            ));
            if !d.reached_by_roots.is_empty() {
                out.push_str("  reached by roots:\n");
                for r in &d.reached_by_roots {
                    out.push_str(&format!("    {}\n", node_line(r)));
                }
            }
            if !d.declared_symbols.is_empty() {
                out.push_str("  declared symbols:\n");
                for s in &d.declared_symbols {
                    out.push_str(&format!("    {}\n", node_line(s)));
                }
            }
            if !d.findings.is_empty() {
                out.push_str(&format!("  findings: {}\n", d.findings.join(", ")));
            }
            if !d.sources.is_empty() {
                out.push_str(&format!("  sources: {}\n", d.sources.join(", ")));
            }
        }
        ResultEntry::Neighbors(r) => {
            out.push_str(&format!("  {}\n", node_line(&r.node)));
            for e in &r.entries {
                out.push_str(&format!("    {}\n", neighbor_line(e)));
            }
            if r.elided > 0 {
                out.push_str(&format!("  … {} more (--limit)\n", r.elided));
            }
        }
        ResultEntry::Impact(r) => {
            out.push_str(&format!("  {}\n", node_line(&r.node)));
            out.push_str(&format!(
                "  affected: {} (production={} test-only={} tooling-only={} unreachable={})\n",
                r.affected.len() + r.elided,
                r.by_color.production,
                r.by_color.test_only,
                r.by_color.tooling_only,
                r.by_color.unreachable
            ));
            for e in &r.affected {
                out.push_str(&format!("    {}\n", neighbor_line(e)));
            }
            if r.elided > 0 {
                out.push_str(&format!("  … {} more (--limit)\n", r.elided));
            }
            if !r.affected_roots.is_empty() {
                out.push_str("  affected roots:\n");
                for root in &r.affected_roots {
                    out.push_str(&format!("    [{}] {}\n", root.kind, node_line(&root.node)));
                }
                if r.affected_roots_elided > 0 {
                    out.push_str(&format!("    … {} more\n", r.affected_roots_elided));
                }
            }
            if let Some(sim) = &r.if_deleted {
                out.push_str("  if deleted:\n");
                out.push_str(&format!(
                    "    newly unreachable: {}\n",
                    sim.newly_unreachable.len() + sim.newly_unreachable_elided
                ));
                for q in &sim.newly_unreachable {
                    out.push_str(&format!("      {}\n", node_line(q)));
                }
                out.push_str(&format!(
                    "    newly test-only: {}\n",
                    sim.newly_test_only.len() + sim.newly_test_only_elided
                ));
                for q in &sim.newly_test_only {
                    out.push_str(&format!("      {}\n", node_line(q)));
                }
                if !sim.freed_dependencies.is_empty() {
                    out.push_str(&format!(
                        "    freed dependencies: {}\n",
                        sim.freed_dependencies.join(", ")
                    ));
                }
            }
        }
        ResultEntry::Trace(r) => {
            if r.paths.is_empty() {
                out.push_str(&format!(
                    "  {} -/-> {}  (no path)\n",
                    node_line(&r.from),
                    node_line(&r.to)
                ));
            }
            for path in &r.paths {
                let mut line = format!("  {}", node_line(&r.from));
                for hop in &path.hops {
                    line.push_str(&format!(" → [{}] {}", hop.via.edge, node_line(&hop.node)));
                }
                out.push_str(&line);
                out.push('\n');
            }
            if r.paths_elided > 0 {
                out.push_str(&format!(
                    "  … {} more paths (--max-paths)\n",
                    r.paths_elided
                ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use kndo::adapter::ProjectPath;
    use kndo::engine::{Delta, DeltaOrigin, Location, Severity};
    use smol_str::SmolStr;

    fn finding(category: &str, group: &str) -> Finding {
        Finding {
            advisory: false,
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
            related: Vec::new(),
            delta: None,
            delta_origin: None,
        }
    }

    fn opts() -> RenderOptions {
        RenderOptions {
            color: false,
            quiet: false,
            verbose: false,
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
                verbose: false,
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

    fn qnode(selector: &str, kind: &str) -> kndo::query::QNodeRef {
        kndo::query::QNodeRef {
            selector: selector.to_string(),
            kind: kind.to_string(),
            color: Some("production".to_string()),
            span: None,
        }
    }

    fn query_result(entry: kndo::query_envelope::ResultEntry) -> QueryResult {
        QueryResult {
            verb: kndo::query_envelope::Verb::Describe,
            selectors: vec!["dep:left-pad".to_string()],
            id: None,
            cache: "warm",
            duration_ms: 3,
            results: vec![entry],
            diagnostics: vec![],
        }
    }

    #[test]
    fn describe_human_output_shows_declaration_file_and_dependency_blocks() {
        use kndo::query::{DeclarationInfo, Degree, DependencyInfo, DescribeResult, NodeSpan};
        use kndo::query_envelope::ResultEntry;

        let symbol = query_result(ResultEntry::Describe(Box::new(DescribeResult {
            node: qnode("src/billing.js#computeTotal", "function"),
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
        let out = render_query(&symbol, &opts());
        assert!(
            out.contains("declaration: function · visibility 1 · exported"),
            "{out}"
        );

        let dep = query_result(ResultEntry::Describe(Box::new(DescribeResult {
            node: qnode("dep:left-pad", "dependency"),
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
        let out = render_query(&dep, &opts());
        assert!(
            out.contains("dependency: prod · 1 importing file · used"),
            "{out}"
        );
    }

    #[test]
    fn verbose_renders_the_phases_block_and_quiet_default_hides_it() {
        let mut result = RunResult {
            mode: "full".to_string(),
            ..RunResult::default()
        };
        result.timings = vec![
            ("assemble".to_string(), 18_700),
            ("health".to_string(), 800),
        ];
        let base = RenderOptions {
            color: false,
            quiet: false,
            verbose: false,
        };
        assert!(!render(&result, &base).contains("phases"));
        let verbose = RenderOptions {
            verbose: true,
            ..base
        };
        let out = render(&result, &verbose);
        assert!(out.contains("phases"));
        assert!(out.contains("assemble"));
        assert!(out.contains("18.7ms"));
        assert!(out.contains("cache: disabled, 0 hits"));
    }
}
