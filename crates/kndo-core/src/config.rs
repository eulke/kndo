//! `kndo.toml` — one tolerant parse feeding every knob the core reads. Reading follows the
//! `[plugins.gate]` posture throughout: a missing file is empty config, a malformed file or
//! value is reported as a problem string (surfaced as a run diagnostic) and otherwise
//! ignored, and unknown tables/keys are skipped silently — forward compatibility, so a file
//! written for a later kndo still works on this one. There is no longer a documented-but-unwired
//! section trading on that tolerance: `[project]` (`roots`, `exclude`) was written by
//! `kndo init` and read by nothing, and has been removed from the template and the docs rather
//! than left inert. Discovery exclusion is `.ignore`'"'"'s job and verdict scoping is `[[rule]]`'"'"'s;
//! `docs/src/configuration.md` says so where the section used to be.
//!
//! What is live: `[analysis]` (`skip`, `min-confidence`), `[analysis.crap]` (`threshold`),
//! `[analysis.duplicate]` (`min-tokens`), `[performance]` (`threads`), `[[rule]]`
//! (path-scoped `skip`), `[plugins.gate]` (parsed by `plugin_gate`, carried here so the
//! file is read exactly once), `[delta]`/`[delta.budget]` (parsed by `delta`, same reason),
//! and `[plugins.<id>]` (`report`, `max-age` — per-plugin coverage-report location and
//! freshness, RFC 0003's "Explicit config").
//!
//! Config suppression runs *after* inline pragmas ([`crate::suppression::apply`]) — pragma
//! staleness is judged against the complete pre-suppression finding set, so a pragma
//! covering a config-skipped finding stays honestly non-stale (no allow/stale flicker), and
//! a finding covered by both counts as `inline`. `stale` itself is exempt from config skip
//! (the same meta-suppression rule pragmas enforce): silencing the "your suppressions are
//! dead" signal from config would defeat its purpose. None of this feeds
//! `compute_graph_key` — every knob acts strictly post-assembly, so cached graphs stay
//! valid across config edits.

/// Every top-level table `parse` actually reads.
///
/// Exported because two things outside this file describe kndo's configuration surface — the
/// template `kndo init` writes and `docs/src/configuration.md` — and both used to be free to
/// describe a table nothing here reads. One did: `[project]`, with `roots` and `exclude`,
/// documented key by key and wired to nothing. This is the list they are checked against
/// (`kndo-cli`'s `the_init_template_offers_no_section_the_engine_ignores`), so the next unwired section fails a test instead
/// of shipping as a promise.
///
/// `[plugins.<id>]` is covered by `plugins`: the table is read, its per-plugin sub-tables are
/// open by design (a plugin's own options are its own).
pub const LIVE_TABLES: &[&str] = &[
    "analysis",
    "performance",
    "rule",
    "externally-invoked",
    "plugins",
    "delta",
];

use std::path::Path;

use smol_str::SmolStr;

use crate::engine::Finding;
use crate::vocab::{Category, Confidence, SubjectKind};

/// One `skip` entry: a category, optionally narrowed to a subject facet
/// (`"unused:enum-member"` skips only `unused` findings whose subject is an enum member).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipSpec {
    pub category: Category,
    pub subject: Option<SubjectKind>,
}

/// Why a `category[:subject]` string is not a usable spec. Both sources of skip specs — the
/// `[analysis] skip` array and the `--only`/`--skip` flags — reject the same two strings for
/// the same two reasons, so the reasons live here rather than once per source.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SkipSpecError {
    #[error("`stale` audits suppressions and cannot itself be skipped")]
    MetaSuppression,
    #[error("empty category")]
    EmptyCategory,
}

impl SkipSpec {
    /// `"unused"` or `"unused:enum-member"` — the vocabulary [suppressions] use, and now the
    /// vocabulary `--only`/`--skip` use, because they are the same policy from another source.
    ///
    /// Does not validate that the category exists: `plugin:<coordinate>/<rule>` is an open
    /// namespace (RFC 0018 §2.1), so "unknown" is not a thing this layer can decide. A
    /// frontend that wants to reject a typo checks against [`Category::ALL`] itself, where it
    /// can also say what the valid ones are.
    pub fn parse(raw: &str) -> Result<SkipSpec, SkipSpecError> {
        let (category, subject) = match raw.split_once(':') {
            // `plugin:acme/rule` is one category, not a category with a subject — the
            // namespace separator and the subject separator are the same character.
            Some(("plugin", _)) => (raw, None),
            Some((c, s)) => (c, Some(SubjectKind::new(s))),
            None => (raw, None),
        };
        if category == "stale" {
            return Err(SkipSpecError::MetaSuppression);
        }
        if category.is_empty() {
            return Err(SkipSpecError::EmptyCategory);
        }
        Ok(SkipSpec {
            category: Category::new(category),
            subject,
        })
    }

