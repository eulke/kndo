//! Suppression binding & marking. Comment syntax
//! is language-defined, so adapters only *extract* `kndo:allow`/`kndo:allow-file` pragmas
//! (`FileFacts::suppressions`, carried onto `ProjectGraph::suppressions` by assembly); binding,
//! validation and marking are core logic here, identical across languages.
//!
//! **No-flicker guarantee:** [`apply`] runs strictly after `analysis::run_all` has already
//! computed the complete finding set as if no pragmas existed — it only *marks*
//! matched findings (filtered from the report and `--fail-on`, still counted in
//! [`SuppressedSummary::inline`]), never influences what analyses themselves see.
//!
//! **The `stale` rule.** A pragma that isn't doing its job becomes an
//! `info` finding in group `hygiene`, subject `suppression`, at the pragma's own span — one per
//! pragma, carrying the most specific of three verdicts:
//! 1. its category isn't in the 1.0 registry — it can never match;
//! 2. a declaration-scoped pragma attaches to no declaration (the placement rule);
//! 3. it bound correctly but matched zero findings this run — the issue it acknowledged is gone.
//!
//! Because staleness is judged against the *complete* pre-suppression finding set, an
//! actively-suppressing pragma can never be `stale`, and deleting a stale pragma can never
//! resurrect a finding (no allow/stale flicker). `stale` findings are appended
//! *after* pragma matching, so they are structurally not inline-suppressible ("`stale` itself is
//! not inline-suppressible") — and a `kndo:allow stale` pragma is rejected up
//! front as meta-suppression, itself stale. `plugin:`-prefixed categories are validated against
//! the rules the active plugins actually declare; a `plugin:` pragma whose category is *not* declared this run
//! is skipped entirely (neither suppressing nor stale) — the plugin may simply be deactivated in
//! this configuration, and flagging it would flicker with activation state.
//!
//! Config-based suppression (`kndo.toml` glob/category disables) has no reader yet;
//! [`SuppressedSummary::config`] stays honestly `0` rather than faked.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::adapter::{RawSuppression, Span, SuppressionScope};
use crate::engine::{Finding, Location, SuppressedSummary};
use crate::graph::ProjectGraph;
use crate::vocab::{Confidence, FileId};

/// The 1.0 category registry — the closed set of core verdicts
/// a suppression may name. `plugin:`-namespaced categories are validated dynamically against
/// declared rules instead.
const CATEGORY_REGISTRY: [&str; 13] = [
    "unused",
    "test-only",
    "untested",
    "undeclared",
    "unresolved",
    "version-skew",
    "duplicate",
    "internal-only",
    "private-type-leak",
    "cyclic",
    "deep-import",
    "crap",
    "stale",
];

enum Scope {
    File,
    /// Line-inclusive range of the bound declaration ("the symbol and everything it
    /// declares") — any finding whose location falls inside covers both the anchor symbol
    /// itself and anything nested in it (e.g. a class's members), without needing to walk a
    /// symbol hierarchy: nested declarations are separate `SymbolNode`s with narrower spans.
    Declaration {
        start_line: u32,
        end_line: u32,
    },
}

struct Binding<'a> {
    file: FileId,
    category: &'a str,
    subject: Option<&'a str>,
    scope: Scope,
    /// The bound declaration's name (`Declaration` scope only) — the stale finding's symbol
    /// anchor, so its id survives reformatting but changes on rename.
    anchor: Option<&'a str>,
    raw: &'a RawSuppression,
    matched: bool,
}

enum StaleKind {
    UnknownCategory,
    /// `kndo:allow stale` — rejected as meta-suppression (`stale` findings are
    /// not inline-suppressible), before it could ever participate in matching.
    Meta,
    Unbound,
    MatchedNothing,
}

