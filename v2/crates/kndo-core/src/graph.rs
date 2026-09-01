//! Assembly: claimed files + their evidence become the graph. Ids are indices into
//! the path-sorted file list (two-phase: order fixed before any resolution runs), and
//! every edge list is sorted, so the graph is a pure function of the tree —
//! serialized, it is byte-identical across thread counts and cache states, which is
//! exactly what the equivalence gates compare. A persisted graph is patched
//! surgically when only file contents changed; anything that moves the ground under
//! resolution (the file set, a manifest) falls back to full assembly.

use crate::cache::EvidenceCache;
use crate::discover::DiscoveredFile;
use crate::extract::ClaimedFile;
use kndo_contract::adapter::{PackageEntry, Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::{FileEvidence, ImportTarget, Root, RootTarget};
use kndo_contract::extension::Extension;
use kndo_contract::vocab::ProjectPath;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};

/// Bump when the SAME evidence assembles into a DIFFERENT graph — resolution
/// candidate changes, reachability semantics, new assembled fields. Folded into the
/// graph cache key beside the contract fingerprint and the adapter set.
pub const GRAPH_SEMANTICS_VERSION: u32 = 8;

#[derive(Serialize, Deserialize)]
pub struct GraphFile {
    pub path: ProjectPath,
    pub adapter: SmolStr,
    pub hash_hex: String,
    pub evidence: FileEvidence,
    /// Files whose names this file can see without an import — the rest of its
    /// compilation unit, per [`kndo_contract::extension::Extension::sees`];
    /// indices into `Graph::files`, sorted, deduplicated. Reachability walks these
    /// like import edges, and analyses pool references over the visibility they
    /// declare. A pure function of path and file set, so a content-only patch can
    /// trust the persisted values.
    pub sees: Vec<u32>,
    /// Per scope token this file's evidence uses: the region a `Scoped { scope }`
    /// declaration here can be seen from, per
    /// [`kndo_contract::extension::Extension::seen_from`] — indices into
    /// `Graph::files`, sorted, deduplicated, self included. Sorted by token. A
    /// token the adapter cannot bound has NO entry: its declarations are judged
    /// as Exported (keep-alive). Same stability class as `sees` — a pure
    /// function of path, token and file set.
    pub scoped_regions: Vec<(SmolStr, Vec<u32>)>,
    /// Roots anchored from OUTSIDE this file's content — a manifest naming it as an
    /// entry point (whole-file), a plugin naming it or one of its declarations. Kept
    /// apart from `evidence.roots` because evidence is cached by this file's content
    /// hash — a manifest change must not invalidate it. Plugin anchors never reach
    /// the persisted graph: an active graph-mutating plugin bypasses that cache.
    pub anchored: Vec<Root>,
    /// Resolved import targets, as indices into `Graph::files`; sorted, deduplicated.
    pub imports: Vec<u32>,
    /// Parallel to `evidence.imports`: the file(s) each import resolved to, so
    /// bindings apply to THEIR targets only. Usually one; several when the imported
    /// unit is a directory (a Go package); empty is keep-alive — an external package
    /// or an unresolvable specifier, never an accusation.
    pub import_targets: Vec<Vec<u32>>,
    pub unresolved_imports: u32,
}

impl GraphFile {
    /// The one spelling of "something anchors this file" — extraction evidence and
    /// manifest anchors alike.
    pub fn is_rooted(&self) -> bool {
        !self.evidence.roots.is_empty() || !self.anchored.is_empty()
    }
}

impl Graph {
    /// The package owning `path`: the longest package dir that prefixes it —
    /// nearest-boundary ownership, `None` outside every declared package.
    pub fn package_of(&self, path: &str) -> Option<u32> {
        let mut best: Option<(usize, u32)> = None;
        for (i, p) in self.packages.iter().enumerate() {
            let d = p.dir.as_str();
            let owns =
                d.is_empty() || path.starts_with(d) && path.as_bytes().get(d.len()) == Some(&b'/');
            if owns {
                let depth = d.len();
                if best.is_none_or(|(b, _)| depth > b) {
                    best = Some((depth, i as u32));
                }
            }
        }
        best.map(|(_, i)| i)
    }
}