    fn covers(&self, finding: &Finding) -> bool {
        self.category == finding.category
            && self
                .subject
                .as_ref()
                .is_none_or(|s| *s == finding.subject_kind)
    }
}

/// One `[[rule]]` table: `skip` entries that apply only to findings whose `location.path`
/// matches one of `paths` (glob patterns, matched against the project-relative path).
/// A finding with no path never matches a path rule.
#[derive(Debug, Clone)]
pub struct PathRule {
    pub paths: Vec<glob::Pattern>,
    pub skip: Vec<SkipSpec>,
}

/// One `[[externally-invoked]]` table: declarations carrying one of `markers` are entry
/// points reached from outside the analyzed source, so reachability seeds them as production
/// roots. `paths`, when non-empty, scopes the rule to files whose project-relative path
/// matches one of the globs.
///
/// This is the one question static analysis cannot answer from the source alone — a Spring
/// `@Controller` is instantiated by classpath scanning and called by a servlet dispatcher, a
/// JUnit `@AfterEach` by the runner, a Koin `@Scoped` by an annotation processor in another
/// repository — and it is the project, not kndo, that knows which markers mean it. The core
/// stays ignorant: it matches strings against
/// [`crate::adapter::Declaration::markers`] and never learns what any of them are.
#[derive(Debug, Clone)]
pub struct ExternallyInvokedRule {
    pub markers: Vec<SmolStr>,
    pub paths: Vec<glob::Pattern>,
}

/// One `[plugins.<id>]` options table (RFC 0003 §"Explicit config"). Today's live keys are
/// the coverage-ingestion pair; other plugins' options (`[plugins.nextjs] app-dir`) keep
/// parsing as unknown keys until their subsystems exist — same posture as the module doc.
#[derive(Debug, Default, Clone)]
pub struct PluginOptions {
    /// `report` — glob pattern(s) naming the plugin's report file(s). REPLACES the
    /// descriptor's well-known list (explicit config wins, like every other knob); a user
    /// who wants both simply lists both. String or array of strings.
    pub report: Vec<String>,
    /// `max-age` — per-plugin freshness override for the report ("7d", "24h", or a bare
    /// integer meaning days). `None` means the engine's built-in default.
    pub max_age: Option<std::time::Duration>,
}

#[derive(Debug, Default)]
pub struct KndoConfig {
    /// `[performance] threads` — `None` when unset or `0` (both mean "physical cores").
    /// Precedence is frontend-owned: an explicit `--threads`/env override wins over this.
    pub threads: Option<usize>,
    /// `[analysis.crap] threshold` — `None` means the built-in default.
    pub crap_threshold: Option<f64>,
    /// `[analysis.duplicate] min-tokens` — `None` means the built-in default. Values below
    /// the extraction floor are clamped at parse time (fingerprints below the floor were
    /// never extracted, so a lower value could not widen detection — pretending it could
    /// would be a lie).
    pub duplicate_min_tokens: Option<u32>,
    /// `[analysis] min-confidence` — the report floor; `None` means report every tier.
    pub min_confidence: Option<Confidence>,
    /// `[analysis] skip` — project-wide category skips, counted in
    /// `SuppressedSummary::config`.
    pub skip: Vec<SkipSpec>,
    /// `[[rule]]` — path-scoped skips, same counting.
    pub rules: Vec<PathRule>,
    /// `[[externally-invoked]]` — marker-scoped entry-point declarations. Unlike `skip`, this
    /// is not a suppression: the symbol becomes genuinely reachable, so everything it reaches
    /// comes alive with it and the analyses keep judging all of it normally. A whole-path
    /// `[[rule]] skip` would silence the real findings in those files too.
    pub externally_invoked: Vec<ExternallyInvokedRule>,
    /// `[plugins.gate]` — owned by [`crate::plugin_gate`]; carried here so `kndo.toml` is
    /// parsed exactly once.
    pub(crate) plugins_gate: crate::plugin_gate::PluginsGate,
    /// `[delta]` — owned by [`crate::delta`], carried here for the same reason. `None` when
    /// the section is absent, which is NOT the same as a section of zeroes: absent evaluates
    /// no budgets and emits no `budget` block, zeroes are the strict ratchet. That is what
    /// keeps opting in a single deliberate act instead of a silent exit-code change.
    pub(crate) delta: Option<crate::delta::DeltaBudget>,
    /// `[plugins.<id>]` — per-plugin option tables, keyed by the raw TOML key; resolved
    /// against descriptor ids by [`KndoConfig::plugin_options_for`].
    pub plugin_options: Vec<(String, PluginOptions)>,
}

/// The extraction-side fingerprint floor (every adapter's `MIN_CLONE_TOKENS`): functions
/// below it carry no fingerprints in their facts, so no analysis-side threshold can reach
/// under it.
pub const DUPLICATE_MIN_TOKENS_FLOOR: u32 = 50;

