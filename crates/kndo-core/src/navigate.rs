//! The navigation index: reference resolution as ONE fact, consumed by judgment
//! and by queries alike. `unused` decides "is anything keeping this?" and
//! `used-by` lists "what keeps this, with sites" — the same question at two
//! levels of detail, so they MUST run on the same rules: an index the judge did
//! not use would let navigation explain a world the findings never came from.
//! [`keepers`] is that one spelling — the analysis asks it with `limit 1` (a
//! boolean in witness's clothing), the query verbs ask it for the full list.
//!
//! The index is a pure function of the graph and its reachability, built once
//! per run: name → reference sites over reachable files, import bindings with
//! their sites, whole-surface importers, and `included_by` (the reverse of a
//! file's `Include` imports).
//! Site lists stay borrowed and filters stream with early exit, so `limit 1`
//! costs what the old set-membership checks did.

use crate::analysis::Reachability;
use crate::graph::Graph;
use kndo_contract::evidence::{ImportShape, Reach, RootKind, RootTarget, SymbolKind};
use kndo_contract::vocab::{Confidence, Span};
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};

/// The one projection of the three reachability floods into the color a
/// navigator speaks: `production` wins, then `test-only`, then `tooling-only`,
/// then `unreachable` — the same precedence the analyses' judgments imply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ReachColor {
    Production,
    TestOnly,
    ToolingOnly,
    Unreachable,
}

impl ReachColor {
    pub fn of(reach: &Reachability, file: usize) -> ReachColor {
        use kndo_contract::evidence::RootKind as R;
        if reach.by(R::Production).get(file).copied().unwrap_or(false) {
            ReachColor::Production
        } else if reach.by(R::Test).get(file).copied().unwrap_or(false) {
            ReachColor::TestOnly
        } else if reach.by(R::Tooling).get(file).copied().unwrap_or(false) {
            ReachColor::ToolingOnly
        } else {
            ReachColor::Unreachable
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ReachColor::Production => "production",
            ReachColor::TestOnly => "test-only",
            ReachColor::ToolingOnly => "tooling-only",
            ReachColor::Unreachable => "unreachable",
        }
    }
}

/// One reference site: which file, where in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
    pub file: u32,
    pub span: Span,
}

pub struct Index {
    /// name → its reference sites across REACHABLE files, in file order. Serves
    /// the member dispatch pool (all reachable files) whole, and the lexical
    /// pools (own file + `included_by`, or a scoped region) filtered.
    sites_by_name: BTreeMap<SmolStr, Vec<Site>>,
    /// (target file, imported name) → the importing sites that bind it.
    bound: BTreeMap<(u32, SmolStr), Vec<Site>>,
    /// Per target file: importers that take its whole surface (namespace /
    /// side-effect / glob — shapes the engine cannot see through).
    surface_importers: Vec<Vec<Site>>,
    /// Reverse of `sees`: reachable viewers only, ascending.
    included_by: Vec<Vec<u32>>,
    reachable: Vec<bool>,
    /// The type surfaces, by NAME and over EVERY file: what each type declares,
    /// and which types relate to which. A supertype in a file no root reaches
    /// still shapes what its subtypes must declare, so reachability is not a
    /// filter here. Same-named types union their surfaces — an
    /// over-approximation, which is the keep-alive direction for both
    /// consumers (a witness keeps its member alive; an overridden member is
    /// not advised to narrow).
    members_of: BTreeMap<SmolStr, BTreeSet<SmolStr>>,
    supertypes_of: BTreeMap<SmolStr, BTreeSet<SmolStr>>,
    subtypes_of: BTreeMap<SmolStr, BTreeSet<SmolStr>>,
    /// Where a name may legally be used, as structure — see
    /// [`crate::scopes::Scopes`].
    scopes: crate::scopes::Scopes,
    /// (file, the owner that fences a heirs member, with its namespace or
    /// not) → the files a heirs-reaching member of that owner pools over:
    /// the owner's file, the files declaring its subtypes transitively, and
    /// the namespace's when granted. Computed for every fence some member
    /// names, so a lookup never allocates.
    heirs_pools: BTreeMap<(u32, u32, bool), Vec<u32>>,
}