#[derive(Serialize, Deserialize)]
pub struct Graph {
    pub files: Vec<GraphFile>,
    /// Every discovered file some extension claims as a manifest, sorted —
    /// manifests are never source, so they are never "unclaimed importers".
    pub manifests: Vec<ProjectPath>,
    /// Every discovered manifest's dependency declarations, path-sorted — the
    /// raw material of the manifest-to-manifest analyses (version-skew), built
    /// from the same [`for_each_manifest`] pipeline activation reads, so the
    /// two can never disagree about what a manifest declares.
    pub manifest_declarations: Vec<ManifestDeclarations>,
    /// Every discovered path, claimed or not, sorted — the tree as discovery
    /// saw it. `unresolved` reads it to tell "no such file" (a defect) from "a
    /// file outside the analyzed world" (an asset, a manifest — not missing).
    pub discovered: Vec<ProjectPath>,
    /// Every manifest-declared package, name-sorted — the aggregation unit
    /// health partitions by and package-level analyses judge. Built from the
    /// same `packages()` pipeline bare-import resolution reads.
    pub packages: Vec<GraphPackage>,
}

#[derive(Serialize, Deserialize)]
pub struct GraphPackage {
    pub name: SmolStr,
    /// `/`-separated directory the package owns; empty at the project root.
    pub dir: SmolStr,
    /// The manifest that anchors the package for findings: the declaring
    /// manifest inside the package's own directory when one exists (a gradle
    /// module's build file), else the manifest that emitted the entry (the
    /// settings file that named it).
    pub manifest: ProjectPath,
}

#[derive(Serialize, Deserialize)]
pub struct ManifestDeclarations {
    pub manifest: ProjectPath,
    /// Sorted by name, then scope order, then requirement — deterministic
    /// whatever order the manifest stated them in.
    pub declarations: Vec<kndo_contract::adapter::DependencyDeclaration>,
    /// Whether the claiming adapter can derive package identity from a
    /// specifier at all ([`Extension::imports_dependency`] answered); when it
    /// cannot, `users` is empty and no dependency here is ever judged —
    /// abstention, never accusation.
    pub judgeable: bool,
    /// The claiming adapter's coordinate — whose spelling judged `users` and
    /// whose declared [`kndo_contract::extension::DependencyScoping`] decides
    /// which scopes are usage claims.
    pub adapter: SmolStr,
    /// The files this manifest's package owns (nearest-boundary ownership;
    /// directory prefix when no package declares itself) — sorted indices into
    /// `Graph::files`. The universe every dependency here is judged against.
    pub owned: Vec<u32>,
    /// Parallel to `declarations`: whether each is a usage claim the engine
    /// judges at all — the adapter derives package identity (`judgeable`) AND
    /// the scope is one that claims use: production, or unscoped under an
    /// `Unscoped` ecosystem. Health's universe counts exactly these.
    pub judged: Vec<bool>,
    /// Parallel to `declarations`: the owned files whose package-shaped imports
    /// name each dependency — sorted, deduplicated indices into `Graph::files`.
    pub users: Vec<Vec<u32>>,
}