/// Binds every extracted pragma to what it covers, then filters findings that match a binding
/// out of `findings` — counting, never deleting the underlying fact (they're just not in the
/// returned set, same contract as [`crate::engine::Engine`]'s baseline filtering). Pragmas that
/// end up suppressing nothing come back as `stale` findings appended to the kept set (module
/// docs) — `plugin_categories` is the set of `plugin:<coordinate>/<rule>` categories the active
/// plugins declare this run, the dynamic half of category validation.
pub(crate) fn apply(
    graph: &ProjectGraph,
    findings: Vec<Finding>,
    plugin_categories: &HashSet<String>,
) -> (Vec<Finding>, SuppressedSummary) {
    if graph.suppressions.is_empty() {
        return (findings, SuppressedSummary::default());
    }

    let (mut bindings, mut stale) = classify_pragmas(graph, plugin_categories);

    let mut inline = 0usize;
    let mut kept = mark_and_filter(graph, findings, &mut bindings, &mut inline);

    stale.extend(
        bindings
            .iter()
            .filter(|b| !b.matched)
            .map(|b| (b.file, b.raw, StaleKind::MatchedNothing, b.anchor)),
    );
    append_stale_findings(graph, stale, &mut kept);

    (kept, SuppressedSummary { inline, config: 0 })
}

type StaleEntry<'a> = (FileId, &'a RawSuppression, StaleKind, Option<&'a str>);

/// Sorts every pragma into a binding (valid category, attached to its scope) or an immediate
/// stale verdict; `plugin:` categories nobody declares are dropped entirely (module docs).
fn classify_pragmas<'a>(
    graph: &'a ProjectGraph,
    plugin_categories: &HashSet<String>,
) -> (Vec<Binding<'a>>, Vec<StaleEntry<'a>>) {
    let mut bindings = Vec::new();
    let mut stale = Vec::new();
    for (file, raw) in &graph.suppressions {
        match classify_one(graph, *file, raw, plugin_categories) {
            Classified::Skip => {}
            Classified::Stale(kind) => stale.push((*file, raw, kind, None)),
            Classified::Bound(b) => bindings.push(b),
        }
    }
    (bindings, stale)
}

enum Classified<'a> {
    /// A `plugin:` category no active plugin declares — out of the game entirely.
    Skip,
    Stale(StaleKind),
    Bound(Binding<'a>),
}

fn classify_one<'a>(
    graph: &'a ProjectGraph,
    file: FileId,
    raw: &'a RawSuppression,
    plugin_categories: &HashSet<String>,
) -> Classified<'a> {
    match validate_category(raw, plugin_categories) {
        Some(verdict) => verdict,
        None => match bind_one(graph, file, raw) {
            Some(b) => Classified::Bound(b),
            None => Classified::Stale(StaleKind::Unbound),
        },
    }
}

/// The static half of pragma validation: `Some` settles the pragma's fate before binding
/// (skip or immediately stale), `None` means the category is valid and binding decides.
fn validate_category(
    raw: &RawSuppression,
    plugin_categories: &HashSet<String>,
) -> Option<Classified<'static>> {
    if raw.category.starts_with("plugin:") {
        // Only categories the active plugins actually declare participate at all — an
        // undeclared one may belong to a currently-deactivated plugin (module docs).
        return (!plugin_categories.contains(raw.category.as_str())).then_some(Classified::Skip);
    }
    if raw.category == "stale" {
        return Some(Classified::Stale(StaleKind::Meta));
    }
    (!CATEGORY_REGISTRY.contains(&raw.category.as_str()))
        .then_some(Classified::Stale(StaleKind::UnknownCategory))
}