/// Every knob's final value — `kndo.toml` merged under [`crate::engine::ConfigOverrides`]'s
/// frontend-supplied precedence, resolved once by [`KndoConfig::resolve`]. Before this
/// existed, `Engine::open_with_plugins` and `assemble_and_analyze` each re-derived their own
/// slice of this precedence by hand (the latter constructing `AnalysisTuning::default()`
/// twice just to pull two fallbacks) — a second merge site is how the two silently drift.
#[derive(Debug, Clone)]
pub struct EffectiveConfig {
    /// `--threads` > `kndo.toml [performance] threads` > `None` ("physical cores").
    pub threads: Option<usize>,
    /// The report floor: `--verbose`/override > `kndo.toml [analysis] min-confidence` >
    /// `Possible` (report every tier).
    pub min_confidence_floor: Confidence,
    /// `[analysis.crap]`/`[analysis.duplicate]`, each defended by `AnalysisTuning::default()`.
    pub tuning: crate::analysis::AnalysisTuning,
    /// Which findings reach the report: `--only`'s lens, then `--skip` unioned with
    /// `[analysis] skip` and `[[rule]]`. Lived on `KndoConfig` before the flags existed,
    /// which would have made the flags a second merge site — exactly what this type is for.
    pub report: ReportFilter,
}

/// Everything that decides whether a finding is *reported*, merged from both sources.
///
/// Two mechanisms, deliberately not one:
///
/// - **`only` is a lens.** It narrows this invocation's view and nothing else. Findings it
///   drops are counted and reported (`RunResult::elided`) so no one has to guess whether they
///   saw everything — that count, not an exemption, is what keeps `--only` honest. It is the
///   one filter `stale` is subject to: a lens the caller asked for this once is not a stored
///   policy that could bury the audit signal.
/// - **`skip` is suppression.** `--skip` and `[analysis] skip` mean the same thing from two
///   sources, count the same way (`SuppressedSummary::config`), and share `stale`'s exemption
///   — the "your suppressions are dead" signal must never be silenceable by the thing it
///   audits, whichever source asks.
#[derive(Debug, Default, Clone)]
pub struct ReportFilter {
    pub only: Vec<SkipSpec>,
    pub skip: Vec<SkipSpec>,
    pub rules: Vec<PathRule>,
}

/// What a report filter removed, split by mechanism because the two mean different things to
/// a reader: a suppressed finding was acknowledged, an elided one was merely not asked for.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FilterCounts {
    pub suppressed: usize,
    pub elided: usize,
}

impl ReportFilter {
    pub(crate) fn apply(&self, findings: Vec<Finding>) -> (Vec<Finding>, FilterCounts) {
        if self.only.is_empty() && self.skip.is_empty() && self.rules.is_empty() {
            return (findings, FilterCounts::default());
        }
        let mut counts = FilterCounts::default();
        let kept = findings
            .into_iter()
            .filter(|f| {
                if !self.only.is_empty() && !self.only.iter().any(|s| s.covers(f)) {
                    counts.elided += 1;
                    return false;
                }
                // The pragma meta-rule: the "your suppressions are dead" signal must never be
                // silenceable by the thing it audits.
                if f.category == "stale" {
                    return true;
                }
                let skipped = self.skip.iter().any(|s| s.covers(f))
                    || self.rules.iter().any(|rule| {
                        f.location
                            .path
                            .as_ref()
                            .is_some_and(|p| rule.paths.iter().any(|g| g.matches(p.0.as_str())))
                            && rule.skip.iter().any(|s| s.covers(f))
                    });
                if skipped {
                    counts.suppressed += 1;
                }
                !skipped
            })
            .collect();
        (kept, counts)
    }
}

impl KndoConfig {
    /// The one place `kndo.toml` and a frontend's [`crate::engine::ConfigOverrides`] merge.
    /// Every default lives here or in the types resolved through it
    /// (`AnalysisTuning::default()`) — nowhere else should fall back to a bare
    /// `.unwrap_or(...)` for one of these knobs.
    pub fn resolve(&self, overrides: &crate::engine::ConfigOverrides) -> EffectiveConfig {
        let default_tuning = crate::analysis::AnalysisTuning::default();
        EffectiveConfig {
            threads: overrides.threads.or(self.threads),
            min_confidence_floor: overrides
                .min_confidence
                .or(self.min_confidence)
                .unwrap_or(Confidence::Possible),
            tuning: crate::analysis::AnalysisTuning {
                crap_threshold: self.crap_threshold.unwrap_or(default_tuning.crap_threshold),
                duplicate_min_tokens: self
                    .duplicate_min_tokens
                    .unwrap_or(default_tuning.duplicate_min_tokens),
                externally_invoked: self.externally_invoked.clone(),
                strict: overrides.strict,
            },
            report: ReportFilter {
                only: overrides.only.clone(),
                // Union, not override: `--skip` adds to what the project already skips. A
                // flag that silently dropped the project's own list would make one CI job's
                // narrowing look like a policy change.
                skip: self
                    .skip
                    .iter()
                    .chain(overrides.skip.iter())
                    .cloned()
                    .collect(),
                rules: self.rules.clone(),
            },
        }
    }
}