impl Graph {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("graph serializes")
    }

    /// For every manifest, which owned files use each declared dependency —
    /// through the claiming adapter's spelling knowledge
    /// ([`Extension::imports_dependency`]). A manifest whose adapter cannot
    /// derive package identity stays unjudgeable, with every `users` list empty.
    fn judge_dependency_usage(
        &mut self,
        files: &[DiscoveredFile],
        adapters: &[Box<dyn Extension>],
    ) {
        let mut claimant: BTreeMap<ProjectPath, SmolStr> = BTreeMap::new();
        for_each_manifest(files, adapters, |adapter, manifest| {
            claimant
                .entry(manifest.path.clone())
                .or_insert_with(|| SmolStr::new(adapter.spec().coordinate()));
        });
        let owners: Vec<Option<u32>> = self
            .files
            .iter()
            .map(|f| self.package_of(f.path.as_str()))
            .collect();
        struct Judged {
            judgeable: bool,
            adapter: SmolStr,
            owned: Vec<u32>,
            judged: Vec<bool>,
            users: Vec<Vec<u32>>,
        }
        let mut judged: Vec<Judged> = Vec::with_capacity(self.manifest_declarations.len());
        for md in &self.manifest_declarations {
            let Some(coordinate) = claimant.get(&md.manifest) else {
                judged.push(Judged {
                    judgeable: false,
                    adapter: SmolStr::default(),
                    owned: Vec::new(),
                    judged: vec![false; md.declarations.len()],
                    users: vec![Vec::new(); md.declarations.len()],
                });
                continue;
            };
            let adapter = adapter_by_id(adapters, coordinate);
            let unscoped = adapter.spec().dependency_scoping()
                == kndo_contract::extension::DependencyScoping::Unscoped;
            let package = self
                .packages
                .iter()
                .position(|p| p.manifest == md.manifest)
                .map(|i| i as u32);
            let dir = md.manifest.as_str().rsplit_once('/').map_or("", |(d, _)| d);
            let owned: Vec<usize> = (0..self.files.len())
                .filter(|&i| match package {
                    Some(ix) => owners[i] == Some(ix),
                    None => {
                        let p = self.files[i].path.as_str();
                        dir.is_empty() || p.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
                    }
                })
                .collect();
            let mut judgeable = true;
            let mut users: Vec<Vec<u32>> = Vec::with_capacity(md.declarations.len());
            'decls: for dd in &md.declarations {
                let mut using = Vec::new();
                for &i in &owned {
                    for import in &self.files[i].evidence.imports {
                        let ImportTarget::Package(spec) = &import.target else {
                            continue;
                        };
                        match adapter.imports_dependency(spec.as_str(), dd.name.as_str()) {
                            Some(true) => {
                                using.push(i as u32);
                                break;
                            }
                            Some(false) => {}
                            None => {
                                judgeable = false;
                                break 'decls;
                            }
                        }
                    }
                }
                users.push(using);
            }
            if !judgeable {
                users = vec![Vec::new(); md.declarations.len()];
            }
            let claims: Vec<bool> = md
                .declarations
                .iter()
                .map(|dd| {
                    judgeable
                        && match dd.scope {
                            Some(kndo_contract::adapter::DependencyScope::Prod) => true,
                            None => unscoped,
                            _ => false,
                        }
                })
                .collect();
            judged.push(Judged {
                judgeable,
                adapter: coordinate.clone(),
                owned: owned.iter().map(|&i| i as u32).collect(),
                judged: claims,
                users,
            });
        }
        for (md, j) in self.manifest_declarations.iter_mut().zip(judged) {
            md.judgeable = j.judgeable;
            md.adapter = j.adapter;
            md.owned = j.owned;
            md.judged = j.judged;
            md.users = j.users;
        }
    }
}

pub fn assemble(
    files: &[DiscoveredFile],
    claims: &[ClaimedFile],
    evidence: Vec<FileEvidence>,
    adapters: &[Box<dyn Extension>],
) -> Graph {
    let known: BTreeSet<ProjectPath> = claims
        .iter()
        .map(|c| files[c.file_index].path.clone())
        .collect();

    let packages = package_map(files, adapters, &known);
    let cx = ResolveContext::with_packages(&known, &packages);

    let mut graph_files: Vec<GraphFile> = claims
        .iter()
        .zip(evidence)
        .map(|(c, ev)| {
            let f = &files[c.file_index];
            GraphFile {
                path: f.path.clone(),
                adapter: SmolStr::new(adapters[c.adapter_index].spec().coordinate()),
                hash_hex: f.hash.iter().map(|b| format!("{b:02x}")).collect(),
                evidence: ev,
                sees: Vec::new(),
                scoped_regions: Vec::new(),
                anchored: Vec::new(),
                imports: Vec::new(),
                import_targets: Vec::new(),
                unresolved_imports: 0,
            }
        })
        .collect();
    graph_files.sort_by(|a, b| a.path.cmp(&b.path));

    // Resolution as a second phase over fixed ids; unit mates ride the same phase —
    // both are functions of the file set the first phase froze.
    let sorted_paths: Vec<ProjectPath> = graph_files.iter().map(|g| g.path.clone()).collect();
    let mut resolved: Vec<ResolvedEdges> = Vec::with_capacity(graph_files.len());
    for (ix, gf) in graph_files.iter().enumerate() {
        let adapter = adapter_by_id(adapters, &gf.adapter);
        let mut edges = resolve_file(&gf.path, &gf.evidence, adapter, &cx, &sorted_paths);
        edges.sees = sees_of(ix, &gf.path, adapter, &cx, &sorted_paths);
        edges.scoped_regions = regions_of(&gf.path, &gf.evidence, adapter, &cx, &sorted_paths);
        resolved.push(edges);
    }
    for (gf, edges) in graph_files.iter_mut().zip(resolved) {
        gf.imports = edges.imports;
        gf.import_targets = edges.import_targets;
        gf.unresolved_imports = edges.unresolved_imports;
        gf.sees = edges.sees;
        gf.scoped_regions = edges.scoped_regions;
        debug_assert_eq!(
            gf.import_targets.len(),
            gf.evidence.imports.len(),
            "import_targets is index-parallel to evidence.imports"
        );
    }

    anchor_manifest_roots(files, adapters, &cx, &mut graph_files);

    let manifest_declarations = collect_manifest_declarations(files, adapters);
    let mut discovered: Vec<ProjectPath> = files.iter().map(|f| f.path.clone()).collect();
    discovered.sort();
    let packages = collect_packages(files, adapters, &known, &manifest_declarations);
    let mut manifests: Vec<ProjectPath> = Vec::new();
    for_each_manifest(files, adapters, |_, manifest| {
        manifests.push(manifest.path.clone())
    });
    manifests.sort();
    manifests.dedup();
    let mut graph = Graph {
        files: graph_files,
        manifests,
        manifest_declarations,
        discovered,
        packages,
    };
    graph.judge_dependency_usage(files, adapters);
    graph
}

