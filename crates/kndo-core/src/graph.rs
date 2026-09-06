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
use kndo_contract::evidence::{FileEvidence, ImportTarget, Reach, Root, RootKind, RootTarget};
use kndo_contract::extension::{Extension, PublishedSurface};
use kndo_contract::manifest::UnitKind;
use kndo_contract::vocab::{Confidence, ProjectPath};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};

/// Bump when the SAME evidence assembles into a DIFFERENT graph — resolution
/// candidate changes, reachability semantics, new assembled fields. Folded into the
/// graph cache key beside the contract fingerprint and the adapter set.
pub const GRAPH_SEMANTICS_VERSION: u32 = 17;

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
    /// Per adapter-bounded reach this file's evidence uses — a `Scoped` token,
    /// or `Unit` until a manifest names the unit — the files a declaration of
    /// that reach can be seen from, per
    /// [`kndo_contract::extension::Extension::seen_from`]: indices into
    /// `Graph::files`, sorted, deduplicated, self included. Sorted by reach. A
    /// reach the adapter cannot bound has NO entry: its declarations are judged
    /// as Exported (keep-alive). Same stability class as `sees` — a pure
    /// function of path, reach and file set.
    pub regions: Vec<(Reach, Vec<u32>)>,
    /// This file holds an exported declaration of a published library unit, in
    /// a language whose units publish every export: its surface is the outside
    /// world's, which reaches the file as production and keeps each exported
    /// declaration ([`crate::navigate::Keeper::Published`]). A function of the
    /// manifest AND the content, so it is recomputed with the evidence.
    pub published: bool,
    /// Roots anchored from OUTSIDE this file's content — a manifest naming it as an
    /// entry point (whole-file), a plugin naming it or one of its declarations. Kept
    /// apart from `evidence.roots` because evidence is cached by this file's content
    /// hash — a manifest change must not invalidate it. Plugin anchors never reach
    /// the persisted graph: an active graph-mutating plugin bypasses that cache.
    pub anchored: Vec<Root>,
    /// Roots the engine's dispatch derived from this file's markers under the
    /// claiming extension's rules ([`crate::dispatch`]) — apart from
    /// `evidence.roots` (the adapter's own statements, cached by content) so a
    /// rule change re-dispatches without re-extracting; sorted, deduplicated.
    pub dispatched: Vec<Root>,
    /// Declarations the source itself exempts from the unused judgment (an
    /// `allow(dead_code)`-class marker, dispatched), by index; sorted.
    pub exempt: Vec<u32>,
    /// What dispatch wants the run to say about this file (a blanket
    /// exemption) — reported as diagnostics.
    pub dispatch_notes: Vec<String>,
    /// The unit compiling this file, as an index into `Graph::project`'s units
    /// — [`crate::project::Project::unit_of`]. `None` until the claiming
    /// adapter reports its manifest's units, which is what every consumer
    /// degrades toward.
    pub unit: Option<u32>,
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
    /// Every root on this file, whatever said it: the adapter's evidence, the
    /// engine's dispatch, the manifests' and plugins' anchors — the one
    /// iteration every color judgment reads.
    pub fn roots(&self) -> impl Iterator<Item = &Root> {
        self.evidence
            .roots
            .iter()
            .chain(&self.dispatched)
            .chain(&self.anchored)
    }

    /// The one spelling of "something anchors this file": a root, or the
    /// published surface it is on.
    pub fn is_rooted(&self) -> bool {
        self.roots().next().is_some() || self.published
    }
}

impl Graph {
    /// The package owning `path`: the longest package dir that prefixes it —
    /// nearest-boundary ownership, `None` outside every declared package.
    pub fn package_of(&self, path: &str) -> Option<u32> {
        self.packages
            .iter()
            .enumerate()
            .filter(|(_, p)| kndo_contract::vocab::is_under(&p.dir, path))
            // Deepest directory owns; on a tie — two packages declared for one
            // directory, a pom and the settings entry naming the same module —
            // the earlier package keeps it, so ownership is a function of the
            // name-sorted list and never of iteration luck.
            .max_by_key(|(i, p)| (p.dir.len(), std::cmp::Reverse(*i)))
            .map(|(i, _)| i as u32)
    }
}