impl KndoConfig {
    /// Read `<root>/kndo.toml`. Never fails: problems come back as strings for the caller
    /// to surface as run diagnostics.
    pub fn load(root: &Path) -> (KndoConfig, Vec<String>) {
        let path = root.join("kndo.toml");
        let Ok(content) = std::fs::read_to_string(&path) else {
            return (KndoConfig::default(), Vec::new());
        };
        Self::parse(&content)
    }

    fn parse(content: &str) -> (KndoConfig, Vec<String>) {
        let mut config = KndoConfig::default();
        let mut problems = Vec::new();
        let table: toml::Table = match content.parse() {
            Ok(t) => t,
            Err(e) => {
                return (
                    config,
                    vec![format!("kndo.toml is not valid TOML — ignored: {e}")],
                )
            }
        };

        if let Some(analysis) = table.get("analysis").and_then(|v| v.as_table()) {
            if let Some(value) = analysis.get("skip") {
                config.skip = parse_skip_list(value, "[analysis] skip", &mut problems);
            }
            if let Some(value) = analysis.get("min-confidence") {
                config.min_confidence = parse_confidence(value, &mut problems);
            }
            if let Some(threshold) = analysis
                .get("crap")
                .and_then(|c| c.as_table())
                .and_then(|c| c.get("threshold"))
            {
                match threshold
                    .as_float()
                    .or(threshold.as_integer().map(|i| i as f64))
                {
                    Some(t) if t > 0.0 => config.crap_threshold = Some(t),
                    _ => problems.push(format!(
                        "kndo.toml [analysis.crap] threshold = {threshold}: expected a \
                         positive number — ignored"
                    )),
                }
            }
            if let Some(min_tokens) = analysis
                .get("duplicate")
                .and_then(|d| d.as_table())
                .and_then(|d| d.get("min-tokens"))
            {
                match min_tokens.as_integer() {
                    Some(t) if t > 0 => {
                        let t = t as u32;
                        if t < DUPLICATE_MIN_TOKENS_FLOOR {
                            problems.push(format!(
                                "kndo.toml [analysis.duplicate] min-tokens = {t}: below the \
                                 extraction floor of {DUPLICATE_MIN_TOKENS_FLOOR} (smaller \
                                 functions carry no fingerprints) — clamped to \
                                 {DUPLICATE_MIN_TOKENS_FLOOR}"
                            ));
                            config.duplicate_min_tokens = Some(DUPLICATE_MIN_TOKENS_FLOOR);
                        } else {
                            config.duplicate_min_tokens = Some(t);
                        }
                    }
                    _ => problems.push(format!(
                        "kndo.toml [analysis.duplicate] min-tokens = {min_tokens}: expected \
                         a positive integer — ignored"
                    )),
                }
            }
        }

        if let Some(threads) = table
            .get("performance")
            .and_then(|p| p.as_table())
            .and_then(|p| p.get("threads"))
        {
            match threads.as_integer() {
                Some(0) => {} // 0 = the default (physical cores) — same as unset
                Some(n) if n > 0 => config.threads = Some(n as usize),
                _ => problems.push(format!(
                    "kndo.toml [performance] threads = {threads}: expected a non-negative \
                     integer — ignored"
                )),
            }
        }

        if let Some(rules) = table.get("rule").and_then(|r| r.as_array()) {
            for rule in rules {
                let Some(rule) = rule.as_table() else {
                    problems.push("kndo.toml [[rule]]: expected a table — ignored".to_string());
                    continue;
                };
                let mut paths = Vec::new();
                for raw in rule
                    .get("paths")
                    .and_then(|p| p.as_array())
                    .into_iter()
                    .flatten()
                {
                    match raw.as_str().map(glob::Pattern::new) {
                        Some(Ok(pattern)) => paths.push(pattern),
                        Some(Err(e)) => problems.push(format!(
                            "kndo.toml [[rule]] paths entry {raw}: invalid glob ({e}) — \
                             entry ignored"
                        )),
                        None => problems.push(format!(
                            "kndo.toml [[rule]] paths entry {raw}: expected a string — \
                             entry ignored"
                        )),
                    }
                }
                let skip = rule
                    .get("skip")
                    .map(|value| parse_skip_list(value, "[[rule]] skip", &mut problems))
                    .unwrap_or_default();
                if paths.is_empty() || skip.is_empty() {
                    problems.push(
                        "kndo.toml [[rule]]: needs both non-empty `paths` and `skip` — \
                         rule ignored"
                            .to_string(),
                    );
                    continue;
                }
                config.rules.push(PathRule { paths, skip });
            }
        }

        if let Some(rules) = table.get("externally-invoked").and_then(|r| r.as_array()) {
            for rule in rules {
                let Some(rule) = rule.as_table() else {
                    problems.push(
                        "kndo.toml [[externally-invoked]]: expected a table — ignored".to_string(),
                    );
                    continue;
                };
                let mut markers = Vec::new();
                for raw in rule
                    .get("markers")
                    .and_then(|m| m.as_array())
                    .into_iter()
                    .flatten()
                {
                    match raw.as_str() {
                        Some(name) if !name.trim().is_empty() => {
                            markers.push(SmolStr::new(name.trim()))
                        }
                        _ => problems.push(format!(
                            "kndo.toml [[externally-invoked]] markers entry {raw}: expected a \
                             non-empty string — entry ignored"
                        )),
                    }
                }
                let mut paths = Vec::new();
                for raw in rule
                    .get("paths")
                    .and_then(|p| p.as_array())
                    .into_iter()
                    .flatten()
                {
                    match raw.as_str().map(glob::Pattern::new) {
                        Some(Ok(pattern)) => paths.push(pattern),
                        Some(Err(e)) => problems.push(format!(
                            "kndo.toml [[externally-invoked]] paths entry {raw}: invalid glob \
                             ({e}) — entry ignored"
                        )),
                        None => problems.push(format!(
                            "kndo.toml [[externally-invoked]] paths entry {raw}: expected a \
                             string — entry ignored"
                        )),
                    }
                }
                if markers.is_empty() {
                    problems.push(
                        "kndo.toml [[externally-invoked]]: needs a non-empty `markers` list — \
                         rule ignored"
                            .to_string(),
                    );
                    continue;
                }
                config
                    .externally_invoked
                    .push(ExternallyInvokedRule { markers, paths });
            }
        }

        let (gate, gate_problems) = crate::plugin_gate::PluginsGate::from_table(
            table.get("plugins").and_then(|p| p.get("gate")),
        );
        config.plugins_gate = gate;
        problems.extend(gate_problems);

        let (delta, delta_problems) = crate::delta::DeltaBudget::from_table(table.get("delta"));
        config.delta = delta;
        problems.extend(delta_problems);

        if let Some(plugins) = table.get("plugins").and_then(|p| p.as_table()) {
            parse_plugin_options(plugins, &mut config, &mut problems);
        }

        (config, problems)
    }

