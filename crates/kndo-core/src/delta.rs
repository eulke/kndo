//! Delta budgets: `[delta]` in `kndo.toml`. The file itself is parsed once by
//! [`crate::config`]; this module owns the section's vocabulary and semantics.
//!
//! ```toml
//! [delta]                  # the strict ratchet is the default INSIDE the section
//! max-health-drop = 0.0    # health never drops
//! max-net-findings = 0     # new − fixed ≤ 0: pay for what you dirty
//!
//! [delta.budget]           # finer tolerances, by group or category
//! defect = 0               # new defects: never
//! duplicate = 2            # up to 2 new clones tolerated per change
//! ```
//!
//! Three semantics RFC 0006 §5 fixes, each of which is a decision rather than an
//! implementation detail:
//!
//! - **`fixed` compensates only inside `max-net-findings`.** Per-group and per-category
//!   budgets are absolute: `defect = 0` means zero new defects even if the change fixes ten
//!   others. Netting everywhere would let a change trade a fixed typo for a new security
//!   defect and call it even.
//! - **Budgets judge the change, never the debt.** They are evaluated against the merge-base
//!   (which is what diff mode already compares against), so pushing a fix to the same branch
//!   cannot "recharge" an allowance, and the baseline never grows because a budget allowed
//!   something through.
//! - **No `[delta]` section, no evaluation and no `budget` field at all.** The strict-ratchet
//!   default applies *within* the section — writing `[delta]` with only `max-net-findings`
//!   leaves `max-health-drop` at 0.0 — so opting in is one deliberate act and no existing
//!   project silently changes exit code.
//!
//! Advisory findings (plugin findings without a `[plugins.gate]` opt-in, RFC 0018 §2.2) are
//! excluded from every count here, the same way [`crate::engine::RunResult::fails_at`]
//! excludes them: installing a finding-emitting plugin must never move someone's gate.

use crate::engine::Finding;

/// One configured limit and what the change measured against it.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BudgetRule {
    /// Lower-kebab, as configured: `max-health-drop`, `max-net-findings`, or the
    /// group/category name under `[delta.budget]`.
    pub rule: String,
    pub limit: f64,
    pub measured: f64,
    pub verdict: BudgetVerdict,
    /// How far past `limit` the measurement landed — present only when this rule failed, so a
    /// consumer reading it always has a number worth acting on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub over_by: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum BudgetVerdict {
    Pass,
    Fail,
}

/// The whole `[delta]` evaluation for one run — the `budget` block of the output schema.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Budget {
    /// `fail` when any rule failed. The gate reads this; `rules` explains it.
    pub verdict: BudgetVerdict,
    pub rules: Vec<BudgetRule>,
}

impl Budget {
    pub fn failed(&self) -> bool {
        self.verdict == BudgetVerdict::Fail
    }

    /// Rules that held, over rules configured — what the agent line's `(2/3)` reports.
    pub fn passed_of_total(&self) -> (usize, usize) {
        (
            self.rules
                .iter()
                .filter(|r| r.verdict == BudgetVerdict::Pass)
                .count(),
            self.rules.len(),
        )
    }
}

/// The configured limits. `None` at the top level means no `[delta]` section at all, which is
/// not the same as a section full of zeroes: the first evaluates nothing, the second is the
/// strict ratchet.
#[derive(Debug, Clone, Default)]
pub struct DeltaBudget {
    pub(crate) max_health_drop: f64,
    pub(crate) max_net_findings: i64,
    /// Per-group and per-category tolerances, sorted by key so the reported rule order is
    /// deterministic regardless of TOML iteration.
    pub(crate) per_subject: Vec<(String, i64)>,
}

