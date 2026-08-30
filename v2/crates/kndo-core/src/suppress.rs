//! Suppression, parsed once, here. Adapters report where comments ARE
//! ([`kndo_contract::evidence::CommentSpan`]); this module owns what a `kndo:` pragma
//! inside one means — every adapter, WASM included, gets the same grammar for free.
//!
//! The grammar, inside any comment:
//!   `kndo:allow <category>[, <category>…] [-- reason]`       (line scope)
//!   `kndo:allow-file <category>[, <category>…] [-- reason]`  (whole file)
//! Line scope covers findings whose subject starts on the pragma's line or the next.
//!
//! An allow that suppresses nothing is `stale` — dead configuration is still dead
//! code — with one carve-out that must never regress: an allow whose category was
//! not JUDGED this run (its analysis abstained, or no analysis ships for it yet) is
//! not stale, or turning an analysis off — or not having built it — would flip
//! every allow of it to stale and back (the v1 allow/stale flicker, four real
//! cases; this repository's own `kndo:allow-file crap` is the not-built case).

use kndo_contract::evidence::DiagnosticLevel;
use kndo_contract::finding::{Finding, Severity, sort_findings};
use kndo_contract::subject::Subject;
use kndo_contract::vocab::{Category, Confidence, ProjectPath, Span};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Line,
    File,
}

struct Pragma {
    path: ProjectPath,
    span: Span,
    /// 1-based line the pragma sits on.
    line: u32,
    categories: Vec<Category>,
    scope: Scope,
}

/// What suppression did to a run, for the report: totals only — the suppressed
/// findings themselves are the pragmas' business, not the envelope's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SuppressedSummary {
    pub total: u32,
    pub by_category: Vec<(Category, u32)>,
}

/// A diagnostic-shaped problem in a pragma (an unknown category name), reported
/// through the run's diagnostics channel, never fatal.
pub struct PragmaProblem {
    pub path: ProjectPath,
    pub level: DiagnosticLevel,
    pub message: String,
}

pub struct Suppressed {
    pub summary: SuppressedSummary,
    pub problems: Vec<PragmaProblem>,
}

/// Applies every pragma in the graph's comment evidence to `findings`: suppressed
/// findings move out, stale-allow findings move in, and the summary says what
/// happened. `contents` supplies comment text and line positions; `abstained` and
/// `judged` come from the run's [`AnalysisOutcome`] and decide staleness.
pub fn apply(
    graph: &crate::graph::Graph,
    contents: &BTreeMap<ProjectPath, &[u8]>,
    abstained: &[crate::analysis::Abstention],
    judged: &BTreeSet<Category>,
    mut findings: Vec<Finding>,
) -> (Vec<Finding>, Suppressed) {
    let mut pragmas: Vec<Pragma> = Vec::new();
    let mut problems: Vec<PragmaProblem> = Vec::new();
    for f in &graph.files {
        let Some(content) = contents.get(&f.path) else {
            continue;
        };
        let starts = line_starts(content);
        for comment in &f.evidence.comments {
            let text = comment_text(content, comment.text);
            parse_pragma(
                text,
                &f.path,
                comment.span,
                &starts,
                &mut pragmas,
                &mut problems,
            );
        }
    }

    let abstained_categories: BTreeSet<&Category> = abstained.iter().map(|a| &a.category).collect();
    let line_index: BTreeMap<&ProjectPath, Vec<u32>> = graph
        .files
        .iter()
        .filter_map(|f| contents.get(&f.path).map(|c| (&f.path, line_starts(c))))
        .collect();

    let mut matched = vec![0u32; pragmas.len()];
    let mut kept: Vec<Finding> = Vec::new();
    let mut summary = SuppressedSummary::default();
    let mut by_category: BTreeMap<Category, u32> = BTreeMap::new();

    'findings: for finding in findings.drain(..) {
        for (ix, pragma) in pragmas.iter().enumerate() {
            if !pragma.categories.contains(&finding.category)
                || finding.subject.path() != &pragma.path
            {
                continue;
            }
            let hits = match pragma.scope {
                Scope::File => true,
                Scope::Line => match &finding.subject {
                    Subject::Symbol { span, .. } | Subject::Suppression { span, .. } => line_index
                        .get(finding.subject.path())
                        .is_some_and(|starts| {
                            let line = line_of(starts, span.start);
                            line == pragma.line || line == pragma.line + 1
                        }),
                    // File-shaped subjects have no line; only allow-file reaches them.
                    _ => false,
                },
            };
            if hits {
                matched[ix] += 1;
                summary.total += 1;
                *by_category.entry(finding.category.clone()).or_insert(0) += 1;
                continue 'findings;
            }
        }
        kept.push(finding);
    }
    summary.by_category = by_category.into_iter().collect();

    // Stale allows — only over categories this run actually JUDGED (the flicker
    // rule: an abstained or not-yet-built category makes its allows un-judgeable,
    // never stale).
    for (ix, pragma) in pragmas.iter().enumerate() {
        if matched[ix] > 0 {
            continue;
        }
        if pragma
            .categories
            .iter()
            .any(|c| !judged.contains(c) || abstained_categories.contains(c))
        {
            continue;
        }
        let listed = pragma
            .categories
            .iter()
            .map(Category::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        kept.push(Finding::new(
            Category::STALE,
            Severity::Warning,
            Confidence::Certain,
            Subject::Suppression {
                path: pragma.path.clone(),
                span: pragma.span,
            },
            "",
            format!("this allow suppresses nothing ({listed})"),
        ));
    }

    sort_findings(&mut kept);
    (kept, Suppressed { summary, problems })
}