#[derive(Serialize, Deserialize)]
pub struct Graph {
    pub files: Vec<GraphFile>,
    /// Every discovered file some extension claims as a manifest, sorted —
    /// manifests are never source, so they are never "unclaimed importers".
    pub manifests: Vec<ProjectPath>,
    /// Every discovered manifest's dependency declarations (an entry per
    /// manifest, declarations or not), path-sorted — the raw material of the
    /// dependency analyses, built from the same [`for_each_manifest`] pipeline
    /// activation reads, so the two can never disagree about what a manifest
    /// declares.
    pub manifest_declarations: Vec<ManifestDeclarations>,
    /// Every discovered path, claimed or not, sorted — the tree as discovery
    /// saw it. `unresolved` reads it to tell "no such file" (a defect) from "a
    /// file outside the analyzed world" (an asset, a manifest — not missing).
    pub discovered: Vec<ProjectPath>,
    /// Every manifest-declared package, name-sorted — the aggregation unit
    /// health partitions by and package-level analyses judge. Built from the
    /// same manifest evidence bare-import resolution reads.
    pub packages: Vec<GraphPackage>,
    /// What the project's manifests declared about its structure — the units
    /// that compile its files, and the files they are entered through.
    #[serde(default)]
    pub project: crate::project::Project,
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
    /// The claiming adapter's declared
    /// [`kndo_contract::extension::DependencyIdentity`] — the spelling that
    /// judged `users`. `Underivable` leaves every `users` list empty and no
    /// dependency here ever judged: abstention, never accusation.
    pub identity: kndo_contract::extension::DependencyIdentity,
    /// The claiming adapter's coordinate.
    pub adapter: SmolStr,
    /// The claiming adapter's declared
    /// [`kndo_contract::extension::DependencyScoping`]: under `Unscoped` an
    /// unscoped declaration is a usage claim, and "only tests import it" has
    /// no section to move to.
    pub scoping: kndo_contract::extension::DependencyScoping,
    /// The claiming adapter's declared
    /// [`kndo_contract::extension::ExtensionSpec::dependency_importers`]:
    /// suffixes of files outside its claims that carry this manifest's
    /// ecosystem's imports — read when another extension claims them, doubt on
    /// any usage judgment when nothing does.
    pub importers: Vec<SmolStr>,
    /// The claiming adapter's declared
    /// [`kndo_contract::extension::DependencyBuiltins`] — the platform's own
    /// modules, never a dependency to declare.
    pub builtins: kndo_contract::extension::DependencyBuiltins,
    /// Names the manifest spells outside its declarations, sorted and
    /// deduplicated ([`Extension::manifest_mentions`]): a dependency it names
    /// is in use without any import; a package it names is not undeclared.
    pub mentions: Vec<SmolStr>,
    /// The package this manifest declares, as an index into `Graph::packages`;
    /// `None` for a manifest that declares none, which owns by directory.
    pub package: Option<u32>,
    /// The files this manifest's package owns ([`ManifestDeclarations::owns`])
    /// — sorted indices into `Graph::files`.
    pub owned: Vec<u32>,
    /// Parallel to `declarations`: whether each is a usage claim the engine
    /// judges at all — the adapter derives package identity (`identity`) AND
    /// the scope is one that claims use: production, or unscoped under an
    /// `Unscoped` ecosystem.
    pub judged: Vec<bool>,
    /// Parallel to `declarations`: the files this adapter claims, anywhere in
    /// the tree, whose package-shaped imports name each dependency — sorted,
    /// deduplicated indices into `Graph::files`. `owned` tells the package's
    /// own users from cross-package ones.
    pub users: Vec<Vec<u32>>,
    /// The paths this manifest's adapter never compiles
    /// ([`ExtensionSpec::ignores`]): an unclaimed file under one casts no doubt
    /// on the judgment, since the adapter itself left it unread.
    ///
    /// [`ExtensionSpec::ignores`]: kndo_contract::extension::ExtensionSpec::ignores
    pub ignores: Vec<SmolStr>,
}

impl ManifestDeclarations {
    /// Does this manifest's package own `path`, whose owning package per
    /// [`Graph::package_of`] is `path_package`? Nearest-boundary ownership
    /// when the manifest declares a package; the manifest's own directory
    /// otherwise. One rule for the claimed files judged against a declaration
    /// and for the unclaimed files that cast doubt on the judgment.
    pub fn owns(&self, path: &str, path_package: Option<u32>) -> bool {
        match self.package {
            Some(ix) => path_package == Some(ix),
            None => {
                let dir = self
                    .manifest
                    .as_str()
                    .rsplit_once('/')
                    .map_or("", |(d, _)| d);
                kndo_contract::vocab::is_under(dir, path)
            }
        }
    }
}

