//! Duplicated code, two granularities: whole files with identical content (byte
//! clones), and functions whose winnowed fingerprint sets are equal (Type-1/2
//! structural clones — renames and re-valued literals included, because adapters
//! normalize those leaves before fingerprinting). The first member in path order is
//! the canonical copy; every other member is the finding.

use super::{Analysis, AnalysisContext};
use kndo_contract::evidence::EvidenceStream;
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::{Subject, SymbolSelector};
use kndo_contract::vocab::{Category, Confidence};
use std::collections::BTreeMap;

/// Functions below this many normalized tokens fingerprint too easily to accuse.
/// Corpus-measured (2026-08-30, DECISIONS): 60 vs 100 added 16 findings on vite —
/// every sampled one a true template clone — and covers the harvested fixture's
/// designed clones (76 tokens). Becomes a config key when the registry lands; until
/// then, one owner here.
const MIN_TOKENS: u32 = 60;

pub struct Duplicate;

impl Analysis for Duplicate {
    fn id(&self) -> &'static str {
        "duplicate"
    }

    fn category(&self) -> Category {
        Category::DUPLICATE
    }

    fn requires(&self) -> &'static [EvidenceStream] {
        &[EvidenceStream::Metrics]
    }

    fn run(&self, cx: &AnalysisContext<'_>) -> Vec<Finding> {
        let g = cx.graph();
        let mut out = Vec::new();

        // Byte-identical files. Members beyond the first (path order) are findings;
        // their contents are then excluded from the structural pass — every function
        // in them would duplicate trivially.
        let mut by_hash: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (i, f) in g.files.iter().enumerate() {
            if cx.measured[i] {
                by_hash.entry(f.hash_hex.as_str()).or_default().push(i);
            }
        }
        let mut shadowed = vec![false; g.files.len()];
        for group in by_hash.values() {
            let [first, rest @ ..] = group.as_slice() else {
                continue;
            };
            for &i in rest {
                shadowed[i] = true;
                out.push(Finding::new(
                    Category::DUPLICATE,
                    Severity::Info,
                    Confidence::Certain,
                    Subject::File {
                        path: g.files[i].path.clone(),
                    },
                    "",
                    format!("byte-identical to {}", g.files[*first].path.as_str()),
                ));
            }
        }

        // Structural function clones: equal fingerprint sets, above the token floor.
        let mut groups: BTreeMap<&[u64], Vec<(usize, usize)>> = BTreeMap::new();
        for (i, f) in g.files.iter().enumerate() {
            if !cx.measured[i] || shadowed[i] {
                continue;
            }
            for (decl_id, m) in &f.evidence.metrics {
                if m.token_count < MIN_TOKENS || m.fingerprints.is_empty() {
                    continue;
                }
                groups
                    .entry(m.fingerprints.as_slice())
                    .or_default()
                    .push((i, decl_id.index()));
            }
        }
        for members in groups.values() {
            let [first, rest @ ..] = members.as_slice() else {
                continue;
            };
            let canonical = render(g, *first);
            for &(file, decl) in rest {
                let f = &g.files[file];
                let d = &f.evidence.declarations[decl];
                let selector = match d.owner {
                    Some(owner) => SymbolSelector::Member {
                        owner: f.evidence.declarations[owner.index()].name.clone(),
                        name: d.name.clone(),
                    },
                    None => SymbolSelector::Free(d.name.clone()),
                };
                out.push(Finding::new(
                    Category::DUPLICATE,
                    Severity::Info,
                    Confidence::Certain,
                    Subject::Symbol {
                        path: f.path.clone(),
                        selector,
                        span: d.span,
                    },
                    "",
                    format!("structural clone of {canonical}"),
                ));
            }
        }
        out
    }
}

fn render(g: &crate::graph::Graph, (file, decl): (usize, usize)) -> String {
    let f = &g.files[file];
    let d = &f.evidence.declarations[decl];
    let name = match d.owner {
        Some(owner) => format!("{}.{}", f.evidence.declarations[owner.index()].name, d.name),
        None => d.name.to_string(),
    };
    format!("{}:{}", f.path.as_str(), name)
}