/// The declared packages, name-sorted and deduplicated (first declaration
/// wins, like the resolution map): each anchored to the declaring manifest in
/// its own directory when one exists, else to the manifest that emitted it.
fn collect_packages(
    files: &[DiscoveredFile],
    adapters: &[Box<dyn Extension>],
    known: &BTreeSet<ProjectPath>,
    declarations: &[ManifestDeclarations],
) -> Vec<GraphPackage> {
    let files_cx = ResolveContext::new(known);
    // Keyed by (name, dir): parallel trees legitimately duplicate a package
    // name (guava's android/ mirror), and ownership is directory truth.
    let mut out: BTreeMap<(SmolStr, SmolStr), GraphPackage> = BTreeMap::new();
    for_each_manifest(files, adapters, |adapter, manifest| {
        for pkg in adapter.packages(&manifest, &files_cx) {
            out.entry((pkg.name.clone(), pkg.dir.clone()))
                .or_insert(GraphPackage {
                    name: pkg.name,
                    dir: pkg.dir,
                    manifest: manifest.path.clone(),
                });
        }
    });
    let mut packages: Vec<GraphPackage> = out.into_values().collect();
    for p in &mut packages {
        let own_manifest = declarations
            .iter()
            .map(|d| &d.manifest)
            .find(|m| m.as_str().rsplit_once('/').map(|(d, _)| d).unwrap_or("") == p.dir.as_str());
        if let Some(m) = own_manifest {
            p.manifest = m.clone();
        }
    }
    packages
}

/// One entry per declaring manifest, path-sorted, declarations name-sorted —
/// the deterministic projection of every adapter's `manifest_dependencies`.
fn collect_manifest_declarations(
    files: &[DiscoveredFile],
    adapters: &[Box<dyn Extension>],
) -> Vec<ManifestDeclarations> {
    let mut by_manifest: std::collections::BTreeMap<
        ProjectPath,
        Vec<kndo_contract::adapter::DependencyDeclaration>,
    > = std::collections::BTreeMap::new();
    for_each_manifest(files, adapters, |adapter, manifest| {
        let declarations = adapter.manifest_dependencies(&manifest);
        if !declarations.is_empty() {
            by_manifest
                .entry(manifest.path.clone())
                .or_default()
                .extend(declarations);
        }
    });
    by_manifest
        .into_iter()
        .map(|(manifest, mut declarations)| {
            declarations.sort_by(|a, b| {
                (
                    a.name.as_str(),
                    a.scope.map(|s| s as u8),
                    a.version_req.as_deref(),
                )
                    .cmp(&(
                        b.name.as_str(),
                        b.scope.map(|s| s as u8),
                        b.version_req.as_deref(),
                    ))
            });
            declarations.dedup();
            ManifestDeclarations {
                manifest,
                declarations,
                judgeable: false,
                adapter: SmolStr::default(),
                owned: Vec::new(),
                judged: Vec::new(),
                users: Vec::new(),
            }
        })
        .collect()
}

fn adapter_by_id<'a>(adapters: &'a [Box<dyn Extension>], id: &str) -> &'a dyn Extension {
    adapters
        .iter()
        .find(|a| a.spec().coordinate() == id)
        .expect("claiming adapter is registered")
        .as_ref()
}