impl DeltaBudget {
    /// Parse the `[delta]` value out of an already-parsed `kndo.toml` ([`crate::config`] owns
    /// the single file read). `None` (section absent) is `None` here too — the caller must be
    /// able to tell "not configured" from "configured to zero". Problems come back as strings,
    /// never a failure, matching every other knob.
    pub(crate) fn from_table(delta: Option<&toml::Value>) -> (Option<DeltaBudget>, Vec<String>) {
        let Some(table) = delta.and_then(|d| d.as_table()) else {
            return (None, Vec::new());
        };
        let mut budget = DeltaBudget::default();
        let mut problems = Vec::new();

        if let Some(v) = table.get("max-health-drop") {
            // Accepts `0` as well as `0.0`: the template writes one and users write the other.
            match v.as_float().or(v.as_integer().map(|i| i as f64)) {
                Some(d) if d >= 0.0 => budget.max_health_drop = d,
                _ => problems.push(format!(
                    "kndo.toml [delta] max-health-drop = {v}: expected a non-negative number \
                     (the largest tolerated DROP) — ignored, using 0.0"
                )),
            }
        }
        if let Some(v) = table.get("max-net-findings") {
            match v.as_integer() {
                Some(n) if n >= 0 => budget.max_net_findings = n,
                _ => problems.push(format!(
                    "kndo.toml [delta] max-net-findings = {v}: expected a non-negative \
                     integer — ignored, using 0"
                )),
            }
        }
        if let Some(sub) = table.get("budget") {
            match sub.as_table() {
                Some(entries) => {
                    for (key, value) in entries {
                        match value.as_integer() {
                            Some(n) if n >= 0 => budget.per_subject.push((key.clone(), n)),
                            _ => problems.push(format!(
                                "kndo.toml [delta.budget] {key} = {value}: expected a \
                                 non-negative integer — ignored"
                            )),
                        }
                    }
                    budget.per_subject.sort();
                }
                None => problems.push(
                    "kndo.toml [delta.budget]: expected a table of group/category \
                     tolerances — ignored"
                        .to_string(),
                ),
            }
        }
        (Some(budget), problems)
    }

    /// Judge one diff-mode run. `health_drop` is `before − after`, so a positive number is a
    /// regression and `max-health-drop = 0.0` reads as "must not go down".
    ///
    /// There is deliberately no "health could not be measured" case: a run that failed to
    /// assemble either side returns before a budget is built at all, so the envelope carries
    /// no `budget` block rather than a passing one. That distinction lives at the call site
    /// (`Engine::run_diff`) because it is the only place that can tell the two apart.
    pub(crate) fn evaluate(
        &self,
        new_findings: &[Finding],
        fixed_findings: &[Finding],
        health_drop: f64,
    ) -> Budget {
        let mut rules = vec![rule("max-health-drop", self.max_health_drop, health_drop)];
        let gated = |f: &&Finding| !f.advisory;
        let net = new_findings.iter().filter(gated).count() as i64
            - fixed_findings.iter().filter(gated).count() as i64;
        rules.push(rule(
            "max-net-findings",
            self.max_net_findings as f64,
            net as f64,
        ));
        for (subject, limit) in &self.per_subject {
            // Absolute, not net: `fixed` never compensates here (see the module doc).
            let measured = new_findings
                .iter()
                .filter(gated)
                .filter(|f| f.group.as_str() == subject || f.category.as_str() == subject)
                .count() as i64;
            rules.push(rule(subject, *limit as f64, measured as f64));
        }
        let verdict = if rules.iter().any(|r| r.verdict == BudgetVerdict::Fail) {
            BudgetVerdict::Fail
        } else {
            BudgetVerdict::Pass
        };
        Budget { verdict, rules }
    }
}