    /// The `[plugins.<id>]` table for a descriptor id, if any. A bare key names a built-in
    /// without its reserved namespace (RFC 0003's promised spelling: `[plugins.coverage-lcov]`
    /// for `kndo:coverage-lcov`); a quoted key matches an id verbatim (external coordinates
    /// contain `/` and need quoting anyway).
    pub fn plugin_options_for(&self, id: &str) -> Option<&PluginOptions> {
        self.plugin_options
            .iter()
            .find(|(key, _)| *key == id || id.strip_prefix("kndo:") == Some(key.as_str()))
            .map(|(_, options)| options)
    }
}

/// Every `[plugins.<id>]` options table under `[plugins]`. `gate` is the tier policy,
/// owned by `plugin_gate` — everything else is a per-plugin table. Unknown keys inside a
/// table stay silent (forward compatibility — `[plugins.nextjs] app-dir` must keep parsing
/// as inert), and a table with no live keys is simply carried empty.
fn parse_plugin_options(
    plugins: &toml::map::Map<String, toml::Value>,
    config: &mut KndoConfig,
    problems: &mut Vec<String>,
) {
    for (key, value) in plugins {
        if key == "gate" {
            continue;
        }
        let Some(options) = value.as_table() else {
            problems.push(format!(
                "kndo.toml [plugins.{key}]: expected a table — ignored"
            ));
            continue;
        };
        let mut parsed = PluginOptions::default();
        if let Some(report) = options.get("report") {
            parsed.report = parse_report_list(report, key, problems);
        }
        if let Some(raw) = options.get("max-age") {
            parsed.max_age = parse_max_age(raw, key, problems);
        }
        config.plugin_options.push((key.clone(), parsed));
    }
}

/// `report`: a glob string or array of glob strings, validated at parse time (mirrors
/// `[[rule]] paths`): invalid entries are problems and dropped; an empty result means
/// "unset" (the descriptor's well-known list applies).
fn parse_report_list(value: &toml::Value, key: &str, problems: &mut Vec<String>) -> Vec<String> {
    let raws: Vec<&toml::Value> = match value {
        toml::Value::Array(items) => items.iter().collect(),
        single => vec![single],
    };
    let mut patterns = Vec::new();
    for raw in raws {
        match raw.as_str() {
            Some(text) => match glob::Pattern::new(text) {
                Ok(_) => patterns.push(text.to_string()),
                Err(e) => problems.push(format!(
                    "kndo.toml [plugins.{key}] report entry {raw}: invalid glob ({e}) — \
                     entry ignored"
                )),
            },
            None => problems.push(format!(
                "kndo.toml [plugins.{key}] report entry {raw}: expected a string — \
                 entry ignored"
            )),
        }
    }
    patterns
}