fn parse_pragma(
    text: &str,
    path: &ProjectPath,
    span: Span,
    starts: &[u32],
    pragmas: &mut Vec<Pragma>,
    problems: &mut Vec<PragmaProblem>,
) {
    let Some(at) = text.find("kndo:allow") else {
        return;
    };
    let rest = &text[at + "kndo:allow".len()..];
    let (scope, rest) = match rest.strip_prefix("-file") {
        Some(r) => (Scope::File, r),
        None => (Scope::Line, rest),
    };
    if !rest.starts_with([' ', '\t']) {
        return;
    }
    // The categories are the LEADING words that parse; the first word that is not a
    // category ends the list, and everything after it is the reason — free text, the
    // way real pragmas write it (`kndo:allow-file unused invoked by action.yml…`),
    // with an explicit `--` working the same way. Only a pragma that names NO valid
    // category is a problem worth a diagnostic.
    let mut categories = Vec::new();
    let mut first_reject = None;
    for word in rest.split([' ', '\t', ',']).filter(|w| !w.is_empty()) {
        match Category::parse(word) {
            Some(c) => categories.push(c),
            None => {
                first_reject = Some(word);
                break;
            }
        }
    }
    if categories.is_empty() {
        problems.push(PragmaProblem {
            path: path.clone(),
            level: DiagnosticLevel::Warn,
            message: match first_reject {
                Some(word) => {
                    format!("kndo:allow names no valid category (first word: `{word}`)")
                }
                None => "kndo:allow names no category".to_string(),
            },
        });
        return;
    }
    pragmas.push(Pragma {
        path: path.clone(),
        span,
        line: line_of(starts, span.start),
        categories,
        scope,
    });
}

fn comment_text(content: &[u8], text: Span) -> &str {
    let end = (text.end as usize).min(content.len());
    let start = (text.start as usize).min(end);
    std::str::from_utf8(&content[start..end]).unwrap_or("")
}

fn line_starts(content: &[u8]) -> Vec<u32> {
    let mut starts = vec![0u32];
    for (i, &b) in content.iter().enumerate() {
        if b == b'\n' {
            starts.push(i as u32 + 1);
        }
    }
    starts
}

/// 1-based.
fn line_of(starts: &[u32], byte: u32) -> u32 {
    starts.partition_point(|&s| s <= byte) as u32
}
