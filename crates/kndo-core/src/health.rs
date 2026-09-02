//! Project health: how much of the judged graph is implicated by findings, as a
//! ratio of two counted integers — never a weighted score. The universe is the
//! judged one ([`Universe`]): every declaration plus every claimed file, plus
//! every dependency declaration the usage judgment counted. A finding counts
//! when it is first-party (extension findings are advisory by the two-tier
//! decision), warning or worse (`Info` is the advisory severity tier), and lands
//! on a subject inside that universe. Subjects are counted DISTINCT: a function
//! that is both unused and duplicated is one problem unit, not two penalties.
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
use kndo_contract::vocab::{Category, ProjectPath};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// What health divides by. Files and declarations are the graph's own;
/// dependency declarations join only where the usage judgment counted them, so
/// a dependency-subject finding implicates exactly when its subject was judged
/// — never a finding on something outside the ratio's denominator.
#[derive(Debug, Clone, Default)]
pub struct Universe {
    /// Every claimed file plus every declaration.
    pub graph_subjects: u32,
    /// The dependency declarations the usage judgment counted, by declaring
    /// manifest.
    pub dependencies: BTreeMap<ProjectPath, BTreeSet<SmolStr>>,
}

impl Universe {
    pub fn of(
        graph: &crate::graph::Graph,
        dependencies: BTreeMap<ProjectPath, BTreeSet<SmolStr>>,
    ) -> Universe {
        Universe {
            graph_subjects: (graph.files.len()
                + graph
                    .files
                    .iter()
                    .map(|f| f.evidence.declarations.len())
                    .sum::<usize>()) as u32,
            dependencies,
        }
    }

    pub fn size(&self) -> u32 {
        self.graph_subjects
            + self
                .dependencies
                .values()
                .map(|names| names.len() as u32)
                .sum::<u32>()
    }

    fn contains(&self, subject: &Subject) -> bool {
        match subject {
            Subject::Symbol { .. } | Subject::File { .. } => true,
            Subject::Dependency {
                owner_manifest,
                name,
            } => self
                .dependencies
                .get(owner_manifest)
                .is_some_and(|names| names.contains(name)),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Health {
    /// Distinct universe subjects carrying at least one counting finding.
    pub implicated: u32,
    /// The judged universe: every declaration plus every claimed file.
    pub subjects: u32,
    /// Counting findings per category, in category order. More entries than
    /// `implicated` when one subject carries findings from several categories.
    pub by_category: Vec<CategoryCount>,
    /// The same two integers partitioned by owning package (name-sorted; the
    /// empty name is the unpackaged remainder). Filled by
    /// [`Health::partition`] where a graph is in hand; absent on
    /// graph-free measurements.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_package: Vec<PackageHealth>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PackageHealth {
    /// The package's declared name; empty for subjects outside every package.
    pub name: smol_str::SmolStr,
    /// The anchoring manifest — what tells two same-named packages apart
    /// (parallel trees legitimately duplicate names); empty for the
    /// unpackaged bucket.
    pub manifest: smol_str::SmolStr,
    pub implicated: u32,
    pub subjects: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        universe: &Universe,
        judged: &BTreeSet<Category>,
    ) -> Option<Health> {
        let subjects = universe.size();
        if subjects == 0 || !judged.contains(&Category::UNUSED) {
            return None;
        }
        let mut implicated: HashSet<&Subject> = HashSet::new();
        let mut by_category: BTreeMap<&Category, u32> = BTreeMap::new();
        for finding in findings.iter().filter(|f| counts(f, universe)) {
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
            by_package: Vec::new(),
        })
    }

    /// Partition the measurement by owning package — same universe, same
    /// counting rule, split by [`crate::graph::Graph::package_of`] on each
    /// subject's path (a dependency's path is its manifest). The whole always
    /// reconciles: every bucket's subjects sum to the top-level count.
    pub fn partition(
        &mut self,
        graph: &crate::graph::Graph,
        findings: &[Finding],
        universe: &Universe,
    ) {
        // Buckets by package INDEX — two same-named packages (parallel trees)
        // stay two rows, told apart by their manifests.
        let mut subjects: BTreeMap<Option<u32>, u32> = BTreeMap::new();
        for f in &graph.files {
            *subjects
                .entry(graph.package_of(f.path.as_str()))
                .or_insert(0) += 1 + f.evidence.declarations.len() as u32;
        }
        for (manifest, names) in &universe.dependencies {
            *subjects
                .entry(graph.package_of(manifest.as_str()))
                .or_insert(0) += names.len() as u32;
        }
        let mut implicated: BTreeMap<Option<u32>, HashSet<&Subject>> = BTreeMap::new();
        for finding in findings.iter().filter(|f| counts(f, universe)) {
            implicated
                .entry(graph.package_of(finding.subject.path().as_str()))
                .or_default()
                .insert(&finding.subject);
        }
        let mut rows: Vec<PackageHealth> = subjects
            .into_iter()
            .map(|(bucket, subject_count)| {
                let (name, manifest) = match bucket {
                    Some(i) => {
                        let p = &graph.packages[i as usize];
                        (p.name.clone(), SmolStr::new(p.manifest.as_str()))
                    }
                    None => (SmolStr::default(), SmolStr::default()),
                };
                PackageHealth {
                    implicated: implicated.get(&bucket).map_or(0, |s| s.len() as u32),
                    name,
                    manifest,
                    subjects: subject_count,
                }
            })
            .collect();
        rows.sort_by(|a, b| (&a.name, &a.manifest).cmp(&(&b.name, &b.manifest)));
        self.by_package = rows;
    }

    /// The score as every frontend prints it — `100 × (1 − implicated/subjects)`
    /// to one decimal, in the one place, so no render keeps its own arithmetic.
    pub fn score_text(&self) -> String {
        let clean = 1.0 - f64::from(self.implicated) / f64::from(self.subjects);
        format!("{:.1}", 100.0 * clean)
    }
}

fn counts(finding: &Finding, universe: &Universe) -> bool {
    !finding.category.is_extension()
        && finding.severity.at_least(Severity::Warning)
        && universe.contains(&finding.subject)
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

    fn graph_free(subjects: u32) -> Universe {
        Universe {
            graph_subjects: subjects,
            ..Default::default()
        }
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
        let h = Health::measure(&findings, &graph_free(10), &judged_with_unused()).unwrap();
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
        let h = Health::measure(&findings, &graph_free(4), &judged_with_unused()).unwrap();
        assert_eq!(h.implicated, 0);
        assert!(h.by_category.is_empty());
        assert_eq!(h.score_text(), "100.0");
    }

    #[test]
    fn no_reachability_judgment_means_no_health() {
        assert!(Health::measure(&[], &graph_free(5), &BTreeSet::new()).is_none());
        assert!(Health::measure(&[], &graph_free(0), &judged_with_unused()).is_none());
    }
}