/// The marking pass: filters findings any binding covers out of the set —
/// counting them in `inline` — while recording on each binding whether it suppressed anything.
fn mark_and_filter(
    graph: &ProjectGraph,
    findings: Vec<Finding>,
    bindings: &mut [Binding],
    inline: &mut usize,
) -> Vec<Finding> {
    let file_by_path: HashMap<&str, FileId> = graph
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| (f.path.0.as_str(), FileId(i as u32)))
        .collect();

    findings
        .into_iter()
        .filter(|f| {
            let Some(file) = f
                .location
                .path
                .as_ref()
                .and_then(|p| file_by_path.get(p.0.as_str()))
            else {
                return true; // no (resolvable) location to bind against — never suppressible
            };
            let mut suppressed = false;
            for b in bindings.iter_mut() {
                if matches(b, *file, f) {
                    b.matched = true;
                    suppressed = true;
                }
            }
            if suppressed {
                *inline += 1;
            }
            !suppressed
        })
        .collect()
}

/// Materializes the collected stale verdicts as findings, appended to `kept` in the same
/// deterministic by-id order the engine established before [`apply`] was called.
fn append_stale_findings(
    graph: &ProjectGraph,
    mut stale: Vec<StaleEntry>,
    kept: &mut Vec<Finding>,
) {
    if stale.is_empty() {
        return;
    }
    // Source order (file, then pragma span) so the per-(file, target) ordinal that
    // disambiguates duplicate pragmas in a finding id is deterministic.
    stale.sort_by_key(|(file, raw, _, _)| (file.0, raw.span.start));
    let mut ordinals: HashMap<(u32, String), u32> = HashMap::default();
    for (file, raw, kind, anchor) in stale {
        let target = pragma_target(raw);
        let ordinal = ordinals.entry((file.0, target.clone())).or_insert(0);
        kept.push(stale_finding(
            graph, file, raw, &kind, anchor, &target, *ordinal,
        ));
        *ordinal += 1;
    }
    kept.sort_unstable_by(|a, b| a.id.cmp(&b.id));
}

fn pragma_target(raw: &RawSuppression) -> String {
    match &raw.subject {
        Some(s) => format!("{}:{}", raw.category, s),
        None => raw.category.to_string(),
    }
}

/// One `stale` finding for one dead pragma (module docs): `info` / `hygiene` / subject
/// `suppression`, located at the pragma's own span. The id hashes the pragma's target and its
/// per-(file, target) ordinal — never line numbers — so reformatting keeps it stable while a
/// retargeted pragma is a new finding.
fn stale_finding(
    graph: &ProjectGraph,
    file: FileId,
    raw: &RawSuppression,
    kind: &StaleKind,
    anchor: Option<&str>,
    target: &str,
    ordinal: u32,
) -> Finding {
    let file_node = &graph.files[file.0 as usize];
    let path = file_node.path.clone();
    Finding {
        id: crate::analysis::finding_id(
            "stale",
            "suppression",
            path.0.as_str(),
            anchor.unwrap_or(""),
            &format!("{target}#{}#{ordinal}", kind.tag()),
        ),
        category: "stale".to_string(),
        group: "hygiene".to_string(),
        subject_kind: "suppression".to_string(),
        severity: crate::engine::Severity::Info,
        confidence: Confidence::Certain,
        message: stale_message(kind, &path.0, raw, anchor, target),
        location: Location {
            path: Some(path),
            range: Some(Span {
                start: raw.span.start,
                end: raw.span.end,
            }),
            symbol: anchor.map(|a| a.to_string()),
            package: graph
                .packages
                .get(file_node.package.0 as usize)
                .and_then(|p| p.name.as_ref())
                .map(|n| n.to_string()),
        },
        related: Vec::new(),
        delta: None,
        delta_origin: None,
        advisory: false,
    }
}

impl StaleKind {
    /// The finding-id discriminator tag (never shown to users).
    fn tag(&self) -> &'static str {
        match self {
            StaleKind::UnknownCategory => "unknown-category",
            StaleKind::Meta => "meta",
            StaleKind::Unbound => "unbound",
            StaleKind::MatchedNothing => "matched-nothing",
        }
    }
}

