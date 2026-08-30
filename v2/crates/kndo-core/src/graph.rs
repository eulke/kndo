//! Assembly: claimed files + their evidence become the graph. Ids are indices into
//! the path-sorted file list (two-phase: order fixed before any resolution runs), and
//! every edge list is sorted, so the graph is a pure function of the tree —
//! serialized, it is byte-identical across thread counts and cache states, which is
//! exactly what the equivalence gates compare.

use crate::discover::DiscoveredFile;
use crate::extract::ClaimedFile;
use kndo_contract::adapter::{LanguageAdapter, Resolution, ResolveContext};
use kndo_contract::evidence::{FileEvidence, ImportTarget};
use kndo_contract::vocab::ProjectPath;
use serde::Serialize;
use smol_str::SmolStr;
use std::collections::BTreeSet;

#[derive(Serialize)]
pub struct GraphFile {
    pub path: ProjectPath,
    pub adapter: SmolStr,
    pub hash_hex: String,
    pub evidence: FileEvidence,
    /// Resolved import targets, as indices into `Graph::files`; sorted, deduplicated.
    pub imports: Vec<u32>,
    pub unresolved_imports: u32,
}

#[derive(Serialize)]
pub struct Graph {
    pub files: Vec<GraphFile>,
}

impl Graph {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("graph serializes")
    }
}

pub fn assemble(
    files: &[DiscoveredFile],
    claims: &[ClaimedFile],
    evidence: Vec<FileEvidence>,
    adapters: &[Box<dyn LanguageAdapter>],
) -> Graph {
    let known: BTreeSet<ProjectPath> = claims
        .iter()
        .map(|c| files[c.file_index].path.clone())
        .collect();
    let cx = ResolveContext::new(&known);

    let mut graph_files: Vec<GraphFile> = claims
        .iter()
        .zip(evidence)
        .map(|(c, ev)| {
            let f = &files[c.file_index];
            GraphFile {
                path: f.path.clone(),
                adapter: SmolStr::new(adapters[c.adapter_index].spec().id()),
                hash_hex: f.hash.iter().map(|b| format!("{b:02x}")).collect(),
                evidence: ev,
                imports: Vec::new(),
                unresolved_imports: 0,
            }
        })
        .collect();
    graph_files.sort_by(|a, b| a.path.cmp(&b.path));

    let index_of = |path: &ProjectPath, gf: &[GraphFile]| -> Option<u32> {
        gf.binary_search_by(|x| x.path.cmp(path))
            .ok()
            .map(|i| i as u32)
    };

    // Resolution as a second phase over fixed ids.
    let mut resolved: Vec<(Vec<u32>, u32)> = Vec::with_capacity(graph_files.len());
    for gf in &graph_files {
        let adapter = adapters
            .iter()
            .find(|a| a.spec().id() == gf.adapter.as_str())
            .expect("claiming adapter is registered");
        let mut targets = BTreeSet::new();
        let mut unresolved = 0u32;
        for import in &gf.evidence.imports {
            let specifier = match &import.target {
                ImportTarget::Relative(s) => s,
                // Package targets wait for manifest capabilities (M2+): keep-alive,
                // not an unresolved accusation.
                _ => continue,
            };
            match adapter.resolve(&gf.path, specifier, &cx) {
                Resolution::File(p) => {
                    if let Some(ix) = index_of(&p, &graph_files) {
                        targets.insert(ix);
                    } else {
                        unresolved += 1;
                    }
                }
                _ => unresolved += 1,
            }
        }
        resolved.push((targets.into_iter().collect(), unresolved));
    }
    for (gf, (imports, unresolved)) in graph_files.iter_mut().zip(resolved) {
        gf.imports = imports;
        gf.unresolved_imports = unresolved;
    }

    Graph { files: graph_files }
}