/// `max-age`: `"<N>d"`, `"<N>h"`, or a bare integer meaning days. Zero or unparseable →
/// problem + `None` (the built-in default applies) — problems, never failures.
fn parse_max_age(
    value: &toml::Value,
    key: &str,
    problems: &mut Vec<String>,
) -> Option<std::time::Duration> {
    let seconds = match value {
        toml::Value::Integer(days) if *days > 0 => Some(*days as u64 * 86_400),
        toml::Value::String(text) => {
            let (number, unit_seconds) = match text.strip_suffix('d') {
                Some(n) => (n, 86_400),
                None => match text.strip_suffix('h') {
                    Some(n) => (n, 3_600),
                    None => (text.as_str(), 0),
                },
            };
            match number.trim().parse::<u64>() {
                Ok(n) if n > 0 && unit_seconds > 0 => Some(n * unit_seconds),
                _ => None,
            }
        }
        _ => None,
    };
    if seconds.is_none() {
        problems.push(format!(
            "kndo.toml [plugins.{key}] max-age = {value}: expected \"<N>d\", \"<N>h\" or a \
             positive integer (days) — ignored"
        ));
    }
    seconds.map(std::time::Duration::from_secs)
}

fn parse_skip_list(
    value: &toml::Value,
    context: &str,
    problems: &mut Vec<String>,
) -> Vec<SkipSpec> {
    let Some(entries) = value.as_array() else {
        problems.push(format!(
            "kndo.toml {context} = {value}: expected an array of strings — ignored"
        ));
        return Vec::new();
    };
    let mut specs = Vec::new();
    for entry in entries {
        let Some(raw) = entry.as_str() else {
            problems.push(format!(
                "kndo.toml {context} entry {entry}: expected a string — entry ignored"
            ));
            continue;
        };
        match SkipSpec::parse(raw) {
            Ok(spec) => specs.push(spec),
            Err(e) => problems.push(format!(
                "kndo.toml {context} entry \"{raw}\": {e} — entry ignored"
            )),
        }
    }
    specs
}

