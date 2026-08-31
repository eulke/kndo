//! The same dependency declared with diverging version requirements across the
//! project's manifests — an inconsistency someone will reconcile eventually,
//! and the exact case workspace-level version pools exist to prevent.
//! Manifest-to-manifest only: no usage edge, no ownership question, just
//! [`crate::graph::Graph::manifest_declarations`].
//!
//! Two exemptions, both measured on the corpus before this shipped:
//! - **Peer requirements are contracts, not pins.** A wide `peerDependencies`
//!   range beside a narrow dev pin is CORRECT practice (the range states what
//!   consumers may bring; the pin states what CI tests against) — comparing
//!   them manufactured findings on exactly the best-maintained manifests.
//! - **A declaration with no comparable requirement says nothing.** Workspace
//!   protocols, path/git specs, BOM-managed coordinates arrive as
//!   `version_req: None` from the adapter that knows the ecosystem, and a
//!   comparison the manifest does not enable stays silent.
//!
//! Severity `info`: the divergence is a fact (`certain`), but whether it bites
//! is ecosystem-dependent — cargo unifies compatible ranges at build time, npm
//! may install duplicates — so this nudges, and deliberately never dents
//! health.

use super::{Analysis, AnalysisContext};
use kndo_contract::adapter::DependencyScope;
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::Subject;
use kndo_contract::vocab::{Category, Confidence};
use std::collections::BTreeMap;

pub struct VersionSkew;

impl Analysis for VersionSkew {
    fn id(&self) -> &'static str {
        "version-skew"
    }

    fn category(&self) -> Category {
        Category::VERSION_SKEW
    }

    fn run(&self, cx: &AnalysisContext<'_>) -> Vec<Finding> {
        let mut by_name: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
        for entry in &cx.graph().manifest_declarations {
            for d in &entry.declarations {
                if d.scope == Some(DependencyScope::Peer) {
                    continue;
                }
                let Some(req) = &d.version_req else { continue };
                by_name
                    .entry(d.name.as_str())
                    .or_default()
                    .push((entry.manifest.as_str(), req.as_str()));
            }
        }
        let mut out = Vec::new();
        for (name, mut declarations) in by_name {
            declarations.sort_unstable();
            declarations.dedup();
            let distinct: std::collections::BTreeSet<&str> =
                declarations.iter().map(|&(_, r)| r).collect();
            if distinct.len() <= 1 {
                continue;
            }
            let evidence = declarations
                .iter()
                .map(|(manifest, req)| format!("{manifest} ({req})"))
                .collect::<Vec<_>>()
                .join(", ");
            out.push(Finding::new(
                Category::VERSION_SKEW,
                Severity::Info,
                Confidence::Certain,
                Subject::Dependency {
                    // The lexicographically-first declaring manifest anchors the
                    // finding; every declaration is in the message.
                    owner_manifest: kndo_contract::vocab::ProjectPath::new(declarations[0].0),
                    name: smol_str::SmolStr::new(name),
                },
                "",
                format!("declared with diverging version requirements: {evidence}"),
            ));
        }
        out
    }
}
