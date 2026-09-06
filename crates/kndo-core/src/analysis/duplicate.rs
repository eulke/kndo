//! Duplicated code, two granularities: whole files with identical content (byte
//! clones), and functions whose winnowed fingerprint sets are equal (Type-1/2
//! structural clones — renames and re-valued literals included, because adapters
//! normalize those leaves before fingerprinting). The first member in path order is
//! the canonical copy; every other member is the finding.
//!
//! Each granularity has a floor below which identity is not duplication: a
//! function of too few normalized tokens fingerprints too easily, and a file
//! with too few bytes outside its comments is identical to another by having
//! nothing in it — an empty package marker, a one-line stub, a license header
//! over a `package` clause — not by having been copied. Comments are excluded
//! from the measure because a license header makes every trivial file look
//! substantial; the floor reads what could have been duplicated.

use super::{Analysis, AnalysisContext};
use kndo_contract::evidence::{DeclarationId, EvidenceStream};
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::Subject;
use kndo_contract::vocab::{Category, Confidence};
use std::collections::BTreeMap;

/// Functions below this many normalized tokens fingerprint too easily to accuse.
/// Corpus-measured (2026-08-30, DECISIONS): 60 vs 100 added 16 findings on vite —
/// every sampled one a true template clone — and covers the harvested fixture's
/// designed clones (76 tokens). Becomes a config key when the registry lands; until
/// then, one owner here.
const MIN_TOKENS: u32 = 60;

/// Files with fewer bytes than this outside their comments are not judged for
/// byte-identity. Corpus-measured (2026-09-06, DECISIONS): below it sit empty
/// `__init__.py` markers, one-line fixture stubs, `package-info.java` files
/// whose thousand bytes are Javadoc over one clause; above it, classes with
/// methods and modules with functions — and the smallest function clone the
/// token floor admits is about this size.
const MIN_FILE_BYTES: u32 = 200;

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
                // Shadowed whatever its size: a copied file must not also
                // duplicate every function inside itself.
                shadowed[i] = true;
                if bytes_outside_comments(&g.files[i].evidence) < MIN_FILE_BYTES {
                    continue;
                }
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
        let mut groups: BTreeMap<&[u64], Vec<(usize, DeclarationId)>> = BTreeMap::new();
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
                    .push((i, *decl_id));
            }
        }
        for members in groups.values() {
            let [first, rest @ ..] = members.as_slice() else {
                continue;
            };
            let canonical = render(g, *first);
            for &(file, decl) in rest {
                let f = &g.files[file];
                out.push(Finding::new(
                    Category::DUPLICATE,
                    Severity::Info,
                    Confidence::Certain,
                    f.evidence.subject_of(&f.path, decl),
                    "",
                    format!("structural clone of {canonical}"),
                ));
            }
        }
        out
    }
}

/// What a file holds beyond its comments — the size a byte-identity judgment
/// measures against. Every built-in declares the comments stream; a file whose
/// adapter does not is measured whole, the keep-judging direction.
fn bytes_outside_comments(evidence: &kndo_contract::evidence::FileEvidence) -> u32 {
    let commented: u32 = evidence
        .comments
        .iter()
        .map(|c| c.span.end.saturating_sub(c.span.start))
        .sum();
    evidence.len.saturating_sub(commented)
}

fn render(g: &crate::graph::Graph, (file, decl): (usize, DeclarationId)) -> String {
    let f = &g.files[file];
    format!(
        "{}:{}",
        f.path.as_str(),
        f.evidence.selector_of(decl).render()
    )
}
