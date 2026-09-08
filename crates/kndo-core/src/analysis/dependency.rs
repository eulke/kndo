//! The shared floor under every dependency-subject verdict: which manifests the
//! run judges at all, which declarations count, and who imports them. `unused`
//! and `test-only` each read this once and add their own rule — eligibility is
//! decided here so the two can never disagree about it, and health's dependency
//! universe is exactly what was judged.
//!
//! A manifest is judged when its adapter derives package identity from
//! specifiers, no unclaimed file its adapter says could import sits inside its
//! package (outside the paths the adapter never compiles), at least one owned
//! file is reached, and not every owned file is a test. Each failed condition
//! is a named abstention, never a silent skip.

use super::{AbstentionReason, AbstentionScope, AnalysisContext, Reachability, RunContext};
use crate::graph::{Graph, ManifestDeclarations};
use kndo_contract::adapter::DependencyDeclaration;
use kndo_contract::vocab::ProjectPath;
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};

/// Why each manifest's dependency usage goes unjudged, parallel to
/// `graph.manifest_declarations`; `None` where the run judges it.
pub(super) fn eligibility(graph: &Graph, reach: &Reachability) -> Vec<Option<AbstentionReason>> {
    // Unclaimed files with a suffix, each with the package owning it — the
    // same ownership `owned` follows, so a monorepo's root never inherits the
    // doubt a member's `.vue` file casts on the member. Which suffixes cast
    // doubt is each manifest's adapter's declaration.
    let claimed: BTreeSet<&str> = graph.files.iter().map(|f| f.path.as_str()).collect();
    let manifests: BTreeSet<&str> = graph.manifests.iter().map(ProjectPath::as_str).collect();
    let unclaimed: Vec<(&str, &str, Option<u32>)> = graph
        .discovered
        .iter()
        .map(ProjectPath::as_str)
        .filter(|p| !claimed.contains(p) && !manifests.contains(p))
        .filter_map(|p| {
            let name = p.rsplit('/').next().unwrap_or(p);
            let (_, suffix) = name.rsplit_once('.')?;
            Some((p, suffix, graph.package_of(p)))
        })
        .collect();
    graph
        .manifest_declarations
        .iter()
        .map(|md| {
            if md.identity == kndo_contract::plugin::DependencyIdentity::Underivable {
                return Some(AbstentionReason::SpecifierIdentityUnderivable);
            }
            // What the adapter never compiles imports nothing on its behalf:
            // a dependency's `.vue` under `node_modules` is unread by design.
            let ignored = crate::extract::ignore_set(&md.ignores);
            let suffixes: BTreeSet<SmolStr> = unclaimed
                .iter()
                .filter(|(p, suffix, owner)| {
                    md.importers.iter().any(|s| s.eq_ignore_ascii_case(suffix))
                        && md.owns(p, *owner)
                        && !ignored.is_match(p)
                })
                .map(|(_, suffix, _)| SmolStr::new(suffix.to_ascii_lowercase()))
                .collect();
            if !suffixes.is_empty() {
                return Some(AbstentionReason::UnclaimedImporters {
                    suffixes: suffixes.into_iter().collect(),
                });
            }
            if !md.owned.iter().any(|&i| reach.any(i as usize)) {
                return Some(AbstentionReason::NothingReachesOwnedFiles);
            }
            if md
                .owned
                .iter()
                .all(|&i| super::is_test_file(graph, i as usize))
            {
                return Some(AbstentionReason::OwnedFilesAreTests);
            }
            None
        })
        .collect()
}

/// One declaration the run judges as a usage claim, with who imports it.
pub(super) struct JudgedDependency<'a> {
    pub manifest: &'a ManifestDeclarations,
    pub declaration: &'a DependencyDeclaration,
    /// The importing files, the package's own and the rest of the tree alike:
    /// a cross-package import keeps a declaration in use as surely as an
    /// in-package one — the hoisting a monorepo relies on.
    pub users: &'a [u32],
}

/// Every declaration judged this run: an eligible manifest, a judged scope, and
/// the manifest not naming it itself (a `scripts` binary, an alias — used
/// without any import; `ManifestDeclarations::mentions`).
pub(super) fn judged<'a>(cx: &AnalysisContext<'a>) -> impl Iterator<Item = JudgedDependency<'a>> {
    let run = cx.run;
    run.graph
        .manifest_declarations
        .iter()
        .zip(&run.manifests)
        .filter(|(_, doubt)| doubt.is_none())
        .flat_map(|(md, _)| {
            md.declarations
                .iter()
                .enumerate()
                .filter(move |(i, dd)| {
                    md.judged[*i]
                        && md
                            .mentions
                            .binary_search_by(|m| m.as_str().cmp(dd.name.as_str()))
                            .is_err()
                })
                .map(move |(i, dd)| JudgedDependency {
                    manifest: md,
                    declaration: dd,
                    users: &md.users[i],
                })
        })
}

/// The judged declarations by manifest — what health divides by. Counts every
/// judged declaration, the manifest-named ones included: those were judged and
/// found in use.
pub(super) fn universe(run: &RunContext<'_>) -> BTreeMap<ProjectPath, BTreeSet<SmolStr>> {
    let mut out: BTreeMap<ProjectPath, BTreeSet<SmolStr>> = BTreeMap::new();
    for (md, doubt) in run.graph.manifest_declarations.iter().zip(&run.manifests) {
        if doubt.is_some() {
            continue;
        }
        for (dd, judged) in md.declarations.iter().zip(&md.judged) {
            if *judged {
                out.entry(md.manifest.clone())
                    .or_default()
                    .insert(dd.name.clone());
            }
        }
    }
    out
}

/// Record this run's unjudged manifests under the calling analysis's category:
/// one abstention per reason, in the order the (path-sorted) manifests first
/// raised it, with the unclaimed suffixes unioned.
pub(super) fn abstain(cx: &AnalysisContext<'_>) {
    let mut groups: Vec<(AbstentionReason, u32)> = Vec::new();
    for reason in cx.run.manifests.iter().flatten() {
        let slot = groups
            .iter_mut()
            .find(|(g, _)| std::mem::discriminant(g) == std::mem::discriminant(reason));
        match (slot, reason) {
            (
                Some((AbstentionReason::UnclaimedImporters { suffixes }, n)),
                AbstentionReason::UnclaimedImporters { suffixes: more },
            ) => {
                let mut union: BTreeSet<SmolStr> = suffixes.drain(..).collect();
                union.extend(more.iter().cloned());
                suffixes.extend(union);
                *n += 1;
            }
            (Some((_, n)), _) => *n += 1,
            (None, reason) => groups.push((reason.clone(), 1)),
        }
    }
    for (reason, unjudged) in groups {
        cx.abstain(reason, AbstentionScope::Manifests { unjudged });
    }
}

/// The manifests a file answers to, nearest first: every manifest whose
/// directory prefixes the path. The nearest is the one that should declare
/// what the file imports; the rest are the ancestors hoisting may resolve
/// through.
pub(super) fn manifest_chain(graph: &Graph, path: &str) -> Vec<usize> {
    let mut chain: Vec<(usize, usize)> = graph
        .manifest_declarations
        .iter()
        .enumerate()
        .filter_map(|(ix, md)| {
            let dir = md.manifest.as_str().rsplit_once('/').map_or("", |(d, _)| d);
            let prefixes =
                dir.is_empty() || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'));
            prefixes.then_some((dir.len(), ix))
        })
        .collect();
    chain.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    chain.into_iter().map(|(_, ix)| ix).collect()
}