/// The files from which an unqualified reference counts as a use of a
/// declaration — the one reading of [`Reach`] the judge and the navigator
/// share. A bounded reach names its files from the scope forest or, until a
/// manifest bounds it, from the adapter's own enumeration; a reach the engine
/// cannot bound is published surface, the keep-alive direction, so a rung this
/// build does not know can never accuse.
#[derive(Debug, Clone, Copy)]
pub enum Pool<'a> {
    /// Its own file, plus the files that see it without an import.
    Own,
    /// Exactly these files, ascending.
    Files(&'a [u32]),
    /// Nameable from anywhere: entries and whole-surface importers keep it.
    Published,
}

/// Why a declaration is alive — each variant carries the evidence a navigator
/// shows and the judge counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keeper {
    /// A reference to its name from inside its legal pool.
    Reference { site: Site },
    /// An import binding the module-system name (or its exported alias).
    Binding { site: Site },
    /// A root anchoring the declaration itself (or its owner).
    Root { kind: RootKind },
    /// A root the engine's dispatch derived from a marker on the declaration
    /// (or its owner), under the language's rules.
    Dispatch { kind: RootKind },
    /// The source itself exempts the declaration from the unused judgment (an
    /// `allow(dead_code)`-class marker, dispatched).
    Exempt,
    /// A whole-file entry hands out the exported surface this rides.
    EntrySurface,
    /// An importer takes the file's whole surface (namespace/side-effect).
    SurfaceImport { site: Site },
    /// A member riding its owner: an importer binds the owner's name.
    OwnerBinding { site: Site },
    /// The member satisfies a surface its owner promised: an override, an
    /// interface method, a protocol requirement. Alive while its owner is —
    /// no call site can be required to exist, because the caller holds the
    /// SUPERTYPE. `of` names it: the supertype the graph resolved, or — where
    /// the base is outside the project — the base or marker the language's
    /// own rule named.
    Witness { of: SmolStr },
    /// The unit compiling this file publishes its exported API, and this
    /// declaration is on it: the outside world is the consumer no call site
    /// can show.
    Published { unit: SmolStr },
}

impl Index {
    pub fn build(graph: &Graph, reach: &Reachability, scopes: crate::scopes::Scopes) -> Index {
        let n = graph.files.len();
        let reachable: Vec<bool> = (0..n).map(|i| reach.any(i)).collect();
        let mut sites_by_name: BTreeMap<SmolStr, Vec<Site>> = BTreeMap::new();
        let mut bound: BTreeMap<(u32, SmolStr), Vec<Site>> = BTreeMap::new();
        let mut surface_importers: Vec<Vec<Site>> = vec![Vec::new(); n];
        let mut included_by: Vec<Vec<u32>> = vec![Vec::new(); n];
        for (i, f) in graph.files.iter().enumerate() {
            if !reachable[i] {
                continue;
            }
            for r in &f.evidence.references {
                sites_by_name.entry(r.name.clone()).or_default().push(Site {
                    file: i as u32,
                    span: r.span,
                });
            }
            for &m in &f.includes {
                included_by[m as usize].push(i as u32);
            }
            for (import, targets) in f.evidence.imports.iter().zip(&f.import_targets) {
                for &t in targets {
                    let site = Site {
                        file: i as u32,
                        span: import.span,
                    };
                    match &import.shape {
                        ImportShape::Bindings(bs) | ImportShape::Reexport(bs) => {
                            for b in bs {
                                bound.entry((t, b.imported.clone())).or_default().push(site);
                            }
                        }
                        // A mount hands out nothing: it says where the target
                        // sits in the forest, and the parent names what it
                        // holds by qualifying it — a reference of its own. An
                        // include hands out nothing either: the graph reads it
                        // as sight, which is stronger and exact.
                        ImportShape::Mount { .. } | ImportShape::Include => {}
                        _ => surface_importers[t as usize].push(site),
                    }
                }
            }
        }
        // The type surfaces: every file, reachable or not.
        let mut members_of: BTreeMap<SmolStr, BTreeSet<SmolStr>> = BTreeMap::new();
        let mut supertypes_of: BTreeMap<SmolStr, BTreeSet<SmolStr>> = BTreeMap::new();
        let mut subtypes_of: BTreeMap<SmolStr, BTreeSet<SmolStr>> = BTreeMap::new();
        for f in &graph.files {
            for d in &f.evidence.declarations {
                if let Some(owner) = d.owner {
                    members_of
                        .entry(f.evidence.declarations[owner.index()].name.clone())
                        .or_default()
                        .insert(d.name.clone());
                }
            }
            for r in &f.evidence.relations {
                let from = f.evidence.declarations[r.from.index()].name.clone();
                supertypes_of
                    .entry(from.clone())
                    .or_default()
                    .insert(r.to.clone());
                subtypes_of.entry(r.to.clone()).or_default().insert(from);
            }
        }
        let mut index = Index {
            sites_by_name,
            bound,
            surface_importers,
            included_by,
            reachable,
            members_of,
            supertypes_of,
            subtypes_of,
            scopes,
            heirs_pools: BTreeMap::new(),
        };
        index.heirs_pools = index.heirs_pools_of(graph);
        index
    }

