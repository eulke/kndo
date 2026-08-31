//! Project health: how much of the judged graph is implicated by findings, as a
//! ratio of two counted integers — never a weighted score. The universe is the
//! graph's own: every declaration plus every claimed file. A finding counts when
//! it is first-party (extension findings are advisory by the two-tier decision),
//! warning or worse (`Info` is the advisory severity tier), and lands on a
//! subject inside that universe (symbol or file; dependency-shaped findings join
//! when their own universe — declared dependencies — is a counted thing).
//! Subjects are counted DISTINCT: a function that is both unused and duplicated
//! is one problem unit, not two penalties.
//!
//! Health measures the CURRENT tree. The baseline acknowledges debt and hides it
//! from the report's listing — never from health: baselining everything must not
//! read as getting healthier. Suppression is the opposite edge: an in-code
//! `kndo:allow` is a human verdict overriding the analysis, so suppressed
//! findings implicate nothing. And there is no `previous`, no delta, no clock:
//! health is a pure function of this tree, byte-stable under the equivalence
//! gates like everything else in the envelope.

use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::Subject;
use kndo_contract::vocab::Category;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Health {
    /// Distinct universe subjects carrying at least one counting finding.
    pub implicated: u32,
    /// The judged universe: every declaration plus every claimed file.
    pub subjects: u32,
    /// Counting findings per category, in category order. More entries than
    /// `implicated` when one subject carries findings from several categories.
    pub by_category: Vec<CategoryCount>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CategoryCount {
    pub category: Category,
    pub findings: u32,
}

impl Health {
    /// `None` when reachability itself never judged: with `unused` abstained the
    /// implicated set would be a fiction, and absent health is honest health —
    /// the abstention channel already says why.
    pub fn measure(
        findings: &[Finding],
        subjects: u32,
        judged: &BTreeSet<Category>,
    ) -> Option<Health> {
        if subjects == 0 || !judged.contains(&Category::UNUSED) {
            return None;
        }
        let mut implicated: HashSet<&Subject> = HashSet::new();
        let mut by_category: BTreeMap<&Category, u32> = BTreeMap::new();
        for finding in findings.iter().filter(|f| counts(f)) {
            implicated.insert(&finding.subject);
            *by_category.entry(&finding.category).or_insert(0) += 1;
        }
        Some(Health {
            implicated: implicated.len() as u32,
            subjects,
            by_category: by_category
                .into_iter()
                .map(|(category, findings)| CategoryCount {
                    category: category.clone(),
                    findings,
                })
                .collect(),
        })
    }

    /// The score as every frontend prints it — `100 × (1 − implicated/subjects)`
    /// to one decimal, in the one place, so no render keeps its own arithmetic.
    pub fn score_text(&self) -> String {
        let clean = 1.0 - f64::from(self.implicated) / f64::from(self.subjects);
        format!("{:.1}", 100.0 * clean)
    }
}

fn counts(finding: &Finding) -> bool {
    !finding.category.is_extension()
        && finding.severity.at_least(Severity::Warning)
        && matches!(
            finding.subject,
            Subject::Symbol { .. } | Subject::File { .. }
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_contract::subject::SymbolSelector;
    use kndo_contract::vocab::{Confidence, ProjectPath, Span};
    use smol_str::SmolStr;

    fn symbol(path: &str, name: &str) -> Subject {
        Subject::Symbol {
            path: ProjectPath::new(path),
            selector: SymbolSelector::Free(SmolStr::new(name)),
            span: Span::new(0, 10),
        }
    }

    fn finding(category: Category, severity: Severity, subject: Subject) -> Finding {
        Finding::new(category, severity, Confidence::Certain, subject, "", "m")
    }

    fn judged_with_unused() -> BTreeSet<Category> {
        BTreeSet::from([Category::UNUSED])
    }

    #[test]
    fn distinct_subjects_count_once_across_categories() {
        let hot = symbol("a.py", "f");
        let findings = vec![
            finding(Category::UNUSED, Severity::Warning, hot.clone()),
            finding(Category::DUPLICATE, Severity::Warning, hot),
            finding(Category::UNUSED, Severity::Warning, symbol("b.py", "g")),
        ];
        let h = Health::measure(&findings, 10, &judged_with_unused()).unwrap();
        assert_eq!(h.implicated, 2, "one subject, one problem unit");
        assert_eq!(h.subjects, 10);
        assert_eq!(h.score_text(), "80.0");
        let cats: Vec<(&str, u32)> = h
            .by_category
            .iter()
            .map(|c| (c.category.as_str(), c.findings))
            .collect();
        assert_eq!(cats, [("duplicate", 1), ("unused", 2)]);
    }

    #[test]
    fn advisory_tiers_never_implicate() {
        let findings = vec![
            // Info is the advisory severity tier.
            finding(Category::INTERNAL_ONLY, Severity::Info, symbol("a.py", "f")),
            // Extension findings are advisory by the two-tier decision.
            finding(
                Category::extension("demo:probe", "note"),
                Severity::Warning,
                symbol("a.py", "g"),
            ),
            // A subject outside the graph universe measures nothing here.
            finding(
                Category::STALE,
                Severity::Warning,
                Subject::Suppression {
                    path: ProjectPath::new("a.py"),
                    span: Span::new(0, 5),
                },
            ),
        ];
        let h = Health::measure(&findings, 4, &judged_with_unused()).unwrap();
        assert_eq!(h.implicated, 0);
        assert!(h.by_category.is_empty());
        assert_eq!(h.score_text(), "100.0");
    }

    #[test]
    fn no_reachability_judgment_means_no_health() {
        assert!(Health::measure(&[], 5, &BTreeSet::new()).is_none());
        assert!(Health::measure(&[], 0, &judged_with_unused()).is_none());
    }
}