/// The package pass: what each manifest declares becomes queryable by every
/// adapter's `resolve`. Manifests are consulted in path order; the first manifest to
/// declare a name keeps it.
fn package_map(
    files: &[DiscoveredFile],
    adapters: &[Box<dyn Extension>],
    known: &BTreeSet<ProjectPath>,
) -> BTreeMap<SmolStr, PackageEntry> {
    let files_cx = ResolveContext::new(known);
    let mut packages: BTreeMap<SmolStr, PackageEntry> = BTreeMap::new();
    for_each_manifest(files, adapters, |adapter, manifest| {
        for pkg in adapter.packages(&manifest, &files_cx) {
            packages.entry(pkg.name.clone()).or_insert(pkg);
        }
    });
    packages
}

/// The adapter's unit mates for one file, as graph ids: sorted, deduplicated,
/// never the file itself, and only files actually in the graph — a mate the claim
/// set does not contain is silently absent, keep-alive.
fn sees_of(
    ix: usize,
    path: &ProjectPath,
    adapter: &dyn Extension,
    cx: &ResolveContext<'_>,
    sorted_paths: &[ProjectPath],
) -> Vec<u32> {
    let mut mates: Vec<u32> = adapter
        .sees(path, cx)
        .iter()
        .filter_map(|p| sorted_paths.binary_search(p).ok().map(|i| i as u32))
        .filter(|&t| t as usize != ix)
        .collect();
    mates.sort_unstable();
    mates.dedup();
    mates
}

/// The regions behind one file's scope tokens, as graph ids. Tokens come from
/// the evidence (which declarations said `Scoped`); each is answered once per
/// file. An unanswerable token is ABSENT, and judgment treats its declarations
/// as Exported.
fn regions_of(
    path: &ProjectPath,
    evidence: &kndo_contract::evidence::FileEvidence,
    adapter: &dyn Extension,
    cx: &ResolveContext<'_>,
    sorted_paths: &[ProjectPath],
) -> Vec<(SmolStr, Vec<u32>)> {
    let mut tokens: Vec<&SmolStr> = evidence
        .declarations
        .iter()
        .filter_map(|d| match &d.reach {
            kndo_contract::evidence::Reach::Scoped { scope } => Some(scope),
            _ => None,
        })
        .collect();
    tokens.sort();
    tokens.dedup();
    let mut out = Vec::new();
    for token in tokens {
        let Some(region) = adapter.seen_from(path, token, cx) else {
            continue;
        };
        let mut ids: Vec<u32> = region
            .iter()
            .filter_map(|p| sorted_paths.binary_search(p).ok().map(|i| i as u32))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        out.push((token.clone(), ids));
    }
    out
}

/// One file's assembled edges, mirroring the `GraphFile` fields they land in.
struct ResolvedEdges {
    imports: Vec<u32>,
    import_targets: Vec<Vec<u32>>,
    unresolved_imports: u32,
    sees: Vec<u32>,
    scoped_regions: Vec<(SmolStr, Vec<u32>)>,
}

fn resolve_file(
    from: &ProjectPath,
    evidence: &FileEvidence,
    adapter: &dyn Extension,
    cx: &ResolveContext<'_>,
    sorted_paths: &[ProjectPath],
) -> ResolvedEdges {
    let index_of = |p: &ProjectPath| sorted_paths.binary_search(p).ok().map(|i| i as u32);
    let mut targets = BTreeSet::new();
    let mut per_import: Vec<Vec<u32>> = Vec::with_capacity(evidence.imports.len());
    let mut unresolved = 0u32;
    for import in &evidence.imports {
        let (specifier, relative) = match &import.target {
            ImportTarget::Relative(s) => (s, true),
            ImportTarget::Package(s) => (s, false),
            // An unknown target kind keeps its import alive, unresolved-silently.
            _ => {
                per_import.push(Vec::new());
                continue;
            }
        };
        // A `Possible` package-shaped import (a specifier spelled inside a
        // string literal) is enough for a declared dependency to count as used,
        // never enough to draw a reachability edge to a workspace sibling.
        if !relative && import.confidence == kndo_contract::vocab::Confidence::Possible {
            per_import.push(Vec::new());
            continue;
        }
        let resolved: Vec<u32> = match adapter.resolve(from, specifier, cx) {
            Resolution::File(p) => index_of(&p).into_iter().collect(),
            Resolution::Files(paths) => {
                let mut ixs: Vec<u32> = paths.iter().filter_map(&index_of).collect();
                ixs.sort_unstable();
                ixs.dedup();
                ixs
            }
            _ => Vec::new(),
        };
        if resolved.is_empty() {
            // A relative specifier that resolves nowhere is a broken edge worth
            // counting; an unmatched bare specifier is an external package, which
            // is normal.
            if relative {
                unresolved += 1;
            }
        } else {
            targets.extend(resolved.iter().copied());
        }
        per_import.push(resolved);
    }
    ResolvedEdges {
        imports: targets.into_iter().collect(),
        import_targets: per_import,
        unresolved_imports: unresolved,
        sees: Vec::new(),
        scoped_regions: Vec::new(),
    }
}