fn rule(name: &str, limit: f64, measured: f64) -> BudgetRule {
    let over = measured - limit;
    // Strictly over, so a measurement that lands exactly on the limit passes — `max-` reads as
    // "at most", and a ratchet that failed at its own stated maximum would be unusable.
    let failed = over > 0.0;
    BudgetRule {
        rule: name.to_string(),
        limit,
        measured,
        verdict: if failed {
            BudgetVerdict::Fail
        } else {
            BudgetVerdict::Pass
        },
        over_by: failed.then_some(over),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Location, Severity};
    use crate::vocab::{Category, Confidence, Group, SubjectKind};

    fn parsed(toml_src: &str) -> (Option<DeltaBudget>, Vec<String>) {
        let table: toml::Value = toml::from_str(toml_src).unwrap();
        DeltaBudget::from_table(table.get("delta"))
    }

    fn finding(category: &str, group: Group, advisory: bool) -> Finding {
        Finding {
            id: format!("id-{category}-{advisory}"),
            category: Category::new(category),
            group,
            subject_kind: SubjectKind::FILE,
            severity: Severity::Warning,
            confidence: Confidence::Certain,
            message: String::new(),
            location: Location::default(),
            related: Vec::new(),
            rolled_up: None,
            sources: Vec::new(),
            delta: None,
            delta_origin: None,
            advisory,
        }
    }

    #[test]
    fn no_delta_section_is_not_the_same_as_a_section_of_zeroes() {
        // The distinction the whole opt-in rests on: absent evaluates nothing and emits no
        // `budget` block; present-but-empty is the strict ratchet.
        assert!(parsed("[analysis]\nskip = []\n").0.is_none());
        let present = parsed("[delta]\n")
            .0
            .expect("an empty section still opts in");
        assert_eq!(present.max_health_drop, 0.0);
        assert_eq!(present.max_net_findings, 0);
    }

    #[test]
    fn an_integer_is_accepted_where_a_float_is_expected() {
        // The template writes `0.0`; a user writes `0`. Rejecting one of them would be a
        // parser detail leaking into the config surface.
        let b = parsed("[delta]\nmax-health-drop = 2\n").0.unwrap();
        assert_eq!(b.max_health_drop, 2.0);
    }

    #[test]
    fn a_bad_value_is_a_diagnostic_and_the_ratchet_holds() {
        // Never a hard failure, and never a silently loosened gate: a garbage limit falls back
        // to the strict default rather than to "no limit".
        let (budget, problems) =
            parsed("[delta]\nmax-net-findings = -3\nmax-health-drop = \"lots\"\n");
        let budget = budget.unwrap();
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert_eq!(budget.max_net_findings, 0);
        assert_eq!(budget.max_health_drop, 0.0);
    }

    #[test]
    fn per_subject_tolerances_parse_sorted_and_absolute() {
        let b = parsed("[delta.budget]\nduplicate = 2\ndefect = 0\n")
            .0
            .unwrap();
        assert_eq!(
            b.per_subject,
            vec![("defect".to_string(), 0), ("duplicate".to_string(), 2)],
            "sorted, so the reported rule order does not depend on TOML iteration"
        );
    }

    #[test]
    fn fixed_findings_compensate_the_net_rule_and_nothing_else() {
        // RFC 0006 §5: `fixed` compensates only inside `max-net-findings`; a per-group budget
        // is absolute. Otherwise a change could trade a fixed typo for a new defect and call
        // it even.
        let mut b = DeltaBudget::default();
        b.per_subject.push(("defect".to_string(), 0));
        let new = vec![finding("unresolved", Group::Defect, false)];
        let fixed = vec![
            finding("unused", Group::Waste, false),
            finding("duplicate", Group::Waste, false),
        ];
        let budget = b.evaluate(&new, &fixed, 0.0);

        let net = budget
            .rules
            .iter()
            .find(|r| r.rule == "max-net-findings")
            .unwrap();
        assert_eq!(net.measured, -1.0, "1 new − 2 fixed");
        assert_eq!(net.verdict, BudgetVerdict::Pass);

        let defect = budget.rules.iter().find(|r| r.rule == "defect").unwrap();
        assert_eq!(
            defect.measured, 1.0,
            "absolute: the two fixes do not pay for it"
        );
        assert_eq!(defect.verdict, BudgetVerdict::Fail);
        assert_eq!(defect.over_by, Some(1.0));
        assert!(budget.failed());
    }

    #[test]
    fn advisory_findings_never_move_a_budget() {
        // RFC 0018 §2.2 — installing a finding-emitting plugin must not move anyone's gate.
        let b = DeltaBudget::default();
        let new = vec![
            finding("plugin:acme/x", Group::Convention, true),
            finding("plugin:acme/y", Group::Convention, true),
        ];
        let budget = b.evaluate(&new, &[], 0.0);
        let net = budget
            .rules
            .iter()
            .find(|r| r.rule == "max-net-findings")
            .unwrap();
        assert_eq!(net.measured, 0.0);
        assert!(!budget.failed());
    }

    #[test]
    fn a_measurement_exactly_on_the_limit_passes() {
        // `max-` reads as "at most". A ratchet that failed at its own stated maximum would be
        // unusable, and off-by-one here is the difference between a gate and a nuisance.
        let b = DeltaBudget {
            max_health_drop: 1.0,
            max_net_findings: 1,
            per_subject: Vec::new(),
        };
        let new = vec![finding("unused", Group::Waste, false)];
        let budget = b.evaluate(&new, &[], 1.0);
        assert!(!budget.failed(), "{:?}", budget.rules);
        assert_eq!(budget.passed_of_total(), (2, 2));
    }

    #[test]
    fn a_health_gain_is_never_a_failure() {
        // The rule measures a DROP, so an improvement is a negative measurement and must sit
        // comfortably under a 0.0 limit — a sign error here would fail every improving change.
        let b = DeltaBudget::default();
        let budget = b.evaluate(&[], &[], -1.7);
        let health = budget
            .rules
            .iter()
            .find(|r| r.rule == "max-health-drop")
            .unwrap();
        assert_eq!(health.measured, -1.7);
        assert_eq!(health.verdict, BudgetVerdict::Pass);
        assert_eq!(health.over_by, None);
        assert_eq!(budget.passed_of_total(), (2, 2));
    }
}
