//! `undeclared` — a package a reached file imports that no manifest from its own
//! up to the root declares: the build works through hoisting or a transitive
//! dependency, and breaks on a clean install elsewhere.
//!
//! The floor [`super::dependency`] shares with `unused`/`test-only` decides which
//! manifests judge at all; on it, only the author's own unconditional statement
//! accuses (`Certain` — a nested `require` runs conditionally), and everything
//! the project itself provides exempts: a self-reference resolved in the tree, a
//! platform module ([`DependencyBuiltins`]), a name the file declares, a
//! declaration in the chain (`@types/` included), a mention in any manifest or
//! any literal in the tree (an alias table, a virtual module). `Probable`, not
//! `Certain`: a runtime alias table the analysis never reads is the known blind
//! spot. Never a health subject — the universe counts declarations, and an
//! undeclared package is precisely not one.

use super::{Analysis, AnalysisContext, dependency};
use kndo_contract::evidence::{ImportShape, ImportTarget};
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::plugin::DependencyBuiltins;
use kndo_contract::subject::Subject;
use kndo_contract::vocab::{Category, Confidence};
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};

pub struct Undeclared;

impl Analysis for Undeclared {
    fn id(&self) -> &'static str {
        "undeclared"
    }

    fn category(&self) -> Category {
        Category::UNDECLARED
    }

    fn run(&self, cx: &AnalysisContext<'_>) -> Vec<Finding> {
        let g = cx.graph();
        let run = cx.run;
        dependency::abstain(cx);
        // Specifiers the code spells in literals, per adapter: what the project
        // resolves itself — an alias table, a virtual module.
        let mut mentioned: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for f in &g.files {
            for im in &f.evidence.imports {
                if let (ImportTarget::Package(s), ImportShape::Mention) = (&im.target, &im.shape) {
                    mentioned
                        .entry(f.adapter.as_str())
                        .or_default()
                        .insert(s.as_str());
                }
            }
        }
        let mut accused: BTreeMap<(usize, SmolStr), u32> = BTreeMap::new();
        for (i, f) in g.files.iter().enumerate() {
            if !cx.measured[i] || !run.reach.any(i) {
                continue;
            }
            let chain = dependency::manifest_chain(g, f.path.as_str());
            let Some(&nearest) = chain.first() else {
                continue;
            };
            if run.manifests[nearest].is_some() {
                continue;
            }
            let md = &g.manifest_declarations[nearest];
            let identity = md.identity;
            let owner = g.package_of(f.path.as_str());
            for (k, im) in f.evidence.imports.iter().enumerate() {
                let ImportTarget::Package(spec) = &im.target else {
                    continue;
                };
                if matches!(im.shape, ImportShape::Mention) || im.confidence != Confidence::Certain
                {
                    continue;
                }
                let spec = spec.as_str();
                // A self-reference resolves within the importer's own package; a
                // sibling package it resolves to must still be declared.
                if f.import_targets.get(k).is_some_and(|targets| {
                    !targets.is_empty()
                        && targets
                            .iter()
                            .all(|&t| g.package_of(g.files[t as usize].path.as_str()) == owner)
                }) {
                    continue;
                }
                if md.builtins != DependencyBuiltins::None && md.builtins.covers(spec, identity) {
                    continue;
                }
                let Some(package) = identity.package_of(spec) else {
                    continue;
                };
                if f.evidence
                    .declarations
                    .iter()
                    .any(|d| d.name.as_str() == package)
                {
                    continue;
                }
                let declared = chain.iter().any(|&ix| {
                    let m = &g.manifest_declarations[ix];
                    m.declarations
                        .iter()
                        .any(|d| identity.names(spec, d.name.as_str()))
                        || m.mentions.iter().any(|name| identity.names(name, package))
                });
                if declared {
                    continue;
                }
                if mentioned
                    .get(f.adapter.as_str())
                    .is_some_and(|names| names.iter().any(|s| identity.names(s, package)))
                {
                    continue;
                }
                *accused.entry((nearest, SmolStr::new(package))).or_default() += 1;
            }
        }
        accused
            .into_iter()
            .map(|((ix, package), importers)| {
                Finding::new(
                    Category::UNDECLARED,
                    Severity::Warning,
                    Confidence::Probable,
                    Subject::Dependency {
                        owner_manifest: g.manifest_declarations[ix].manifest.clone(),
                        name: package,
                    },
                    "",
                    format!(
                        "imported by {importers} reached file{} but declared by no manifest from \
                         here up: it resolves only through hoisting or a transitive dependency",
                        if importers == 1 { "" } else { "s" }
                    ),
                )
            })
            .collect()
    }
}