    /// Every fence a heirs-reaching member names, with its pool. The subtypes
    /// are the relation walk's, by name and over every file, so a same-named
    /// type elsewhere widens the pool — the keep-alive direction.
    fn heirs_pools_of(&self, graph: &Graph) -> BTreeMap<(u32, u32, bool), Vec<u32>> {
        let mut files_of_type: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
        for (i, f) in graph.files.iter().enumerate() {
            for d in &f.evidence.declarations {
                if d.kind == SymbolKind::Type {
                    files_of_type
                        .entry(d.name.as_str())
                        .or_default()
                        .push(i as u32);
                }
            }
        }
        let mut out: BTreeMap<(u32, u32, bool), Vec<u32>> = BTreeMap::new();
        for (i, f) in graph.files.iter().enumerate() {
            for d in &f.evidence.declarations {
                let (Reach::Heirs { and_namespace }, Some(owner)) = (&d.reach, d.owner) else {
                    continue;
                };
                let key = (i as u32, owner.index() as u32, *and_namespace);
                if out.contains_key(&key) {
                    continue;
                }
                let fence = f.evidence.declarations[owner.index()].name.as_str();
                let mut pool: Vec<u32> = vec![i as u32];
                for heir in self.subtypes(fence) {
                    if let Some(files) = files_of_type.get(heir.as_str()) {
                        pool.extend_from_slice(files);
                    }
                }
                if *and_namespace && let Some(namespace) = self.scopes.namespace_pool(i, 0) {
                    pool.extend_from_slice(namespace);
                }
                pool.sort_unstable();
                pool.dedup();
                out.insert(key, pool);
            }
        }
        out
    }

    /// The reach a declaration really has: its own after every owner above it
    /// caps it, and after the fence any mount above its FILE imposes. The one
    /// seam every judgment reads — a `pub` item of a privately mounted module
    /// is nameable where that mount says and nowhere else, which no reading of
    /// the file alone can tell.
    pub fn effective(&self, graph: &Graph, file: usize, decl: usize) -> Reach {
        let f = &graph.files[file];
        let own = f.evidence.effective_reach_at(decl);
        match &f.mount_cap {
            Some(cap) => own.capped_by(cap),
            None => own,
        }
    }

    /// The pool a declaration is nameable from, by its effective reach — see
    /// [`Index::pool_of`] — with a heirs reach resolved against the owner
    /// that fences it: the declaration carrying the reach, self first up the
    /// owner chain, names the fence, whichever member inherits it.
    pub fn pool_for<'a>(&'a self, graph: &'a Graph, file: usize, decl: usize) -> Pool<'a> {
        let f = &graph.files[file];
        let effective = self.effective(graph, file, decl);
        let Reach::Heirs { and_namespace } = effective else {
            return self.pool_of(graph, file, &effective);
        };
        let mut cursor = decl;
        for _ in 0..=f.evidence.declarations.len() {
            let d = &f.evidence.declarations[cursor];
            if matches!(d.reach, Reach::Heirs { .. }) {
                let Some(fence) = d.owner else {
                    return Pool::Published;
                };
                return self
                    .heirs_pools
                    .get(&(file as u32, fence.index() as u32, and_namespace))
                    .map_or(Pool::Published, |p| Pool::Files(p));
            }
            match d.owner {
                Some(o) => cursor = o.index(),
                None => return Pool::Published,
            }
        }
        Pool::Published
    }

    /// The type this member's owner promised it to, if any: a supertype —
    /// transitively — that declares a member of the same name. `None` when the
    /// owner promised nothing carrying it.
    pub fn witnessed_type(&self, owner: &str, member: &str) -> Option<SmolStr> {
        self.walk(&self.supertypes_of, owner)
            .into_iter()
            .find(|t| self.declares(t, member))
    }

    /// Every type that is a SUBtype of this one, transitively — the types
    /// whose own files reach its members by a bare name, because inheritance
    /// puts them in their scopes.
    pub fn subtypes(&self, type_name: &str) -> Vec<SmolStr> {
        self.walk(&self.subtypes_of, type_name)
    }

