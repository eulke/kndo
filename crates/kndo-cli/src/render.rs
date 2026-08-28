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
//!
//! Rendered in full, and listed because earlier drafts of this doc claimed otherwise:
//! - The health block — a score/grade line plus per-category penalty bars (non-zero
//!   categories only in `check` output; `kndo health` renders the full table), with diff
//!   mode's before ──▶ after line carrying the grade-boundary distance on drops.
//! - The `[delta]` budget block, beside it, whenever budgets are configured.
//! - Findings that carry a `related` evidence chain (first populated by `cyclic`) render it as
//!   indented `└` lines under the finding — role, location, note.

use kndo::analysis::health::Health;
use kndo::query::NeighborEntry;
use kndo::{
    Confidence, DeltaOrigin, Diagnostic, Finding, Group, QueryResult, ResultEntry, RunResult,
};

pub(crate) struct RenderOptions {
    pub(crate) color: bool,
    pub(crate) quiet: bool,
    /// Adds the per-phase timing block and cache state to check output. The third
    /// `--verbose` effect — revealing `possible`-confidence findings — is engine-side:
    /// `check` passes a `Possible` report floor override, so verbose shows every tier
    /// even when the project's `min-confidence` config raises the floor.
    pub(crate) verbose: bool,
    /// `kndo health --by-package`: include the per-package breakdown table. Only
    /// `render_health`'s `full_table` (`kndo health` itself) ever looks at this — `check`'s own
    /// summary health line never shows packages regardless.
    pub(crate) by_package: bool,
}

/// Diagnostics print on stderr in every output format — a degraded run must never look silent.
/// Shared by every command that runs a real analysis (`check`, `kndo health`).
pub(crate) fn diagnostics(diagnostics: &[Diagnostic]) {
    for d in diagnostics {
        match &d.path {
            Some(p) => eprintln!("kndo: {}: {}: {}", d.level, p.0, d.message),
            None => eprintln!("kndo: {}: {}", d.level, d.message),
        }
    }
}

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

/// `--only`'s narrowing, and never omitted when non-zero — least of all on the clean line.
/// A `kndo · clean` header printed over a report that quietly dropped forty findings is the
/// worst sentence this program can write.
fn elided_suffix(result: &RunResult) -> String {
    match result.elided {
        0 => String::new(),
        n => format!(" · {n} outside --only"),
    }
}

const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";
const BLUE: &str = "\x1b[34m";
const CYAN: &str = "\x1b[36m";
const RESET: &str = "\x1b[0m";

pub(crate) fn render(result: &RunResult, opts: &RenderOptions) -> String {
    if result.mode == "staged" || result.mode == "diff" {
        return render_diff(result, opts);
    }

    let baseline_suffix = baseline_suffix(result);
    let suppressed_suffix = suppressed_suffix(result) + &elided_suffix(result);

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
    // The findings listing has no header line to hang a suffix on, and this is exactly the
    // output a `--only` run produces — so the narrowing gets its own line rather than being
    // the one branch where the lens goes unmentioned.
    if result.elided > 0 {
        out.push_str(&format!("{} outside --only\n\n", result.elided));
    }
    let present: std::collections::BTreeSet<Group> =
        result.findings.iter().map(|f| f.group).collect();
    let groups = Group::DISPLAY_ORDER
        .into_iter()
        .filter(|g| present.contains(g));

    for group in groups {
        let mut in_group: Vec<&Finding> = result
            .findings
            .iter()
            .filter(|f| f.group == group)
            .collect();
        kndo::sort_findings_for_display(&mut in_group);
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
    if full_table && opts.by_package && !health.packages.is_empty() {
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

/// The budget block: the label on the first row only, then one row per configured rule —
/// `<rule> ≤ <limit>   <measured>   ok|FAIL`, with the overrun spelled out on the rows that
/// broke. Aligned into columns for the same reason the health table is: a reader scans the
/// limit column to find the one that gave way, not the prose.
///
/// No color-conditional glyphs. `penalty_bar` branches on `opts.color` because it draws a
/// *chart*; a verdict is a word, and the word is the same in both terminals — which is also
/// what keeps this block diffable in a CI log.
fn budget_block(budget: &kndo::Budget) -> String {
    let width = budget
        .rules
        .iter()
        .map(|r| r.rule.len())
        .max()
        .unwrap_or(0)
        .max("max-net-findings".len());
    let mut out = String::new();
    for (i, rule) in budget.rules.iter().enumerate() {
        // The label names the block once, like `health` does; repeating it on every row would
        // read as several budgets rather than one budget with several rules.
        let label = if i == 0 { "budget" } else { "" };
        let verdict = match rule.over_by {
            Some(over) => format!("FAIL   (over by {})", trim_num(over)),
            None => "ok".to_string(),
        };
        out.push_str(&format!(
            "{label:<8} {:<width$} ≤ {:<6} {:<8} {verdict}\n",
            rule.rule,
            trim_num(rule.limit),
            trim_num(rule.measured),
        ));
    }
    out
}

/// Whole numbers lose the decimal tail; a real fraction keeps one place. Same rule the agent
/// format uses, so the two renderers describe one limit identically.
fn trim_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v:.1}")
    }
}