fn stale_message(
    kind: &StaleKind,
    path: &smol_str::SmolStr,
    raw: &RawSuppression,
    anchor: Option<&str>,
    target: &str,
) -> String {
    match kind {
        StaleKind::UnknownCategory => format!(
            "{path} suppression names unknown category '{}'{} — it can never match; fix or delete the pragma",
            raw.category,
            typo_hint(&raw.category)
        ),
        StaleKind::Meta => format!(
            "{path} suppression targets 'stale' — stale findings are not inline-suppressible; acknowledge them via baseline instead"
        ),
        StaleKind::Unbound => format!(
            "{path} suppression '{target}' attaches to no declaration — none starts on the pragma's line or the line after; move it directly above the declaration or use kndo:allow-file"
        ),
        StaleKind::MatchedNothing => format!(
            "{path}{} suppression '{target}' matches no finding — the issue it acknowledged is gone; delete the pragma",
            anchor.map(|a| format!("#{a}")).unwrap_or_default()
        ),
    }
}

/// " (did you mean '…'?)" for a near-miss of a registry category, empty otherwise.
fn typo_hint(cat: &str) -> String {
    nearest_category(cat)
        .map(|c| format!(" (did you mean '{c}'?)"))
        .unwrap_or_default()
}

/// Closest registry category within edit distance 2 — a typo hint, not a correction.
fn nearest_category(cat: &str) -> Option<&'static str> {
    CATEGORY_REGISTRY
        .iter()
        .map(|c| (edit_distance(cat, c), *c))
        .min()
        .filter(|(d, _)| *d <= 2)
        .map(|(_, c)| c)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut row = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            row.push(sub.min(prev[j + 1] + 1).min(row[j] + 1));
        }
        prev = row;
    }
    prev[b.len()]
}

/// A `Declaration`-scope pragma "attaches to the declaration it precedes or shares a line with"
///: the pragma's own line matches the declaration's start line (a trailing
/// same-line comment), or the declaration starts on the line right after the pragma ends (a
/// comment directly above it). `None` when nothing in the file qualifies — the pragma binds to
/// nothing, an `Unbound` stale finding (module docs).
fn bind_one<'a>(
    graph: &'a ProjectGraph,
    file: FileId,
    raw: &'a RawSuppression,
) -> Option<Binding<'a>> {
    let (scope, anchor) = match raw.scope {
        SuppressionScope::File => (Scope::File, None),
        SuppressionScope::Declaration => {
            let anchor = graph.symbols.iter().find(|s| {
                s.file == file
                    && (s.span.start.0 == raw.span.start.0 || s.span.start.0 == raw.span.end.0 + 1)
            })?;
            (
                Scope::Declaration {
                    start_line: anchor.span.start.0,
                    end_line: anchor.span.end.0,
                },
                Some(anchor.name.as_str()),
            )
        }
    };
    Some(Binding {
        file,
        category: raw.category.as_str(),
        subject: raw.subject.as_deref(),
        scope,
        anchor,
        raw,
        matched: false,
    })
}