    /// Does some SUBtype — transitively — declare a member of this name? Then
    /// the member is overridden, and narrowing it below what its overriders
    /// need is not advice, it is a compile error.
    pub fn is_overridden(&self, owner: &str, member: &str) -> bool {
        self.walk(&self.subtypes_of, owner)
            .iter()
            .any(|t| self.declares(t, member))
    }

    fn declares(&self, type_name: &SmolStr, member: &str) -> bool {
        self.members_of
            .get(type_name)
            .is_some_and(|m| m.contains(member))
    }

    /// Every type reachable from `start` through `edges`, excluding `start`
    /// itself — a plain closure over a name graph small enough to walk whole,
    /// and cycle-safe (a language's type graph should be acyclic; a malformed
    /// one must not hang the run).
    fn walk(&self, edges: &BTreeMap<SmolStr, BTreeSet<SmolStr>>, start: &str) -> Vec<SmolStr> {
        let mut seen: BTreeSet<&SmolStr> = BTreeSet::new();
        let mut out: Vec<SmolStr> = Vec::new();
        let mut frontier: Vec<&SmolStr> = edges.get(start).into_iter().flatten().collect();
        while let Some(next) = frontier.pop() {
            if next.as_str() == start || !seen.insert(next) {
                continue;
            }
            out.push(next.clone());
            frontier.extend(edges.get(next).into_iter().flatten());
        }
        out.sort();
        out
    }

    pub fn reachable(&self, file: u32) -> bool {
        self.reachable.get(file as usize).copied().unwrap_or(false)
    }

    /// Reachable viewers of a file (the reverse of `sees`), ascending.
    pub fn included_by(&self, file: usize) -> &[u32] {
        &self.included_by[file]
    }

    pub fn surface_importers(&self, file: usize) -> &[Site] {
        &self.surface_importers[file]
    }

    /// Every reachable reference site for a name, in file order.
    pub fn reference_sites(&self, name: &str) -> &[Site] {
        self.sites_by_name
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// The files a namespace-reaching declaration in `file` pools over — see
    /// [`crate::scopes::Scopes::namespace_pool`].
    /// The files this one COMPILES WITH — see [`crate::scopes::Scopes::covisible`].
    pub fn covisible(&self, file: usize) -> &[u32] {
        self.scopes.covisible(file)
    }

    pub fn namespace_pool(&self, file: usize, up: u32) -> Option<&[u32]> {
        self.scopes.namespace_pool(file, up)
    }

    /// The pool a declaration of `reach` in `file` is nameable from — see
    /// [`Pool`]. A namespace is bounded by the scope forest; a unit by the
    /// forest where a manifest named it and by the adapter until one does; a
    /// token only ever by the adapter.
    pub fn pool_of<'a>(&'a self, graph: &'a Graph, file: usize, reach: &Reach) -> Pool<'a> {
        let bounded = |files: Option<&'a [u32]>| match files {
            Some(files) => Pool::Files(files),
            None => Pool::Published,
        };
        let f = &graph.files[file];
        match reach {
            Reach::Owner | Reach::File => Pool::Own,
            Reach::Namespace { up } => bounded(self.scopes.namespace_pool(file, *up)),
            Reach::Unit { up: 0 } => match f.unit {
                Some(u) => Pool::Files(self.scopes.unit_pool(u)),
                // No manifest named the unit: what still bounds the reach is
                // the language's to say — the tree its mounts spell, or the
                // namespace it declared where the two are one thing. Neither,
                // and the reach is UNBOUNDED and says so: keep-alive, the
                // typed absence, never a directory an adapter walked.
                None => bounded(self.scopes.unnamed_unit_pool(file)),
            },
            Reach::Unit { up: 1 } => bounded(f.unit.and_then(|u| self.scopes.group_pool(u))),
            Reach::Directory { up } => bounded(self.scopes.directory_pool(file, *up)),
            Reach::Named { namespace } => bounded(self.scopes.named_pool(file, namespace)),
            // A unit's group beyond its aggregator, a reach that is its
            // owner's (resolved by the caller through the effective reach),
            // and any reach this build does not know: published surface.
            _ => Pool::Published,
        }
    }