fn grade_boundary_suffix(score: f64, grade: &str) -> String {
    match kndo::analysis::health::grade_boundary(grade) {
        Some((threshold, next)) => format!("  ({:.1} from {next})", score - threshold),
        None => String::new(),
    }
}

/// Diff modes' rendering: a one-line header (`N new · M fixed · net ±K`), then
/// `NEW (introduced by this change)`, `NEW (derived, in untouched code)`, and `FIXED` sections
/// — each present only when non-empty, in that fixed order.
fn render_diff(result: &RunResult, opts: &RenderOptions) -> String {
    let net = result.net_findings();
    let baseline_suffix = baseline_suffix(result);
    let suppressed_suffix = suppressed_suffix(result) + &elided_suffix(result);
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

    let budget_lines = result.budget.as_ref().map(budget_block).unwrap_or_default();

    // `--quiet` is header + exit code by contract (RFC 0009 §6), so it stays bare. The clean
    // branch is NOT: a change can move zero findings and still break `max-health-drop`, and
    // that is precisely the case where a bare header would leave the exit code unexplained.
    if opts.quiet {
        return format!("{header}{health_line}");
    }
    if result.findings.is_empty() && result.fixed.is_empty() {
        return format!("{header}{health_line}{budget_lines}");
    }

    let mut out = format!("{header}{health_line}{budget_lines}");
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
    kndo::sort_findings_for_display(&mut sorted);
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
        out.push_str(&r.render("      └ ", " — "));
        out.push('\n');
    }
}