fn parse_confidence(value: &toml::Value, problems: &mut Vec<String>) -> Option<Confidence> {
    match value.as_str() {
        Some("possible") => Some(Confidence::Possible),
        Some("probable") => Some(Confidence::Probable),
        Some("certain") => Some(Confidence::Certain),
        _ => {
            problems.push(format!(
                "kndo.toml [analysis] min-confidence = {value}: expected \"possible\", \
                 \"probable\", or \"certain\" — ignored"
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Location, Severity};

    fn parsed(body: &str) -> (KndoConfig, Vec<String>) {
        KndoConfig::parse(body)
    }

    fn finding(category: &str, subject: &str, path: Option<&str>) -> Finding {
        Finding {
            advisory: false,
            id: format!("{category}:{subject}:{}", path.unwrap_or("")),
            category: Category::new(category),
            group: crate::vocab::Group::Waste,
            subject_kind: SubjectKind::new(subject),
            severity: Severity::Info,
            confidence: Confidence::Certain,
            message: String::new(),
            location: Location {
                path: path.map(|p| crate::adapter::ProjectPath(smol_str::SmolStr::new(p))),
                range: None,
                symbol: None,
                package: None,
            },
            related: Vec::new(),
            rolled_up: None,
            sources: Vec::new(),
            delta: None,
            delta_origin: None,
        }
    }

    #[test]
    fn absent_file_is_empty_config() {
        let (config, problems) = KndoConfig::load(Path::new("/nonexistent-kndo-config-dir"));
        assert!(problems.is_empty());
        assert!(config.skip.is_empty() && config.rules.is_empty());
        assert!(config.threads.is_none() && config.min_confidence.is_none());
    }

    #[test]
    fn each_knob_parses_from_its_documented_table() {
        let (config, problems) = parsed(
            "[analysis]\n\
             skip = [\"unused\", \"internal-only:method\"]\n\
             min-confidence = \"probable\"\n\
             [analysis.crap]\n\
             threshold = 25\n\
             [analysis.duplicate]\n\
             min-tokens = 80\n\
             [performance]\n\
             threads = 3\n",
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(config.skip.len(), 2);
        assert_eq!(config.skip[1].category, "internal-only");
        assert_eq!(config.skip[1].subject.as_deref(), Some("method"));
        assert_eq!(config.min_confidence, Some(Confidence::Probable));
        assert_eq!(config.crap_threshold, Some(25.0));
        assert_eq!(config.duplicate_min_tokens, Some(80));
        assert_eq!(config.threads, Some(3));
    }

    #[test]
    fn zero_threads_means_the_default_like_the_template_says() {
        let (config, problems) = parsed("[performance]\nthreads = 0\n");
        assert!(problems.is_empty());
        assert!(config.threads.is_none());
    }

    #[test]
    fn min_tokens_below_the_extraction_floor_clamps_with_a_problem() {
        let (config, problems) = parsed("[analysis.duplicate]\nmin-tokens = 10\n");
        assert_eq!(
            config.duplicate_min_tokens,
            Some(DUPLICATE_MIN_TOKENS_FLOOR)
        );
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("clamped"), "{problems:?}");
    }

    #[test]
    fn malformed_values_are_problems_never_failures() {
        let (config, problems) = parsed(
            "[analysis]\n\
             skip = \"unused\"\n\
             min-confidence = \"definitely\"\n\
             [analysis.crap]\n\
             threshold = \"high\"\n\
             [performance]\n\
             threads = -2\n",
        );
        assert_eq!(problems.len(), 4, "{problems:?}");
        assert!(config.skip.is_empty());
        assert!(config.min_confidence.is_none());
        assert!(config.crap_threshold.is_none());
        assert!(config.threads.is_none());
        let (_, problems) = parsed("not toml [ at all");
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn skipping_stale_is_rejected_as_meta_suppression() {
        let (config, problems) = parsed("[analysis]\nskip = [\"stale\", \"unused\"]\n");
        assert_eq!(config.skip.len(), 1);
        assert_eq!(config.skip[0].category, "unused");
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("stale"), "{problems:?}");
    }

    #[test]
    fn unknown_tables_are_silently_ignored_for_forward_compat() {
        // `[project]` is no longer written by `kndo init` and was never read; kept here as a
        // realistic unknown table (someone'"'"'s older kndo.toml still has one). `[future]` stands in for
        // a table a newer kndo will understand. Neither may become a diagnostic — a config a
        // newer version writes has to stay readable by an older one.
        let (config, problems) = parsed("[project]\nroots = [\"src\"]\n[future]\nx = 1\n");
        assert!(problems.is_empty(), "{problems:?}");
        assert!(config.skip.is_empty());
        assert!(config.delta.is_none());
    }

    #[test]
    fn the_delta_section_reaches_the_effective_config() {
        // `[delta]` used to sit in the same test as the forward-compat tables, asserting it had
        // no effect. It has one now: `crate::delta` owns the semantics, but the single file
        // read is here, so this is the seam that has to be pinned.
        let (config, problems) = parsed("[delta]\nmax-net-findings = 3\n");
        assert!(problems.is_empty(), "{problems:?}");
        let delta = config.delta.expect("the section opts in");
        assert_eq!(delta.max_net_findings, 3);
    }

    #[test]
    fn invalid_glob_drops_the_entry_and_a_rule_needs_both_halves() {
        let (config, problems) = parsed(
            "[[rule]]\n\
             paths = [\"src/[\"]\n\
             skip = [\"unused\"]\n\
             [[rule]]\n\
             paths = [\"src/**\"]\n\
             skip = []\n",
        );
        assert!(config.rules.is_empty());
        assert_eq!(problems.len(), 3, "{problems:?}");
    }

    #[test]
    fn externally_invoked_parses_markers_and_optional_paths() {
        let (config, problems) = parsed(
            "[[externally-invoked]]\n\
             markers = [\"Controller\", \"Bean\"]\n\
             paths = [\"src/main/java/**\"]\n\
             \n\
             [[externally-invoked]]\n\
             markers = [\"AfterEach\"]\n",
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(config.externally_invoked.len(), 2);
        assert_eq!(
            config.externally_invoked[0].markers,
            vec![SmolStr::new("Controller"), SmolStr::new("Bean")]
        );
        assert!(config.externally_invoked[0].paths[0].matches("src/main/java/p/C.java"));
        assert!(
            config.externally_invoked[1].paths.is_empty(),
            "`paths` is optional — an unscoped rule applies project-wide"
        );
    }

    #[test]
    fn externally_invoked_without_markers_is_a_problem_and_is_dropped() {
        let (config, problems) = parsed("[[externally-invoked]]\npaths = [\"src/**\"]\n");
        assert!(config.externally_invoked.is_empty());
        assert!(
            problems.iter().any(|p| p.contains("non-empty `markers`")),
            "{problems:?}"
        );

        // A bad glob drops that entry and says so; the rule itself survives on its markers.
        let (config, problems) =
            parsed("[[externally-invoked]]\nmarkers = [\"Bean\"]\npaths = [\"src/[\"]\n");
        assert_eq!(config.externally_invoked.len(), 1);
        assert!(config.externally_invoked[0].paths.is_empty());
        assert!(
            problems.iter().any(|p| p.contains("invalid glob")),
            "{problems:?}"
        );
    }

    #[test]
    fn plugins_gate_rides_the_same_parse() {
        let (config, problems) = parsed("[plugins.gate]\n\"github.com/a/p\" = \"warning\"\n");
        assert!(problems.is_empty());
        assert!(config.plugins_gate.resolve("github.com/a/p", "r").is_some());
    }

    #[test]
    fn plugin_options_parse_the_rfc_0003_example_verbatim() {
        let (config, problems) =
            parsed("[plugins.coverage-lcov]\nreport = \"coverage/lcov.info\"\nmax-age = \"7d\"\n");
        assert!(problems.is_empty(), "{problems:?}");
        let opts = config.plugin_options_for("kndo:coverage-lcov").unwrap();
        assert_eq!(opts.report, vec!["coverage/lcov.info".to_string()]);
        assert_eq!(
            opts.max_age,
            Some(std::time::Duration::from_secs(7 * 86_400))
        );
        // The bare key names the built-in; a quoted full id also matches verbatim.
        let (config, _) = parsed("[plugins.\"kndo:coverage-lcov\"]\nmax-age = 3\n");
        assert!(config.plugin_options_for("kndo:coverage-lcov").is_some());
    }

    #[test]
    fn plugin_options_report_accepts_an_array_and_max_age_hours_and_days() {
        let (config, problems) = parsed(
            "[plugins.coverage-lcov]\nreport = [\"packages/*/coverage/lcov.info\", \"lcov.info\"]\n\
             [plugins.coverage-go]\nmax-age = \"36h\"\n",
        );
        assert!(problems.is_empty(), "{problems:?}");
        let lcov = config.plugin_options_for("kndo:coverage-lcov").unwrap();
        assert_eq!(lcov.report.len(), 2);
        let go = config.plugin_options_for("kndo:coverage-go").unwrap();
        assert_eq!(go.max_age, Some(std::time::Duration::from_secs(36 * 3_600)));
    }

    #[test]
    fn plugin_options_problems_never_failures() {
        let (config, problems) =
            parsed("[plugins.coverage-lcov]\nreport = [\"src/[\", 5]\nmax-age = \"soon\"\n");
        // Both report entries dropped, max-age ignored — three problems, nothing fatal.
        assert_eq!(problems.len(), 3, "{problems:?}");
        let opts = config.plugin_options_for("kndo:coverage-lcov").unwrap();
        assert!(opts.report.is_empty());
        assert!(opts.max_age.is_none());
    }

    #[test]
    fn other_plugins_option_tables_stay_inert_and_unmatched_ids_are_none() {
        let (config, problems) = parsed("[plugins.nextjs]\napp-dir = \"app\"\n");
        assert!(problems.is_empty(), "{problems:?}");
        let opts = config.plugin_options_for("kndo:nextjs").unwrap();
        assert!(opts.report.is_empty() && opts.max_age.is_none());
        assert!(config.plugin_options_for("kndo:coverage-lcov").is_none());
    }

    #[test]
    fn global_skip_filters_by_category_and_subject_but_never_stale() {
        let (config, _) = parsed("[analysis]\nskip = [\"unused\", \"internal-only:method\"]\n");
        let findings = vec![
            finding("unused", "function", Some("src/a.rs")),
            finding("internal-only", "method", Some("src/a.rs")),
            finding("internal-only", "function", Some("src/a.rs")),
            finding("stale", "suppression", Some("src/a.rs")),
        ];
        let (kept, counts) = config
            .resolve(&crate::engine::ConfigOverrides::default())
            .report
            .apply(findings);
        assert_eq!(counts.suppressed, 2);
        assert_eq!(counts.elided, 0, "no `--only` lens is in play");
        let categories: Vec<(&str, &str)> = kept
            .iter()
            .map(|f| (f.category.as_str(), f.subject_kind.as_str()))
            .collect();
        assert_eq!(
            categories,
            vec![("internal-only", "function"), ("stale", "suppression")]
        );
    }

    #[test]
    fn path_rules_scope_their_skips_to_matching_paths_only() {
        let (config, problems) =
            parsed("[[rule]]\npaths = [\"schemas/**\", \"internal/perf-baseline.json\"]\nskip = [\"unused\"]\n");
        assert!(problems.is_empty(), "{problems:?}");
        let findings = vec![
            finding("unused", "file", Some("schemas/output.json")),
            finding("unused", "file", Some("internal/perf-baseline.json")),
            finding("unused", "file", Some("src/lib.rs")),
            finding("untested", "file", Some("schemas/output.json")),
            finding("unused", "file", None),
        ];
        let (kept, counts) = config
            .resolve(&crate::engine::ConfigOverrides::default())
            .report
            .apply(findings);
        assert_eq!(counts.suppressed, 2);
        assert_eq!(kept.len(), 3);
        assert!(kept
            .iter()
            .all(|f| f.location.path.as_ref().map(|p| p.0.as_str())
                != Some("schemas/output.json")
                || f.category == "untested"));
    }
}
