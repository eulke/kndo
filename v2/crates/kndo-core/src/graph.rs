//! Assembly: claimed files + their evidence become the graph. Ids are indices into
//! the path-sorted file list (two-phase: order fixed before any resolution runs), and
//! every edge list is sorted, so the graph is a pure function of the tree —
//! serialized, it is byte-identical across thread counts and cache states, which is
//! exactly what the equivalence gates compare.

use crate::discover::DiscoveredFile;
use crate::extract::ClaimedFile;
use kndo_contract::adapter::{LanguageAdapter, Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::{FileEvidence, ImportTarget, Root, RootTarget};
use kndo_contract::vocab::ProjectPath;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::collections::BTreeSet;

#[derive(Serialize, Deserialize)]
pub struct GraphFile {
    pub path: ProjectPath,
    pub adapter: SmolStr,
    pub hash_hex: String,
    pub evidence: FileEvidence,
    /// Whole-file roots anchored from OUTSIDE this file's content (a manifest naming
    /// it as an entry point). Kept apart from `evidence.roots` because evidence is
    /// cached by this file's content hash — a manifest change must not invalidate it.
    pub anchored: Vec<Root>,
    /// Resolved import targets, as indices into `Graph::files`; sorted, deduplicated.
    pub imports: Vec<u32>,
    /// Parallel to `evidence.imports`: where each import resolved, so bindings apply
    /// to THEIR target only. `None` is keep-alive — an external package or an
    /// unresolvable specifier, never an accusation.
    pub import_targets: Vec<Option<u32>>,
    pub unresolved_imports: u32,
}

impl GraphFile {
    /// The one spelling of "something anchors this file" — extraction evidence and
    /// manifest anchors alike.
    pub fn is_rooted(&self) -> bool {
        !self.evidence.roots.is_empty() || !self.anchored.is_empty()
    }
}

#[derive(Serialize, Deserialize)]
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

    // The package pass first: what each manifest declares becomes queryable by every
    // adapter's `resolve`. Manifests are consulted in path order; the first manifest
    // to declare a name keeps it.
    let files_cx = ResolveContext::new(&known);
    let mut packages: std::collections::BTreeMap<SmolStr, kndo_contract::adapter::PackageEntry> =
        std::collections::BTreeMap::new();
    for_each_manifest(files, adapters, |adapter, manifest| {
        for pkg in adapter.packages(&manifest, &files_cx) {
            packages.entry(pkg.name.clone()).or_insert(pkg);
        }
    });
    let cx = ResolveContext::with_packages(&known, &packages);

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
                anchored: Vec::new(),
                imports: Vec::new(),
                import_targets: Vec::new(),
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
    let mut resolved: Vec<(Vec<u32>, Vec<Option<u32>>, u32)> =
        Vec::with_capacity(graph_files.len());
    for gf in &graph_files {
        let adapter = adapters
            .iter()
            .find(|a| a.spec().id() == gf.adapter.as_str())
            .expect("claiming adapter is registered");
        let mut targets = BTreeSet::new();
        let mut per_import = Vec::with_capacity(gf.evidence.imports.len());
        let mut unresolved = 0u32;
        for import in &gf.evidence.imports {
            let (specifier, relative) = match &import.target {
                ImportTarget::Relative(s) => (s, true),
                ImportTarget::Package(s) => (s, false),
                // An unknown target kind keeps its import alive, unresolved-silently.
                _ => {
                    per_import.push(None);
                    continue;
                }
            };
            match adapter.resolve(&gf.path, specifier, &cx) {
                Resolution::File(p) => match index_of(&p, &graph_files) {
                    Some(ix) => {
                        targets.insert(ix);
                        per_import.push(Some(ix));
                    }
                    None => {
                        per_import.push(None);
                        unresolved += 1;
                    }
                },
                _ => {
                    per_import.push(None);
                    // A relative specifier that resolves nowhere is a broken edge
                    // worth counting; an unmatched bare specifier is an external
                    // package, which is normal.
                    if relative {
                        unresolved += 1;
                    }
                }
            }
        }
        resolved.push((targets.into_iter().collect(), per_import, unresolved));
    }
    for (gf, (imports, per_import, unresolved)) in graph_files.iter_mut().zip(resolved) {
        gf.imports = imports;
        gf.import_targets = per_import;
        gf.unresolved_imports = unresolved;
    }

    anchor_manifest_roots(files, adapters, &cx, &mut graph_files);

    Graph { files: graph_files }
}

/// Every (adapter, discovered manifest) pair, in file-path order — the one iteration
/// both manifest passes share.
fn for_each_manifest(
    files: &[DiscoveredFile],
    adapters: &[Box<dyn LanguageAdapter>],
    mut f: impl FnMut(&dyn LanguageAdapter, SourceFile<'_>),
) {
    let manifest_sets: Vec<Option<globset::GlobSet>> = adapters
        .iter()
        .map(|a| {
            let globs = a.spec().manifests();
            if globs.is_empty() {
                return None;
            }
            let mut b = globset::GlobSetBuilder::new();
            for g in globs {
                if let Ok(glob) = globset::Glob::new(g) {
                    b.add(glob);
                }
            }
            b.build().ok()
        })
        .collect();
    for file in files {
        for (adapter, set) in adapters.iter().zip(&manifest_sets) {
            let Some(set) = set else { continue };
            if !set.is_match(file.path.as_str()) {
                continue;
            }
            f(
                adapter.as_ref(),
                SourceFile {
                    path: &file.path,
                    content: &file.content,
                },
            );
        }
    }
}

/// The manifest pass: every discovered file matching an adapter's declared manifest
/// globs is handed to that adapter's `roots`, and each returned entry that names a
/// file in the graph anchors a whole-file root there. Manifests are consulted in path
/// order and the anchors are sorted, so the result is a pure function of the tree.
fn anchor_manifest_roots(
    files: &[DiscoveredFile],
    adapters: &[Box<dyn LanguageAdapter>],
    cx: &ResolveContext<'_>,
    graph_files: &mut [GraphFile],
) {
    let mut anchors: Vec<(usize, Root)> = Vec::new();
    for_each_manifest(files, adapters, |adapter, manifest| {
        for root in adapter.roots(&manifest, cx) {
            let Ok(ix) = graph_files.binary_search_by(|x| x.path.cmp(&root.file)) else {
                continue;
            };
            anchors.push((
                ix,
                Root {
                    target: RootTarget::WholeFile,
                    kind: root.kind,
                    confidence: root.confidence,
                },
            ));
        }
    });
    for (ix, root) in anchors {
        graph_files[ix].anchored.push(root);
    }
    for gf in graph_files.iter_mut() {
        gf.anchored
            .sort_by_key(|r| (r.kind as u8, std::cmp::Reverse(r.confidence)));
        gf.anchored
            .dedup_by(|a, b| a.kind == b.kind && a.confidence == b.confidence);
    }
}