fn matches(binding: &Binding, file: FileId, finding: &Finding) -> bool {
    if binding.file != file || binding.category != finding.category {
        return false;
    }
    if let Some(subject) = binding.subject {
        if subject != finding.subject_kind {
            return false;
        }
    }
    match &binding.scope {
        Scope::File => true,
        Scope::Declaration {
            start_line,
            end_line,
        } => finding
            .location
            .range
            .is_some_and(|r| r.start.0 >= *start_line && r.start.0 <= *end_line),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ProjectPath, Span, VisibilityLevel};
    use crate::engine::{Location, Severity};
    use crate::graph::{FileNode, SymbolNode};
    use crate::vocab::{Confidence, PackageId, SymbolKind};
    use smol_str::SmolStr;

    fn graph_with(
        symbols: Vec<SymbolNode>,
        suppressions: Vec<(FileId, RawSuppression)>,
    ) -> ProjectGraph {
        ProjectGraph::for_test(
            vec![FileNode {
                path: ProjectPath(SmolStr::new("a.ts")),
                content_hash: [0u8; 32],
                language: Some(SmolStr::new("mock")),
                class: None,
                package: PackageId(0),
                unit: None,
                test_spans: Vec::new(),
                string_call_sites: Vec::new(),
            }],
            symbols,
            vec![],
            vec![],
        )
        .with_suppressions(suppressions)
    }

    fn symbol(name: &str, start_line: u32, end_line: u32) -> SymbolNode {
        SymbolNode {
            file: FileId(0),
            name: SmolStr::new(name),
            kind: SymbolKind::Function,
            span: Span {
                start: (start_line, 1),
                end: (end_line, 1),
            },
            exported: false,
            visibility: VisibilityLevel(0),
            member_of: None,
            signature_span: None,
            implicitly_invoked: false,
        }
    }

    fn suppression(
        category: &str,
        subject: Option<&str>,
        scope: SuppressionScope,
        start_line: u32,
        end_line: u32,
    ) -> RawSuppression {
        RawSuppression {
            span: Span {
                start: (start_line, 1),
                end: (end_line, 1),
            },
            category: SmolStr::new(category),
            subject: subject.map(SmolStr::new),
            reason: None,
            scope,
        }
    }

    fn finding_at(category: &str, subject_kind: &str, line: u32) -> Finding {
        Finding {
            advisory: false,
            id: format!("kndo-{category}-{line}"),
            category: category.to_string(),
            group: "waste".to_string(),
            subject_kind: subject_kind.to_string(),
            severity: Severity::Warning,
            confidence: Confidence::Certain,
            message: "example".to_string(),
            location: Location {
                path: Some(ProjectPath(SmolStr::new("a.ts"))),
                range: Some(Span {
                    start: (line, 1),
                    end: (line, 10),
                }),
                symbol: None,
                package: None,
            },
            related: Vec::new(),
            delta: None,
            delta_origin: None,
        }
    }

    fn apply_no_plugins(
        graph: &ProjectGraph,
        findings: Vec<Finding>,
    ) -> (Vec<Finding>, SuppressedSummary) {
        apply(graph, findings, &HashSet::default())
    }

    fn stale_of(kept: &[Finding]) -> Vec<&Finding> {
        kept.iter().filter(|f| f.category == "stale").collect()
    }

    #[test]
    fn pragma_directly_above_a_declaration_suppresses_its_finding() {
        let graph = graph_with(
            vec![symbol("foo", 3, 5)],
            vec![(
                FileId(0),
                suppression("unused", None, SuppressionScope::Declaration, 2, 2),
            )],
        );
        let (kept, summary) = apply_no_plugins(&graph, vec![finding_at("unused", "function", 3)]);
        assert!(
            kept.is_empty(),
            "an actively-suppressing pragma is never stale"
        );
        assert_eq!(summary.inline, 1);
        assert_eq!(summary.config, 0);
    }

    #[test]
    fn trailing_same_line_pragma_suppresses_its_finding() {
        let graph = graph_with(
            vec![symbol("foo", 3, 3)],
            vec![(
                FileId(0),
                suppression("unused", None, SuppressionScope::Declaration, 3, 3),
            )],
        );
        let (kept, summary) = apply_no_plugins(&graph, vec![finding_at("unused", "function", 3)]);
        assert!(kept.is_empty());
        assert_eq!(summary.inline, 1);
    }

    #[test]
    fn declaration_scope_covers_a_nested_member_within_the_same_span() {
        // class Baz { ... } spans lines 3-10; a member declared inside it (line 6) is a
        // separate SymbolNode but its finding still falls inside the anchor's line range.
        let graph = graph_with(
            vec![symbol("Baz", 3, 10)],
            vec![(
                FileId(0),
                suppression("unused", None, SuppressionScope::Declaration, 2, 2),
            )],
        );
        let (kept, summary) = apply_no_plugins(&graph, vec![finding_at("unused", "method", 6)]);
        assert!(kept.is_empty());
        assert_eq!(summary.inline, 1);
    }

    #[test]
    fn subject_facet_only_matches_the_named_subject_kind() {
        let graph = graph_with(
            vec![symbol("foo", 3, 3)],
            vec![(
                FileId(0),
                suppression(
                    "unused",
                    Some("enum-member"),
                    SuppressionScope::Declaration,
                    3,
                    3,
                ),
            )],
        );
        let (kept, summary) = apply_no_plugins(&graph, vec![finding_at("unused", "function", 3)]);
        assert_eq!(summary.inline, 0);
        let stale = stale_of(&kept);
        assert_eq!(
            kept.len() - stale.len(),
            1,
            "subject facet must not match a different subject_kind"
        );
        assert_eq!(stale.len(), 1, "an ineffective pragma is stale");
        assert!(stale[0].message.contains("unused:enum-member"));
    }

    #[test]
    fn file_scope_suppresses_regardless_of_location() {
        let graph = graph_with(
            vec![],
            vec![(
                FileId(0),
                suppression("version-skew", None, SuppressionScope::File, 1, 1),
            )],
        );
        let (kept, summary) =
            apply_no_plugins(&graph, vec![finding_at("version-skew", "dependency", 40)]);
        assert!(kept.is_empty());
        assert_eq!(summary.inline, 1);
    }

    #[test]
    fn an_unbound_pragma_suppresses_nothing_and_is_stale() {
        // No declaration precedes or shares line 2 — the pragma is orphaned.
        let graph = graph_with(
            vec![symbol("foo", 10, 10)],
            vec![(
                FileId(0),
                suppression("unused", None, SuppressionScope::Declaration, 2, 2),
            )],
        );
        let (kept, summary) = apply_no_plugins(&graph, vec![finding_at("unused", "function", 10)]);
        assert_eq!(summary.inline, 0);
        assert_eq!(kept.len(), 2, "the unsuppressed finding plus one stale");
        let stale = stale_of(&kept);
        assert_eq!(stale.len(), 1);
        assert!(stale[0].message.contains("attaches to no declaration"));
        assert_eq!(stale[0].group, "hygiene");
        assert_eq!(stale[0].subject_kind, "suppression");
        assert_eq!(stale[0].severity, Severity::Info);
        assert_eq!(
            stale[0].location.range.map(|r| r.start.0),
            Some(2),
            "located at the pragma's own span"
        );
    }

    #[test]
    fn a_bound_pragma_matching_nothing_is_stale_with_its_anchor_named() {
        let graph = graph_with(
            vec![symbol("foo", 3, 3)],
            vec![(
                FileId(0),
                suppression("unused", None, SuppressionScope::Declaration, 3, 3),
            )],
        );
        let (kept, summary) =
            apply_no_plugins(&graph, vec![finding_at("test-only", "function", 3)]);
        assert_eq!(summary.inline, 0);
        let stale = stale_of(&kept);
        assert_eq!(stale.len(), 1);
        assert!(stale[0].message.contains("a.ts#foo"));
        assert!(stale[0].message.contains("matches no finding"));
        assert_eq!(stale[0].location.symbol.as_deref(), Some("foo"));
    }

    #[test]
    fn an_unknown_category_pragma_is_stale_with_a_typo_hint() {
        let graph = graph_with(
            vec![symbol("foo", 3, 3)],
            vec![(
                FileId(0),
                suppression("unusedd", None, SuppressionScope::Declaration, 3, 3),
            )],
        );
        let (kept, _) = apply_no_plugins(&graph, vec![]);
        let stale = stale_of(&kept);
        assert_eq!(stale.len(), 1);
        assert!(stale[0].message.contains("unknown category 'unusedd'"));
        assert!(
            stale[0].message.contains("did you mean 'unused'?"),
            "{}",
            stale[0].message
        );
    }

    #[test]
    fn a_plugin_category_pragma_is_skipped_when_no_active_plugin_declares_it() {
        // The plugin may simply be deactivated in this configuration — neither suppressing
        // nor stale (no flicker with activation state).
        let graph = graph_with(
            vec![],
            vec![(
                FileId(0),
                suppression(
                    "plugin:kndo:express/route-shadowed",
                    None,
                    SuppressionScope::File,
                    1,
                    1,
                ),
            )],
        );
        let (kept, summary) = apply_no_plugins(&graph, vec![]);
        assert!(kept.is_empty());
        assert_eq!(summary.inline, 0);
    }

    #[test]
    fn a_declared_plugin_category_participates_fully() {
        let declared: HashSet<String> = ["plugin:kndo:express/route-shadowed".to_string()]
            .into_iter()
            .collect();
        let graph = graph_with(
            vec![],
            vec![(
                FileId(0),
                suppression(
                    "plugin:kndo:express/route-shadowed",
                    None,
                    SuppressionScope::File,
                    1,
                    1,
                ),
            )],
        );
        // Matching a plugin finding suppresses it…
        let (kept, summary) = apply(
            &graph,
            vec![finding_at("plugin:kndo:express/route-shadowed", "call", 9)],
            &declared,
        );
        assert!(kept.is_empty());
        assert_eq!(summary.inline, 1);
        // …and a declared-but-matchless pragma is stale like any core one.
        let (kept, _) = apply(&graph, vec![], &declared);
        assert_eq!(stale_of(&kept).len(), 1);
    }

    #[test]
    fn stale_findings_are_not_inline_suppressible() {
        // `stale` itself is not inline-suppressible — an allow-file pragma for
        // it neither hides the other pragma's staleness nor escapes staleness itself.
        let graph = graph_with(
            vec![],
            vec![
                (
                    FileId(0),
                    suppression("stale", None, SuppressionScope::File, 1, 1),
                ),
                (
                    FileId(0),
                    suppression("unused", None, SuppressionScope::Declaration, 5, 5),
                ),
            ],
        );
        let (kept, summary) = apply_no_plugins(&graph, vec![]);
        assert_eq!(summary.inline, 0);
        let stale = stale_of(&kept);
        assert_eq!(
            stale.len(),
            2,
            "the orphaned pragma AND the allow-stale pragma are both stale"
        );
        assert!(
            stale
                .iter()
                .any(|f| f.message.contains("not inline-suppressible")),
            "the meta-suppression is called out as such"
        );
    }

    #[test]
    fn duplicate_dead_pragmas_get_distinct_ids() {
        let graph = graph_with(
            vec![],
            vec![
                (
                    FileId(0),
                    suppression("unused", None, SuppressionScope::Declaration, 2, 2),
                ),
                (
                    FileId(0),
                    suppression("unused", None, SuppressionScope::Declaration, 7, 7),
                ),
            ],
        );
        let (kept, _) = apply_no_plugins(&graph, vec![]);
        let stale = stale_of(&kept);
        assert_eq!(stale.len(), 2);
        assert_ne!(stale[0].id, stale[1].id);
    }

    #[test]
    fn no_suppressions_is_a_cheap_no_op() {
        let graph = graph_with(vec![], vec![]);
        let (kept, summary) = apply_no_plugins(&graph, vec![finding_at("unused", "function", 3)]);
        assert_eq!(kept.len(), 1);
        assert_eq!(summary.inline, 0);
        assert_eq!(summary.config, 0);
    }

    #[test]
    fn a_finding_with_no_location_path_is_never_suppressible() {
        let graph = graph_with(
            vec![],
            vec![(
                FileId(0),
                suppression("version-skew", None, SuppressionScope::File, 1, 1),
            )],
        );
        let mut f = finding_at("version-skew", "dependency", 1);
        f.location.path = None;
        let (kept, summary) = apply_no_plugins(&graph, vec![f]);
        assert_eq!(summary.inline, 0);
        assert_eq!(kept.len() - stale_of(&kept).len(), 1);
    }
}
