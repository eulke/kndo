//! Analyses: pure functions over the [`crate::graph::ProjectGraph`] (RFC 0005). Each analysis
//! consumes the graph plus whatever shared engines it needs (reachability, dup-detection, …)
//! and produces [`crate::engine::Finding`]s — never source text, never I/O.

pub mod dependency_hygiene;
pub mod duplicate;
pub mod internal_only;
pub mod private_type_leak;
pub mod reachability;
mod rollup;
pub mod test_only;
pub mod undeclared;
pub mod untested;
pub mod unused;
pub mod version_skew;

use crate::adapter::Diagnostic;
use crate::engine::Finding;
use crate::graph::{PackageNode, ProjectGraph};
use crate::vocab::PackageId;

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

/// The stable, empty-for-the-implicit-package identity used in finding ids — deliberately
/// *not* the human-readable label (which can be absent or a display name), so ids stay stable
/// across packages that share a name but not a manifest path. Shared by every per-package
/// dependency analysis (`undeclared`, `dependency_hygiene`).
pub(crate) fn package_discriminator(graph: &ProjectGraph, package: PackageId) -> String {
    match graph.packages[package.0 as usize].manifest.as_ref() {
        Some(path) => path.0.to_string(),
        None => String::new(),
    }
}

pub(crate) fn package_label(graph: &ProjectGraph, package: PackageId) -> String {
    match &graph.packages[package.0 as usize] {
        PackageNode {
            name: Some(name), ..
        } => name.to_string(),
        PackageNode {
            manifest: Some(path),
            ..
        } => path.0.to_string(),
        PackageNode { .. } => "the project (no manifest)".to_string(),
    }
}

/// Runs every M1 analysis and returns their findings, sorted by id for deterministic output.
pub fn run_all(graph: &crate::graph::ProjectGraph) -> (Vec<Finding>, Vec<Diagnostic>) {
    let reach = reachability::compute(graph);
    let mut findings = unused::find_unused_files(graph, &reach);
    findings.extend(unused::find_unused_symbols(graph, &reach));
    findings.extend(test_only::find_test_only_files(graph, &reach));
    findings.extend(test_only::find_test_only_symbols(graph, &reach));
    findings.extend(undeclared::find_undeclared_dependencies(graph));
    findings.extend(version_skew::find_version_skew(graph));
    findings.extend(duplicate::find_duplicate_files(graph));
    findings.extend(dependency_hygiene::find_dependency_hygiene(graph));
    findings.extend(internal_only::find_internal_only(graph, &reach));
    findings.extend(private_type_leak::find_private_type_leaks(graph));
    let (untested_findings, untested_diagnostic) = untested::find_untested(graph, &reach);
    findings.extend(untested_findings);
    findings.sort_by(|a, b| a.id.cmp(&b.id));
    (findings, untested_diagnostic.into_iter().collect())
}