/// Hash over every discovered manifest's (path, content), in path order — the
/// manifest-derived parts of a graph (anchors, the package map) are pure functions
/// of this state.
pub fn manifest_state(files: &[DiscoveredFile], adapters: &[Box<dyn Extension>]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    for_each_manifest(files, adapters, |_, manifest| {
        let path = manifest.path.as_str().as_bytes();
        h.update(&(path.len() as u32).to_le_bytes());
        h.update(path);
        h.update(&(manifest.content.len() as u32).to_le_bytes());
        h.update(manifest.content);
    });
    *h.finalize().as_bytes()
}

/// The surgical path: when the file set, every claim, and every manifest are
/// unchanged, only re-extract and re-resolve the files whose content moved — every
/// other node, edge and anchor is reused verbatim. `None` means the ground shifted
/// (set, claims, or manifests) and the caller assembles from scratch; either way the
/// result is byte-identical to full assembly, which the incremental gate compares.
pub fn patch(
    mut prev: Graph,
    prev_manifest_state: [u8; 32],
    files: &[DiscoveredFile],
    claims: &[ClaimedFile],
    adapters: &[Box<dyn Extension>],
    cache: &EvidenceCache,
) -> Option<Graph> {
    if manifest_state(files, adapters) != prev_manifest_state {
        return None;
    }
    if prev.files.len() != claims.len() {
        return None;
    }
    // Claims iterate discovery order, which is path order — the same order the
    // persisted graph is sorted in.
    let mut changed: Vec<(usize, usize)> = Vec::new();
    for (ix, c) in claims.iter().enumerate() {
        let f = &files[c.file_index];
        let gf = &prev.files[ix];
        if gf.path != f.path || gf.adapter != adapters[c.adapter_index].spec().coordinate() {
            return None;
        }
        let hex: String = f.hash.iter().map(|b| format!("{b:02x}")).collect();
        if gf.hash_hex != hex {
            changed.push((ix, c.file_index));
            prev.files[ix].hash_hex = hex;
        }
    }
    if changed.is_empty() {
        return Some(prev);
    }

    let known: BTreeSet<ProjectPath> = prev.files.iter().map(|g| g.path.clone()).collect();
    let packages = package_map(files, adapters, &known);
    let cx = ResolveContext::with_packages(&known, &packages);
    let sorted_paths: Vec<ProjectPath> = prev.files.iter().map(|g| g.path.clone()).collect();

    for (ix, file_index) in changed {
        let file = &files[file_index];
        let adapter = adapter_by_id(adapters, &prev.files[ix].adapter);
        let evidence = crate::extract::extract_one(file, adapter, cache);
        let edges = resolve_file(&file.path, &evidence, adapter, &cx, &sorted_paths);
        let gf = &mut prev.files[ix];
        gf.scoped_regions = regions_of(&file.path, &evidence, adapter, &cx, &sorted_paths);
        gf.evidence = evidence;
        gf.imports = edges.imports;
        gf.import_targets = edges.import_targets;
        gf.unresolved_imports = edges.unresolved_imports;
        // `sees` is untouched on purpose: it is a pure function of path and
        // file set, and this path only runs when both are unchanged.
        debug_assert_eq!(
            gf.import_targets.len(),
            gf.evidence.imports.len(),
            "import_targets is index-parallel to evidence.imports"
        );
    }
    Some(prev)
}

/// Every (adapter, discovered manifest) pair, in file-path order — the one iteration
/// every manifest pass shares (packages, roots, and the session's dependency-name
/// pass for plugin activation).
pub(crate) fn for_each_manifest(
    files: &[DiscoveredFile],
    adapters: &[Box<dyn Extension>],
    mut f: impl FnMut(&dyn Extension, SourceFile<'_>),
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
    adapters: &[Box<dyn Extension>],
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