fn render_section(out: &mut String, group: Group, findings: &[&Finding], opts: &RenderOptions) {
    out.push_str(&format!(
        "{} ({})\n",
        group.as_str().to_uppercase(),
        findings.len()
    ));
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
        f.category.to_string()
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
        format!(" ({})", f.confidence.as_str())
    };

    let glyph = glyph(f.group, opts.color);
    if opts.color {
        format!(
            "{}{glyph}{RESET} {category} {location}  {}{confidence} [{}]",
            color_code(f.group),
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

fn glyph(group: Group, rich: bool) -> &'static str {
    match (group, rich) {
        (Group::Defect, true) => "✗",
        (Group::Defect, false) => "x",
        (Group::Waste, true) => "◦",
        (Group::Waste, false) => "o",
        (Group::Risk, true) => "▲",
        (Group::Risk, false) => "^",
        (Group::Hygiene, true) => "·",
        (Group::Hygiene, false) => ".",
        // Mirrors action/render.mjs's GLYPH table (the copy that was already correct).
        (Group::Convention, true) => "•",
        (Group::Convention, false) => "?",
    }
}

fn color_code(group: Group) -> &'static str {
    match group {
        Group::Defect => RED,
        Group::Waste => YELLOW,
        Group::Risk => MAGENTA,
        Group::Hygiene => BLUE,
        Group::Convention => CYAN,
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

/// The `describe` block — shared by `describe` and `explain`, which shows the same block over
/// the subject a finding landed on. One renderer, so the two can never describe a node
/// differently in the same terminal.
fn render_describe(out: &mut String, d: &kndo::query::DescribeResult) {
    out.push_str(&format!("  {}\n", d.node));
    if let Some(decl) = &d.declaration {
        out.push_str(&format!(
            "  declaration: {} · visibility {}{}\n",
            decl.kind,
            // The label when the adapter has one, the ordinal when it does not — never
            // the bare number as the only answer, which asks a reader to know a ladder
            // they cannot see.
            decl.visibility_label
                .clone()
                .unwrap_or_else(|| decl.visibility.to_string()),
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
    for m in &d.metrics {
        let shape = match m.shape_ordinal {
            0 => String::new(),
            n => format!(" (nested shape {n} at line {})", m.span.start.0),
        };
        // "unmeasured", never "0%": no coverage report means unknown.
        let covered = match m.coverage {
            Some(c) => format!("{:.0}% covered", c * 100.0),
            None => "coverage unmeasured".to_string(),
        };
        let crap = match m.crap {
            Some(c) => format!(" · crap {c:.1}"),
            None => String::new(),
        };
        out.push_str(&format!(
            "  metrics{shape}: cyclomatic {} · {} loc · {} tokens · {covered}{crap}\n",
            m.cyclomatic, m.loc, m.token_count
        ));
    }
    for g in &d.duplication {
        out.push_str(&format!(
            "  duplication: {} clones ({})\n",
            g.members.len(),
            g.finding
        ));
        for m in &g.members {
            out.push_str(&format!("    {m}\n"));
        }
    }
    out.push_str(&format!(
        "  degree: in={} out={}\n",
        d.degree.in_by_kind.values().sum::<usize>(),
        d.degree.out_by_kind.values().sum::<usize>()
    ));
    if !d.reached_by_roots.is_empty() {
        out.push_str("  reached by roots:\n");
        for r in &d.reached_by_roots {
            out.push_str(&format!("    {}\n", r));
        }
    }
    if !d.declared_symbols.is_empty() {
        out.push_str("  declared symbols:\n");
        for s in &d.declared_symbols {
            out.push_str(&format!("    {}\n", s));
        }
    }
    if !d.findings.is_empty() {
        out.push_str(&format!("  findings: {}\n", d.findings.join(", ")));
    }
    if !d.sources.is_empty() {
        out.push_str(&format!("  sources: {}\n", d.sources.join(", ")));
    }
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
                out.push_str(&format!("  {}. {}\n", n + 1, m));
            }
            if r.elided > 0 {
                out.push_str(&format!("  … {} more (--limit)\n", r.elided));
            }
        }
        ResultEntry::Describe(d) => render_describe(out, d),
        ResultEntry::Explain(e) => {
            // The finding exactly as `check` prints it — same glyph, same category, same id —
            // then its evidence chain, then the same describe block over what it landed on.
            out.push_str(&format!("  {}\n", render_finding_line(&e.finding, opts)));
            render_related(out, &e.finding);
            if !e.finding.sources.is_empty() {
                out.push_str(&format!("  sources: {}\n", e.finding.sources.join(", ")));
            }
            match (&e.subject, &e.subject_selector) {
                (Some(d), _) => {
                    out.push('\n');
                    render_describe(out, d);
                }
                // Stated, not omitted: a rollup has no single node, and a subject the graph
                // never saw is itself worth reading.
                (None, Some(selector)) => {
                    out.push_str(&format!("  subject: {selector} (not a graph node)\n"))
                }
                (None, None) => out.push_str("  subject: not a single node (rollup)\n"),
            }
        }
        ResultEntry::Neighbors(r) => {
            out.push_str(&format!("  {}\n", r.node));
            for e in &r.entries {
                out.push_str(&format!("    {}\n", neighbor_line(e)));
            }
            if r.elided > 0 {
                out.push_str(&format!("  … {} more (--limit)\n", r.elided));
            }
        }
        ResultEntry::Impact(r) => {
            out.push_str(&format!("  {}\n", r.node));
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
                    out.push_str(&format!("    [{}] {}\n", root.kind, root.node));
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
                    out.push_str(&format!("      {}\n", q));
                }
                out.push_str(&format!(
                    "    newly test-only: {}\n",
                    sim.newly_test_only.len() + sim.newly_test_only_elided
                ));
                for q in &sim.newly_test_only {
                    out.push_str(&format!("      {}\n", q));
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
                out.push_str(&format!("  {} -/-> {}  (no path)\n", r.from, r.to));
            }
            for path in &r.paths {
                let mut line = format!("  {}", r.from);
                for hop in &path.hops {
                    line.push_str(&format!(" → [{}] {}", hop.via.edge, hop.node));
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

fn neighbor_line(e: &NeighborEntry) -> String {
    format!("{} via {} (depth {})", e.node, e.via.edge, e.depth)
}

#[cfg(test)]
mod tests {

    /// **Every group has a glyph in both modes, and the rich ones match `action/render.mjs`.**
    ///
    /// `glyph` is a ten-arm table whose own comment says it mirrors the Action's `GLYPH`, and
    /// nothing checked that — kndo's `crap` analysis reported the function at 31% coverage on
    /// this repository, which is the same fact stated numerically: seven arms had never been
    /// evaluated. Two copies of one table, one of them unexercised, is the drift this repo
    /// removes everywhere else; here it is asserted instead, because the Action is JavaScript
    /// and cannot import the Rust one.
    #[test]
    fn every_group_has_a_glyph_and_the_rich_table_matches_the_action() {
        let js = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../action/render.mjs"),
        )
        .expect("action/render.mjs is in the repository");
        let line = js
            .lines()
            .find(|l| l.contains("const GLYPH"))
            .expect("render.mjs declares GLYPH");

        for group in Group::DISPLAY_ORDER {
            let rich = glyph(group, true);
            let plain = glyph(group, false);
            assert!(!rich.is_empty() && !plain.is_empty(), "{group:?}");
            assert_ne!(
                rich, plain,
                "{group:?}: the plain fallback exists so a non-UTF-8 terminal reads differently"
            );
            assert!(
                plain.is_ascii(),
                "{group:?}: the plain glyph {plain:?} is the one for terminals that cannot \
                 render the rich set, so it has to be ASCII"
            );
            assert!(
                line.contains(&format!("{}: \"{rich}\"", group.as_str())),
                "action/render.mjs has a different glyph for {}: {line}",
                group.as_str()
            );
        }
    }

    /// The plain glyphs are distinct from each other, which is the only reason a reader can tell
    /// the groups apart without colour or Unicode.
    #[test]
    fn the_plain_glyphs_are_all_different() {
        let mut seen: Vec<&str> = Group::DISPLAY_ORDER
            .iter()
            .map(|g| glyph(*g, false))
            .collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen.len(),
            count,
            "two groups share a plain glyph: {seen:?}"
        );
    }
    use super::*;
    use kndo::ProjectPath;
    use kndo::{Delta, DeltaOrigin, Location, Severity};
    use smol_str::SmolStr;

    fn parse_group(g: &str) -> Group {
        match g {
            "defect" => Group::Defect,
            "waste" => Group::Waste,
            "risk" => Group::Risk,
            "hygiene" => Group::Hygiene,
            "convention" => Group::Convention,
            other => panic!("unknown test group {other}"),
        }
    }

    fn finding(category: &str, group: &str) -> Finding {
        Finding {
            advisory: false,
            id: format!("kndo-{category}"),
            category: category.into(),
            group: parse_group(group),
            subject_kind: "function".into(),
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
            rolled_up: None,
            sources: Vec::new(),
            delta: None,
            delta_origin: None,
        }
    }

    fn opts() -> RenderOptions {
        RenderOptions {
            color: false,
            quiet: false,
            verbose: false,
            by_package: false,
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
    fn a_clean_line_never_stands_alone_over_a_narrowed_run() {
        // The worst sentence this program can write: `kndo · clean` over a report that
        // dropped everything because the caller narrowed it. A `--only` that matches nothing
        // is exactly how that happens.
        let result = RunResult {
            mode: "full".to_string(),
            elided: 235,
            files_discovered: 339,
            ..RunResult::default()
        };
        let out = render(&result, &opts());
        assert!(out.contains("clean"), "{out}");
        assert!(out.contains("· 235 outside --only"), "{out}");
    }

    #[test]
    fn the_findings_listing_says_what_the_lens_removed() {
        // The listing has no header line to hang a suffix on — the one branch where a lens
        // could have gone unmentioned.
        let mut f = finding("unused", "waste");
        f.delta = None;
        let result = RunResult {
            mode: "full".to_string(),
            findings: vec![f],
            elided: 47,
            ..RunResult::default()
        };
        let out = render(&result, &opts());
        assert!(out.starts_with("47 outside --only\n\n"), "{out}");
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

    fn budget(rules: Vec<kndo::BudgetRule>) -> kndo::Budget {
        let verdict = if rules.iter().any(|r| r.verdict == kndo::BudgetVerdict::Fail) {
            kndo::BudgetVerdict::Fail
        } else {
            kndo::BudgetVerdict::Pass
        };
        kndo::Budget { verdict, rules }
    }

    fn budget_rule(rule: &str, limit: f64, measured: f64) -> kndo::BudgetRule {
        let over = measured - limit;
        kndo::BudgetRule {
            rule: rule.to_string(),
            limit,
            measured,
            verdict: if over > 0.0 {
                kndo::BudgetVerdict::Fail
            } else {
                kndo::BudgetVerdict::Pass
            },
            over_by: (over > 0.0).then_some(over),
        }
    }

    #[test]
    fn the_budget_block_names_itself_once_and_spells_out_every_overrun() {
        let result = RunResult {
            mode: "diff".to_string(),
            base_ref: Some("main".to_string()),
            budget: Some(budget(vec![
                budget_rule("max-health-drop", 0.0, -1.7),
                budget_rule("max-net-findings", 0.0, 1.0),
                budget_rule("defect", 2.0, 0.0),
            ])),
            ..RunResult::default()
        };
        let out = render(&result, &opts());
        // Exact, not `contains`: the columns ARE the feature — a reader scans the limit
        // column to find the rule that gave way, and drifting alignment is how that stops
        // working without any test noticing.
        let expected = concat!(
            "kndo · diff · 0 new · 0 fixed · net +0\n",
            "budget   max-health-drop  ≤ 0      -1.7     ok\n",
            "         max-net-findings ≤ 0      1        FAIL   (over by 1)\n",
            "         defect           ≤ 2      0        ok\n",
        );
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn a_clean_diff_still_prints_a_budget_that_broke() {
        // The case the old "no findings, no body" shortcut got wrong: a change can move zero
        // findings and still break `max-health-drop` (a function got longer, coverage fell).
        // Printing only the header there would leave the exit code with no stated reason.
        let result = RunResult {
            mode: "diff".to_string(),
            base_ref: Some("main".to_string()),
            budget: Some(budget(vec![budget_rule("max-health-drop", 0.0, 2.5)])),
            ..RunResult::default()
        };
        let out = render(&result, &opts());
        assert!(out.contains("FAIL   (over by 2.5)"), "{out}");
    }

    #[test]
    fn quiet_stays_a_one_liner_even_when_a_budget_broke() {
        // `--quiet` is header + exit code by contract (RFC 0009 §6). The budget is why the
        // exit code is 1, and the caller asked not to be told.
        let result = RunResult {
            mode: "diff".to_string(),
            base_ref: Some("main".to_string()),
            budget: Some(budget(vec![budget_rule("max-net-findings", 0.0, 3.0)])),
            ..RunResult::default()
        };
        let out = render(
            &result,
            &RenderOptions {
                color: false,
                quiet: true,
                verbose: false,
                by_package: false,
            },
        );
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
                by_package: false,
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
            suppressed: kndo::SuppressedSummary {
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
            suppressed: kndo::SuppressedSummary {
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
            suppressed: kndo::SuppressedSummary {
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

    fn query_result(entry: kndo::ResultEntry) -> QueryResult {
        QueryResult {
            verb: kndo::Verb::Describe,
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
                visibility_label: None,
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
            metrics: Vec::new(),
            duplication: Vec::new(),
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

    /// **Every section the human renderer can emit, emitted.**
    ///
    /// Its sibling above populates three of the ten optional parts; kndo's own `crap` analysis
    /// put this renderer at 51% coverage on this repository, which says the same thing in a
    /// number. This is the CLI half of the pair `agent_format` also owns — the facade rule means
    /// they read the same `DescribeResult`, so both need the whole shape exercised, not one
    /// corner each.
    #[test]
    fn describe_human_output_renders_every_section_it_declares() {
        use kndo::query::{
            DeclarationInfo, Degree, DescribeResult, DuplicationInfo, FileInfo, NodeSpan,
            PackageInfo, ShapeMetrics,
        };

        let full = query_result(ResultEntry::Describe(Box::new(DescribeResult {
            node: qnode("src/billing.js#computeTotal", "function"),
            declaration: Some(DeclarationInfo {
                kind: "function".to_string(),
                span: NodeSpan {
                    path: "src/billing.js".to_string(),
                    start: (1, 1),
                    end: (9, 2),
                },
                exported: true,
                visibility: 2,
                visibility_label: Some("pub(crate)".to_string()),
            }),
            file: Some(FileInfo {
                role: "production".to_string(),
                origin: "authored".to_string(),
            }),
            dependency: None,
            package: Some(PackageInfo {
                mode: "library".to_string(),
                files: 12,
                dependents: 4,
            }),
            metrics: vec![ShapeMetrics {
                shape_ordinal: 0,
                span: NodeSpan {
                    path: "src/billing.js".to_string(),
                    start: (1, 1),
                    end: (9, 2),
                },
                cyclomatic: 7,
                loc: 40,
                token_count: 120,
                coverage: Some(0.5),
                crap: Some(19.5),
            }],
            duplication: vec![DuplicationInfo {
                finding: "duplicate:callable:abc".to_string(),
                members: vec!["src/other.js#alsoComputes".to_string()],
            }],
            // The degree line renders from the maps' totals and is exercised by the sibling
            // test; populating them here would mean a `rustc-hash` dependency in a crate that
            // imports only through the `kndo::` facade, which is not worth a test's convenience.
            degree: Degree::default(),
            reached_by_roots: vec![qnode("src/index.js", "file")],
            findings: vec!["untested:callable:def".to_string()],
            sources: vec!["adapter:js-ts".to_string()],
            declared_symbols: vec![qnode("src/billing.js#helper", "function")],
            elided: Default::default(),
        })));

        let out = render_query(&full, &opts());
        for expected in [
            // The label wins over the ordinal — the branch the sibling test leaves `None`.
            "visibility pub(crate)",
            "production",
            "authored",
            "library",
            "src/other.js#alsoComputes",
            "src/index.js",
            "src/billing.js#helper",
            "untested:callable:def",
            "adapter:js-ts",
        ] {
            assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
        }
        // The metrics numbers themselves, not a rounded summary.
        assert!(
            out.contains('7') && out.contains("40") && out.contains("120"),
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
            by_package: false,
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
    // ------------------------------------------------------------ render_query_entry variants

    fn spanned_qnode(selector: &str, kind: &str) -> kndo::query::QNodeRef {
        kndo::query::QNodeRef {
            selector: selector.to_string(),
            kind: kind.to_string(),
            color: None,
            span: Some(kndo::query::NodeSpan {
                path: "src/lib.rs".to_string(),
                start: (10, 1),
                end: (12, 2),
            }),
        }
    }

    fn qedge(edge: &str) -> kndo::query::QEdgeRef {
        kndo::query::QEdgeRef {
            edge: edge.to_string(),
            confidence: Confidence::Certain,
            site: None,
        }
    }

    fn no_color() -> RenderOptions {
        RenderOptions {
            color: false,
            quiet: false,
            verbose: false,
            by_package: false,
        }
    }

    #[test]
    fn failed_entry_shows_status_selector_and_message() {
        let out = render_query(
            &query_result(ResultEntry::Failed {
                status: "not-found",
                selector: "sym:ghost".to_string(),
                message: "no symbol matches".to_string(),
            }),
            &no_color(),
        );
        assert!(
            out.contains("not-found: sym:ghost — no symbol matches"),
            "{out}"
        );
    }

    #[test]
    fn find_entry_numbers_matches_and_reports_elided() {
        let out = render_query(
            &query_result(ResultEntry::Find(kndo::query::FindResult {
                matches: vec![
                    spanned_qnode("sym:alpha", "function"),
                    spanned_qnode("sym:beta", "class"),
                ],
                elided: 3,
            })),
            &no_color(),
        );
        assert!(
            out.contains("1. [sym:alpha] function src/lib.rs:10"),
            "{out}"
        );
        assert!(out.contains("2. [sym:beta] class src/lib.rs:10"), "{out}");
        assert!(out.contains("… 3 more (--limit)"), "{out}");
    }

    #[test]
    fn neighbors_entry_shows_node_edges_and_elided() {
        let out = render_query(
            &query_result(ResultEntry::Neighbors(kndo::query::NeighborsResult {
                node: spanned_qnode("file:src/lib.rs", "file"),
                entries: vec![kndo::query::NeighborEntry {
                    node: spanned_qnode("sym:helper", "function"),
                    via: qedge("references"),
                    depth: 2,
                }],
                by_color: kndo::query::ByColor {
                    production: 1,
                    test_only: 0,
                    tooling_only: 0,
                    unreachable: 0,
                },
                elided: 1,
            })),
            &no_color(),
        );
        assert!(out.contains("[file:src/lib.rs] file"), "{out}");
        assert!(
            out.contains("[sym:helper] function src/lib.rs:10 via references (depth 2)"),
            "{out}"
        );
        assert!(out.contains("… 1 more (--limit)"), "{out}");
    }

    #[test]
    fn impact_entry_shows_counts_roots_and_if_deleted_blocks() {
        let out = render_query(
            &query_result(ResultEntry::Impact(Box::new(kndo::query::ImpactResult {
                node: spanned_qnode("sym:core", "function"),
                affected: vec![kndo::query::NeighborEntry {
                    node: spanned_qnode("sym:caller", "function"),
                    via: qedge("references"),
                    depth: 1,
                }],
                by_color: kndo::query::ByColor {
                    production: 2,
                    test_only: 1,
                    tooling_only: 0,
                    unreachable: 0,
                },
                elided: 2,
                affected_roots: vec![kndo::query::AffectedRoot {
                    kind: "production".to_string(),
                    node: spanned_qnode("file:src/main.rs", "file"),
                }],
                affected_roots_elided: 1,
                if_deleted: Some(kndo::query::IfDeleted {
                    newly_unreachable: vec![spanned_qnode("sym:orphan", "function")],
                    newly_unreachable_elided: 0,
                    newly_test_only: vec![],
                    newly_test_only_elided: 2,
                    freed_dependencies: vec!["left-pad".to_string()],
                }),
            }))),
            &no_color(),
        );
        assert!(
            out.contains("affected: 3 (production=2 test-only=1 tooling-only=0 unreachable=0)"),
            "{out}"
        );
        assert!(out.contains("… 2 more (--limit)"), "{out}");
        assert!(out.contains("affected roots:"), "{out}");
        assert!(
            out.contains("[production] [file:src/main.rs] file"),
            "{out}"
        );
        assert!(out.contains("… 1 more"), "{out}");
        assert!(out.contains("if deleted:"), "{out}");
        assert!(out.contains("newly unreachable: 1"), "{out}");
        assert!(out.contains("[sym:orphan] function"), "{out}");
        assert!(out.contains("newly test-only: 2"), "{out}");
        assert!(out.contains("freed dependencies: left-pad"), "{out}");
    }

    #[test]
    fn trace_entry_renders_paths_no_path_and_elision() {
        let with_path = render_query(
            &query_result(ResultEntry::Trace(kndo::query::TraceResult {
                from: spanned_qnode("file:src/a.rs", "file"),
                to: spanned_qnode("file:src/b.rs", "file"),
                paths: vec![kndo::query::Path {
                    hops: vec![kndo::query::Hop {
                        node: spanned_qnode("file:src/b.rs", "file"),
                        via: qedge("imports"),
                    }],
                    weakest_confidence: Confidence::Certain,
                }],
                paths_elided: 4,
            })),
            &no_color(),
        );
        assert!(
            with_path
                .contains("[file:src/a.rs] file src/lib.rs:10 → [imports] [file:src/b.rs] file"),
            "{with_path}"
        );
        assert!(
            with_path.contains("… 4 more paths (--max-paths)"),
            "{with_path}"
        );

        let no_path = render_query(
            &query_result(ResultEntry::Trace(kndo::query::TraceResult {
                from: spanned_qnode("file:src/a.rs", "file"),
                to: spanned_qnode("file:src/b.rs", "file"),
                paths: vec![],
                paths_elided: 0,
            })),
            &no_color(),
        );
        assert!(no_path.contains("-/->"), "{no_path}");
        assert!(no_path.contains("(no path)"), "{no_path}");
    }
}
