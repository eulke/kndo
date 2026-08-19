//! Analyses: pure functions over the [`crate::graph::ProjectGraph`] (RFC 0005). Each analysis
//! consumes the graph plus whatever shared engines it needs (reachability, dup-detection, …)
//! and produces [`crate::engine::Finding`]s — never source text, never I/O.

pub mod reachability;
pub mod undeclared;
pub mod unused;
pub mod version_skew;

use crate::engine::Finding;

/// The stable finding id (contracts/output-schema.md §5): `"kndo-" + blake3(category,
/// subject_kind, path, symbol path, discriminator)[..12 hex]`. Line/column never participate,
/// so reformatting never changes an id; a rename or move does, because it changes `path`/
/// `symbol_path`.
pub fn finding_id(
    category: &str,
    subject_kind: &str,
    path: &str,
    symbol_path: &str,
    discriminator: &str,
) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in [category, subject_kind, path, symbol_path, discriminator] {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    let digest = hasher.finalize();
    format!("kndo-{}", &digest.to_hex()[..12])
}

/// Runs every M1 analysis and returns their findings, sorted by id for deterministic output.
pub fn run_all(graph: &crate::graph::ProjectGraph) -> Vec<Finding> {
    let reach = reachability::compute(graph);
    let mut findings = unused::find_unused_files(graph, &reach);
    findings.extend(undeclared::find_undeclared_dependencies(graph));
    findings.extend(version_skew::find_version_skew(graph));
    findings.sort_by(|a, b| a.id.cmp(&b.id));
    findings
}