    /// Importing sites binding `name` from `target`'s surface.
    pub fn binding_sites(&self, target: u32, name: &str) -> &[Site] {
        self.bound
            .get(&(target, SmolStr::new(name)))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

/// A capped collector: `push` reports "full", and every call site returns as
/// soon as it is — `limit 1` is the analysis's boolean, larger limits are the
/// navigator's listing with its elision cap.
struct Capped {
    out: Vec<Keeper>,
    limit: usize,
}

impl Capped {
    fn push(&mut self, keeper: Keeper) -> bool {
        self.out.push(keeper);
        self.out.len() >= self.limit
    }
}

/// The unit whose published surface this file is on, by name — see
/// [`crate::graph::GraphFile::published`].
fn publishing_unit(graph: &Graph, f: &crate::graph::GraphFile) -> Option<SmolStr> {
    if !f.published {
        return None;
    }
    f.unit.map(|u| graph.project.units[u as usize].name.clone())
}

/// Everything keeping one declaration alive, capped at `limit`. The rules are
/// `unused`'s, spelled once:
///
/// - Reach decides the pool ([`Index::pool_of`]): `Private` pools its own file
///   plus the files that see it; a bounded reach pools its files; `Exported`
///   — and any reach the engine could not bound — is published surface, kept
///   by entries and whole-surface importers too.
/// - Members (owned, or method-kind without a local owner) dispatch through
///   values: their reference pool is every reachable file, and any
///   whole-surface importer keeps them. They ride their owner's handed-out
///   surface — unless their own region is bounded (an `internal` method of a
///   public class) or they are `Private` (never handed out).
/// - A published unit hands its exported surface to the outside world: an
///   exported declaration there is kept by its consumers no call site can
///   show, and a member rides its owner's the way it rides an entry's.
/// - An import binding the module-system name (or exported alias) keeps a free
///   declaration whatever its reach — the adapter resolved that edge as legal.
/// - An exemption the source asked for outranks every question: it is listed
///   first, and alone it keeps.
pub fn keepers(
    graph: &Graph,
    index: &Index,
    file: usize,
    decl: usize,
    limit: usize,
) -> Vec<Keeper> {
    let f = &graph.files[file];
    let d = &f.evidence.declarations[decl];
    let mut kept = Capped {
        out: Vec::new(),
        limit: limit.max(1),
    };
    // The reach the engine pools by: the declared one after every owner above
    // caps it — a public member of a file-private class reaches the file.
    let effective = index.effective(graph, file, decl);

    // The pool a bounded reach names, or `None` for published surface.
    let (exported, region) = match index.pool_for(graph, file, decl) {
        Pool::Published => (true, None),
        Pool::Files(r) => (false, Some(r)),
        Pool::Own => (false, None),
    };
    if f.exempt.binary_search(&(decl as u32)).is_ok() && kept.push(Keeper::Exempt) {
        return kept.out;
    }
    let entry_surface = f.roots().any(|r| matches!(r.target, RootTarget::WholeFile));

    // Roots anchoring the declaration itself, its owner, or a member it owns.
    // The last direction is the launcher's: a class whose `main` the runtime
    // names is named through it, and the member cannot outlive its type. Only
    // a CERTAIN root travels that way — `Probable`/`Possible` say the member
    // MIGHT be dispatched, and inheriting a maybe is how silence spreads from
    // one method to everything around it.
    let anchors = |r: &kndo_contract::evidence::Root| match &r.target {
        RootTarget::Declaration(id) => {
            id.index() == decl
                || d.owner.is_some_and(|o| o.index() == id.index())
                || (r.confidence == Confidence::Certain
                    && f.evidence.declarations[id.index()]
                        .owner
                        .is_some_and(|o| o.index() == decl))
        }
        _ => false,
    };
    for r in f.evidence.roots.iter().chain(&f.anchored) {
        if anchors(r) && kept.push(Keeper::Root { kind: r.kind }) {
            return kept.out;
        }
    }
    for r in &f.dispatched {
        if anchors(r) && kept.push(Keeper::Dispatch { kind: r.kind }) {
            return kept.out;
        }
    }

    let member = d.owner.is_some() || d.kind == SymbolKind::Method;
    if member {
        // A surface its owner promised: no call site can be required to exist,
        // because every caller holds the SUPERTYPE and dispatches through it.
        // Two halves, one keeper: the supertype the graph RESOLVED, and the
        // base a rule NAMED where the supertype is outside the project.
        if let Some(owner) = d.owner
            && let Some(of) = index.witnessed_type(
                f.evidence.declarations[owner.index()].name.as_str(),
                d.name.as_str(),
            )
            && kept.push(Keeper::Witness { of })
        {
            return kept.out;
        }
        if let Some(of) = f.stated_witness(decl)
            && kept.push(Keeper::Witness { of: of.clone() })
        {
            return kept.out;
        }
        // Dispatch is not lexical: any reachable reference to the name.
        for &site in index.reference_sites(d.name.as_str()) {
            if kept.push(Keeper::Reference { site }) {
                return kept.out;
            }
        }
        // A surface hands out what the language exports and nothing else, so a
        // member whose effective reach stops at its owner or its file rides
        // none of the three surface keepers below — not a namespace importer,
        // not an entry, not its owner's binding. Its name cannot be spelled
        // outside this file; the dispatch pool above, which is every reachable
        // file, is the whole of its keep-alive.
        let handed_out = !matches!(effective, Reach::Owner | Reach::File);
        // Any whole-surface importer keeps every member it could name (the
        // engine cannot see through a namespace import, so it degrades toward
        // keep-alive for everything the import can reach).
        if handed_out {
            for &site in index.surface_importers(file) {
                if kept.push(Keeper::SurfaceImport { site }) {
                    return kept.out;
                }
            }
        }
        let surface_reach = match d.owner {
            Some(owner) => index.effective(graph, file, owner.index()),
            None => effective.clone(),
        };
        let owner_surface_exported = matches!(
            index.pool_for(graph, file, d.owner.map_or(decl, |o| o.index())),
            Pool::Published
        );
        // A heirs member rides its owner's published surface though its pool
        // is bounded: a subtype outside the tree may name it, which no pool
        // can hold.
        let rides_published = region.is_none() || matches!(effective, Reach::Heirs { .. });
        if handed_out
            && owner_surface_exported
            && rides_published
            && let Some(unit) = publishing_unit(graph, f)
            && kept.push(Keeper::Published { unit })
        {
            return kept.out;
        }
        if handed_out
            && entry_surface
            && owner_surface_exported
            && region.is_none()
            && kept.push(Keeper::EntrySurface)
        {
            return kept.out;
        }
        // Riding the owner: an importer binding the owner's name — from inside
        // the member's region when it has one.
        if handed_out && let Some(o) = d.owner {
            let od = &f.evidence.declarations[o.index()];
            if !matches!(surface_reach, Reach::Owner | Reach::File) {
                let mut owner_names: Vec<&str> = vec![od.name.as_str()];
                if let Some(alias) = &od.exported_as {
                    owner_names.push(alias.as_str());
                }
                for name in owner_names {
                    for &site in index.binding_sites(file as u32, name) {
                        let inside = match region {
                            None => true,
                            Some(r) => r.binary_search(&site.file).is_ok(),
                        };
                        if inside && kept.push(Keeper::OwnerBinding { site }) {
                            return kept.out;
                        }
                    }
                }
            }
        }
    } else {
        // The lexical pool: own file plus viewers, the bounded region
        // (qualified in-region uses need no import), and — for a name the
        // file HANDS OUT — the files it compiles with. An export is nameable
        // inside its own namespace with no import at all, so a sibling's bare
        // use is a use; a file-private name is not, which is why the
        // compilation is read only where the reach is Exported.
        let seen = index.included_by(file);
        let compiled_with = exported.then(|| index.covisible(file));
        for &site in index.reference_sites(d.name.as_str()) {
            let in_pool = site.file as usize == file
                || seen.binary_search(&site.file).is_ok()
                || compiled_with.is_some_and(|c| c.binary_search(&site.file).is_ok())
                || region.is_some_and(|r| {
                    r.binary_search(&site.file).is_ok() && index.reachable(site.file)
                });
            if in_pool && kept.push(Keeper::Reference { site }) {
                return kept.out;
            }
        }
        // Direct bindings of the name or its exported alias, whatever the reach.
        let mut names: Vec<&str> = vec![d.name.as_str()];
        if let Some(alias) = &d.exported_as {
            names.push(alias.as_str());
        }
        for name in names {
            for &site in index.binding_sites(file as u32, name) {
                if kept.push(Keeper::Binding { site }) {
                    return kept.out;
                }
            }
        }
        // Whole-surface importers: for published surface, all of them; for a
        // bounded region, only importers inside it.
        for &site in index.surface_importers(file) {
            let counts = exported || region.is_some_and(|r| r.binary_search(&site.file).is_ok());
            if counts && kept.push(Keeper::SurfaceImport { site }) {
                return kept.out;
            }
        }
        if exported
            && let Some(unit) = publishing_unit(graph, f)
            && kept.push(Keeper::Published { unit })
        {
            return kept.out;
        }
        if exported && entry_surface && kept.push(Keeper::EntrySurface) {
            return kept.out;
        }
    }
    kept.out
}