impl Graph {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("graph serializes")
    }

    /// For every manifest, which files use each declared dependency — under
    /// the claiming adapter's declared spelling
    /// ([`kndo_contract::extension::DependencyIdentity`]), over every file that
    /// adapter claims. A manifest whose adapter derives no identity keeps every
    /// `users` list empty.
    fn judge_dependency_usage(
        &mut self,
        reads: &[crate::project::ManifestRead],
        adapters: &[Box<dyn Extension>],
    ) {
        let mut claimant: BTreeMap<ProjectPath, SmolStr> = BTreeMap::new();
        let mut mentions: BTreeMap<ProjectPath, Vec<SmolStr>> = BTreeMap::new();
        for read in reads {
            if let Some(first) = read.adapters.first() {
                claimant.insert(read.manifest.clone(), first.clone());
            }
            mentions.insert(read.manifest.clone(), read.evidence.mentions.clone());
        }
        let owners: Vec<Option<u32>> = self
            .files
            .iter()
            .map(|f| self.package_of(f.path.as_str()))
            .collect();
        // Per claiming adapter, and per file suffix, every distinct package-shaped
        // specifier and the files importing it: a declaration is matched against
        // specifiers, never against files, so the cost is declarations × distinct
        // specifiers. The suffix index is how a manifest reads imports written in
        // files another extension claims: a `.css` sheet carries npm's packages
        // (`@import "tailwindcss"`), and the js-ts adapter names that suffix among
        // its `dependency_importers`.
        let mut specifiers: BTreeMap<&str, BTreeMap<&str, Vec<u32>>> = BTreeMap::new();
        let mut by_suffix: BTreeMap<String, BTreeMap<&str, Vec<u32>>> = BTreeMap::new();
        for (i, f) in self.files.iter().enumerate() {
            let suffix = f
                .path
                .as_str()
                .rsplit('/')
                .next()
                .and_then(|name| name.rsplit_once('.'))
                .map(|(_, s)| s.to_ascii_lowercase());
            for import in &f.evidence.imports {
                if let ImportTarget::Package(spec) = &import.target {
                    let importers = specifiers
                        .entry(f.adapter.as_str())
                        .or_default()
                        .entry(spec.as_str())
                        .or_default();
                    if importers.last() != Some(&(i as u32)) {
                        importers.push(i as u32);
                    }
                    if let Some(suffix) = &suffix {
                        let importers = by_suffix
                            .entry(suffix.clone())
                            .or_default()
                            .entry(spec.as_str())
                            .or_default();
                        if importers.last() != Some(&(i as u32)) {
                            importers.push(i as u32);
                        }
                    }
                }
            }
        }
        struct Judged {
            identity: kndo_contract::extension::DependencyIdentity,
            adapter: SmolStr,
            scoping: kndo_contract::extension::DependencyScoping,
            importers: Vec<SmolStr>,
            builtins: kndo_contract::extension::DependencyBuiltins,
            mentions: Vec<SmolStr>,
            package: Option<u32>,
            owned: Vec<u32>,
            judged: Vec<bool>,
            users: Vec<Vec<u32>>,
            ignores: Vec<SmolStr>,
        }
        let mut judged: Vec<Judged> = Vec::with_capacity(self.manifest_declarations.len());
        for md in &self.manifest_declarations {
            let package = self
                .packages
                .iter()
                .position(|p| p.manifest == md.manifest)
                .map(|i| i as u32);
            let Some(coordinate) = claimant.get(&md.manifest) else {
                judged.push(Judged {
                    identity: Default::default(),
                    adapter: SmolStr::default(),
                    scoping: Default::default(),
                    importers: Vec::new(),
                    builtins: Default::default(),
                    mentions: Vec::new(),
                    package,
                    owned: Vec::new(),
                    judged: vec![false; md.declarations.len()],
                    users: vec![Vec::new(); md.declarations.len()],
                    ignores: Vec::new(),
                });
                continue;
            };
            let adapter = adapter_by_id(adapters, coordinate);
            let identity = adapter.spec().dependency_identity();
            let judgeable = identity != kndo_contract::extension::DependencyIdentity::Underivable;
            let scoping = adapter.spec().dependency_scoping();
            let unscoped = scoping == kndo_contract::extension::DependencyScoping::Unscoped;
            let ownership = ManifestDeclarations {
                package,
                ..ManifestDeclarations::unjudged(md.manifest.clone(), Vec::new())
            };
            let owned: Vec<u32> = (0..self.files.len())
                .filter(|&i| ownership.owns(self.files[i].path.as_str(), owners[i]))
                .map(|i| i as u32)
                .collect();
            // This adapter's own files, then every claimed file whose suffix it
            // declares as an importer of its ecosystem.
            let read_from: Vec<&BTreeMap<&str, Vec<u32>>> = specifiers
                .get(coordinate.as_str())
                .into_iter()
                .chain(
                    adapter
                        .spec()
                        .dependency_importers()
                        .iter()
                        .filter_map(|s| by_suffix.get(&s.to_ascii_lowercase())),
                )
                .collect();
            let users: Vec<Vec<u32>> = md
                .declarations
                .iter()
                .map(|dd| {
                    let mut using: Vec<u32> = read_from
                        .iter()
                        .flat_map(|m| m.iter())
                        .filter(|(spec, _)| judgeable && identity.names(spec, dd.name.as_str()))
                        .flat_map(|(_, importers)| importers.iter().copied())
                        .collect();
                    using.sort_unstable();
                    using.dedup();
                    using.sort_unstable();
                    using.dedup();
                    using
                })
                .collect();
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
                identity,
                adapter: coordinate.clone(),
                scoping,
                importers: adapter.spec().dependency_importers().to_vec(),
                builtins: adapter.spec().dependency_builtins().clone(),
                mentions: {
                    let mut names = mentions.remove(&md.manifest).unwrap_or_default();
                    names.sort();
                    names.dedup();
                    names
                },
                package,
                owned,
                judged: claims,
                users,
                ignores: adapter.spec().ignores().to_vec(),
            });
        }
        for (md, j) in self.manifest_declarations.iter_mut().zip(judged) {
            md.identity = j.identity;
            md.adapter = j.adapter;
            md.scoping = j.scoping;
            md.importers = j.importers;
            md.builtins = j.builtins;
            md.mentions = j.mentions;
            md.package = j.package;
            md.owned = j.owned;
            md.judged = j.judged;
            md.users = j.users;
            md.ignores = j.ignores;
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

    // One read of every manifest, shared by every pass that used to walk them
    // again: packages, dependencies, mentions, units and roots all come from
    // the same value, so two passes can never disagree about what a manifest
    // declared.
    let reads = crate::project::read_manifests(files, adapters, &known);
    let project = crate::project::assemble(&reads);
    let packages = package_map(&reads);
    let cx = ResolveContext::with_packages(&known, &packages);

    let mut graph_files: Vec<GraphFile> = claims
        .iter()
        .zip(evidence)
        .map(|(c, ev)| {
            let f = &files[c.file_index];
            let spec = adapters[c.adapter_index].spec();
            let dispatched = crate::dispatch::apply(&ev, spec.dispatch_rules());
            let unit = project.unit_of(&f.path);
            let published = publishes(spec.published_surface(), &project, unit, &ev);
            GraphFile {
                path: f.path.clone(),
                adapter: SmolStr::new(spec.coordinate()),
                hash_hex: f.hash.iter().map(|b| format!("{b:02x}")).collect(),
                evidence: ev,
                sees: Vec::new(),
                regions: Vec::new(),
                published,
                anchored: Vec::new(),
                dispatched: dispatched.roots,
                exempt: dispatched.exempt,
                dispatch_notes: dispatched.notes,
                unit,
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
        edges.regions = regions_of(&gf.path, &gf.evidence, adapter, &cx, &sorted_paths);
        resolved.push(edges);
    }
    for (gf, edges) in graph_files.iter_mut().zip(resolved) {
        gf.imports = edges.imports;
        gf.import_targets = edges.import_targets;
        gf.unresolved_imports = edges.unresolved_imports;
        gf.sees = edges.sees;
        gf.regions = edges.regions;
        debug_assert_eq!(
            gf.import_targets.len(),
            gf.evidence.imports.len(),
            "import_targets is index-parallel to evidence.imports"
        );
    }

    anchor_manifest_roots(files, adapters, &cx, &project, &reads, &mut graph_files);

    let manifest_declarations = collect_manifest_declarations(&reads);
    let mut discovered: Vec<ProjectPath> = files.iter().map(|f| f.path.clone()).collect();
    discovered.sort();
    let packages = collect_packages(&reads, &manifest_declarations);
    let manifests: Vec<ProjectPath> = reads
        .iter()
        .filter(|r| !r.adapters.is_empty())
        .map(|r| r.manifest.clone())
        .collect();
    let mut graph = Graph {
        files: graph_files,
        manifests,
        manifest_declarations,
        discovered,
        packages,
        project,
    };
    graph.judge_dependency_usage(&reads, adapters);
    graph
}

/// The declared packages, name-sorted and deduplicated (first declaration
/// wins, like the resolution map): each anchored to the declaring manifest in
/// its own directory when one exists, else to the manifest that emitted it.
fn collect_packages(
    reads: &[crate::project::ManifestRead],
    declarations: &[ManifestDeclarations],
) -> Vec<GraphPackage> {
    // Keyed by (name, dir): parallel trees legitimately duplicate a package
    // name (guava's android/ mirror), and ownership is directory truth.
    let mut out: BTreeMap<(SmolStr, SmolStr), GraphPackage> = BTreeMap::new();
    for read in reads {
        for pkg in &read.evidence.packages {
            out.entry((pkg.name.clone(), pkg.dir.clone()))
                .or_insert(GraphPackage {
                    name: pkg.name.clone(),
                    dir: pkg.dir.clone(),
                    manifest: read.manifest.clone(),
                });
        }
    }
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

/// One entry per manifest, path-sorted, declarations name-sorted — the
/// deterministic projection of every adapter's `manifest_dependencies`.
fn collect_manifest_declarations(
    reads: &[crate::project::ManifestRead],
) -> Vec<ManifestDeclarations> {
    // Every manifest has an entry, declarations or not: a manifest that declares
    // nothing is still the one its files' imports answer to. A launcher is not
    // a manifest and has none — it declares roots and nothing else.
    reads
        .iter()
        .filter(|r| !r.adapters.is_empty())
        .map(|read| {
            let manifest = read.manifest.clone();
            let mut declarations = read.evidence.dependencies.clone();
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
            ManifestDeclarations::unjudged(manifest, declarations)
        })
        .collect()
}

impl ManifestDeclarations {
    /// The declarations before assembly judged them: no adapter, nothing owned,
    /// nothing counted.
    fn unjudged(
        manifest: ProjectPath,
        declarations: Vec<kndo_contract::adapter::DependencyDeclaration>,
    ) -> ManifestDeclarations {
        ManifestDeclarations {
            manifest,
            declarations,
            identity: Default::default(),
            adapter: SmolStr::default(),
            scoping: Default::default(),
            importers: Vec::new(),
            builtins: Default::default(),
            mentions: Vec::new(),
            package: None,
            owned: Vec::new(),
            judged: Vec::new(),
            users: Vec::new(),
            ignores: Vec::new(),
        }
    }
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
fn package_map(reads: &[crate::project::ManifestRead]) -> BTreeMap<SmolStr, PackageEntry> {
    let mut packages: BTreeMap<SmolStr, PackageEntry> = BTreeMap::new();
    for read in reads {
        for pkg in &read.evidence.packages {
            packages.entry(pkg.name.clone()).or_insert(pkg.clone());
        }
    }
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

/// The pools behind one file's adapter-bounded reaches, as graph ids: a
/// `Scoped` token (the region behind the adapter's own word) and, until a
/// manifest names the unit, `Unit`. Each distinct reach the evidence uses is
/// answered once per file. An unanswerable one is ABSENT, and judgment treats
/// its declarations as Exported.
fn regions_of(
    path: &ProjectPath,
    evidence: &kndo_contract::evidence::FileEvidence,
    adapter: &dyn Extension,
    cx: &ResolveContext<'_>,
    sorted_paths: &[ProjectPath],
) -> Vec<(Reach, Vec<u32>)> {
    let mut reaches: Vec<&Reach> = evidence
        .declarations
        .iter()
        .map(|d| &d.reach)
        .filter(|r| matches!(r, Reach::Scoped { .. } | Reach::Unit))
        .collect();
    reaches.sort();
    reaches.dedup();
    let mut out = Vec::new();
    for reach in reaches {
        let Some(region) = adapter.seen_from(path, reach, cx) else {
            continue;
        };
        let mut ids: Vec<u32> = region
            .iter()
            .filter_map(|p| sorted_paths.binary_search(p).ok().map(|i| i as u32))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        out.push((reach.clone(), ids));
    }
    out
}

/// Is this file on its unit's published surface? Only where the language's
/// units publish every export ([`PublishedSurface::Exports`] — under
/// `Entries` the entries already anchor the surface), the unit is a
/// published library, and the file declares something exported at the top
/// level. The engine's own statement of what nine adapters spelled as a
/// whole-file production root on every non-test file.
fn publishes(
    surface: PublishedSurface,
    project: &crate::project::Project,
    unit: Option<u32>,
    evidence: &FileEvidence,
) -> bool {
    surface == PublishedSurface::Exports
        && unit.is_some_and(|u| project.units[u as usize].published)
        && evidence
            .declarations
            .iter()
            .any(|d| d.owner.is_none() && matches!(d.reach, Reach::Exported))
}

/// One file's assembled edges, mirroring the `GraphFile` fields they land in.
struct ResolvedEdges {
    imports: Vec<u32>,
    import_targets: Vec<Vec<u32>>,
    unresolved_imports: u32,
    sees: Vec<u32>,
    regions: Vec<(Reach, Vec<u32>)>,
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
        // A mention (a specifier spelled inside a string literal) is enough for
        // a declared dependency to count as used, never enough to draw a
        // reachability edge to a workspace sibling.
        if matches!(import.shape, kndo_contract::evidence::ImportShape::Mention) {
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
        regions: Vec::new(),
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
    let packages = package_map(&crate::project::read_manifests(files, adapters, &known));
    let cx = ResolveContext::with_packages(&known, &packages);
    let sorted_paths: Vec<ProjectPath> = prev.files.iter().map(|g| g.path.clone()).collect();

    for (ix, file_index) in changed {
        let file = &files[file_index];
        let adapter = adapter_by_id(adapters, &prev.files[ix].adapter);
        let evidence = crate::extract::extract_one(file, adapter, cache);
        let edges = resolve_file(&file.path, &evidence, adapter, &cx, &sorted_paths);
        let dispatched = crate::dispatch::apply(&evidence, adapter.spec().dispatch_rules());
        let published = publishes(
            adapter.spec().published_surface(),
            &prev.project,
            prev.files[ix].unit,
            &evidence,
        );
        let gf = &mut prev.files[ix];
        gf.regions = regions_of(&file.path, &evidence, adapter, &cx, &sorted_paths);
        gf.published = published;
        gf.evidence = evidence;
        gf.dispatched = dispatched.roots;
        gf.exempt = dispatched.exempt;
        gf.dispatch_notes = dispatched.notes;
        gf.imports = edges.imports;
        gf.import_targets = edges.import_targets;
        gf.unresolved_imports = edges.unresolved_imports;
        // `sees` and `unit` are untouched on purpose, and so is the graph's
        // `project`: each is a pure function of path and (file set,
        // manifests), and this path only runs when all of those are unchanged.
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
    f: impl FnMut(&dyn Extension, SourceFile<'_>),
) {
    for_each_matching(files, adapters, |spec| spec.manifests(), f);
}

/// Every (adapter, discovered file) pair for the globs `globs_of` reads from
/// the adapter's spec — manifests for every manifest pass, launchers for the
/// roots pass alone — in file-path order.
pub(crate) fn for_each_matching(
    files: &[DiscoveredFile],
    adapters: &[Box<dyn Extension>],
    globs_of: impl Fn(&kndo_contract::extension::ExtensionSpec) -> &[SmolStr],
    mut f: impl FnMut(&dyn Extension, SourceFile<'_>),
) {
    let manifest_sets: Vec<Option<globset::GlobSet>> = adapters
        .iter()
        .map(|a| {
            let globs = globs_of(a.spec());
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
    // A manifest under a path the adapter's tool never compiles is somebody
    // else's — a dependency's `package.json` inside `node_modules` declares
    // nothing about this project.
    let ignored = crate::extract::ignore_sets(adapters);
    for file in files {
        for ((adapter, set), ignores) in adapters.iter().zip(&manifest_sets).zip(&ignored) {
            let Some(set) = set else { continue };
            if !set.is_match(file.path.as_str()) || ignores.is_match(file.path.as_str()) {
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
    project: &crate::project::Project,
    reads: &[crate::project::ManifestRead],
    graph_files: &mut [GraphFile],
) {
    let mut anchors: Vec<(usize, Root)> = Vec::new();
    let mut anchor = |file: &ProjectPath, kind: RootKind, confidence: Confidence| {
        let Ok(ix) = graph_files.binary_search_by(|x| x.path.cmp(file)) else {
            return;
        };
        anchors.push((
            ix,
            Root {
                target: RootTarget::WholeFile,
                kind,
                confidence,
            },
        ));
    };
    // A unit's entry is the manifest's own statement about how the build is
    // entered, so it anchors `Certain`; an adapter that only guesses an entry
    // reports it as a manifest root with a confidence of its own.
    for (file, kind) in project.entry_roots() {
        anchor(file, kind, Confidence::Certain);
    }
    // A unit's kind gives its files their role: a test set's files are what
    // the runner discovers, a tooling or example set's are built to build
    // something else — the manifest said which set the directory is, hence
    // Certain. A library's files are reached through its published surface
    // and an executable's through its entries, so neither anchors here.
    for gf in graph_files.iter() {
        let Some(unit) = gf.unit else { continue };
        let kind = match project.units[unit as usize].kind {
            UnitKind::Test | UnitKind::Bench => RootKind::Test,
            UnitKind::Tooling | UnitKind::Example => RootKind::Tooling,
            UnitKind::Library | UnitKind::Executable => continue,
            _ => RootKind::Tooling,
        };
        anchor(&gf.path, kind, Confidence::Certain);
    }
    for read in reads {
        for root in &read.evidence.roots {
            anchor(&root.file, root.kind, root.confidence);
        }
    }
    // The bridge: the hook `extract_manifest` replaces. Launchers reach this
    // pass and no other — they declare roots, never a package
    // ([`ExtensionSpecBuilder::launchers`]).
    //
    // [`ExtensionSpecBuilder::launchers`]: kndo_contract::extension::ExtensionSpecBuilder::launchers
    let mut legacy = |adapter: &dyn Extension, declaring: SourceFile<'_>| {
        for root in adapter.roots(&declaring, cx) {
            anchor(&root.file, root.kind, root.confidence);
        }
    };
    for_each_manifest(files, adapters, &mut legacy);
    for_each_matching(files, adapters, |spec| spec.launchers(), &mut legacy);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str, dir: &str, manifest: &str) -> GraphPackage {
        GraphPackage {
            name: SmolStr::new(name),
            dir: SmolStr::new(dir),
            manifest: ProjectPath::new(manifest),
        }
    }

    #[test]
    fn ownership_is_the_deepest_directory_and_the_earlier_package_on_a_tie() {
        // Two packages for ONE directory is real: a `pom.xml` names itself
        // `group:artifact` while a settings file names the same module bare
        // (Exposed's `exposed-modules-maven` snippet). Whichever the name sort
        // puts first owns the files, so a health row's identity is a function
        // of the package list and never of iteration order.
        let graph = Graph {
            files: Vec::new(),
            manifests: Vec::new(),
            manifest_declarations: Vec::new(),
            discovered: Vec::new(),
            packages: vec![
                package("root", "", "settings.gradle.kts"),
                package(
                    "com.example:snippet",
                    "docs/snippet",
                    "docs/snippet/pom.xml",
                ),
                package("snippet", "docs/snippet", "docs/snippet/pom.xml"),
            ],
            project: Default::default(),
        };
        let owner = |p: &str| {
            graph
                .package_of(p)
                .map(|i| graph.packages[i as usize].name.as_str().to_string())
        };
        assert_eq!(owner("src/main.kt").as_deref(), Some("root"));
        assert_eq!(
            owner("docs/snippet/src/Main.kt").as_deref(),
            Some("com.example:snippet"),
            "the deepest directory wins, and the earlier package breaks the tie"
        );
    }
}
