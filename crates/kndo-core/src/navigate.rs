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
//! their sites, whole-surface importers, and `seen_by` (the reverse of `sees`).
//! Site lists stay borrowed and filters stream with early exit, so `limit 1`
//! costs what the old set-membership checks did.

use crate::analysis::Reachability;
use crate::graph::Graph;
use kndo_contract::evidence::{ImportShape, Reach, RootKind, RootTarget, SymbolKind};
use kndo_contract::vocab::Span;
use smol_str::SmolStr;
use std::collections::BTreeMap;

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
    /// pools (own file + `seen_by`, or a scoped region) filtered.
    sites_by_name: BTreeMap<SmolStr, Vec<Site>>,
    /// (target file, imported name) → the importing sites that bind it.
    bound: BTreeMap<(u32, SmolStr), Vec<Site>>,
    /// Per target file: importers that take its whole surface (namespace /
    /// side-effect / glob — shapes the engine cannot see through).
    surface_importers: Vec<Vec<Site>>,
    /// Reverse of `sees`: reachable viewers only, ascending.
    seen_by: Vec<Vec<u32>>,
    reachable: Vec<bool>,
}

/// Why a declaration is alive — each variant carries the evidence a navigator
/// shows and the judge counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

impl Index {
    pub fn build(graph: &Graph, reach: &Reachability) -> Index {
        let n = graph.files.len();
        let reachable: Vec<bool> = (0..n).map(|i| reach.any(i)).collect();
        let mut sites_by_name: BTreeMap<SmolStr, Vec<Site>> = BTreeMap::new();
        let mut bound: BTreeMap<(u32, SmolStr), Vec<Site>> = BTreeMap::new();
        let mut surface_importers: Vec<Vec<Site>> = vec![Vec::new(); n];
        let mut seen_by: Vec<Vec<u32>> = vec![Vec::new(); n];
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
            for &m in &f.sees {
                seen_by[m as usize].push(i as u32);
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
                        _ => surface_importers[t as usize].push(site),
                    }
                }
            }
        }
        Index {
            sites_by_name,
            bound,
            surface_importers,
            seen_by,
            reachable,
        }
    }

    pub fn reachable(&self, file: u32) -> bool {
        self.reachable.get(file as usize).copied().unwrap_or(false)
    }

    /// Reachable viewers of a file (the reverse of `sees`), ascending.
    pub fn seen_by(&self, file: usize) -> &[u32] {
        &self.seen_by[file]
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

/// Everything keeping one declaration alive, capped at `limit`. The rules are
/// `unused`'s, spelled once:
///
/// - Reach decides the pool: `Private` pools its own file plus the files that
///   see it; `Scoped` pools its REGION; `Exported` — and any Scoped token the
///   adapter could not bound — is published surface, kept by entries and
///   whole-surface importers too.
/// - Members (owned, or method-kind without a local owner) dispatch through
///   values: their reference pool is every reachable file, and any
///   whole-surface importer keeps them. They ride their owner's handed-out
///   surface — unless their own region is bounded (an `internal` method of a
///   public class) or they are `Private` (never handed out).
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

    let region_of = |scope: &str| -> Option<&[u32]> {
        f.scoped_regions
            .binary_search_by(|(t, _)| t.as_str().cmp(scope))
            .ok()
            .map(|ix| f.scoped_regions[ix].1.as_slice())
    };
    let (exported, region) = match &d.reach {
        Reach::Exported => (true, None),
        Reach::Scoped { scope } => match region_of(scope) {
            Some(r) => (false, Some(r)),
            None => (true, None),
        },
        Reach::Private => (false, None),
    };
    if f.exempt.binary_search(&(decl as u32)).is_ok() && kept.push(Keeper::Exempt) {
        return kept.out;
    }
    let entry_surface = f.roots().any(|r| matches!(r.target, RootTarget::WholeFile));

    // Roots anchoring the declaration itself, or its owner.
    let anchors = |r: &kndo_contract::evidence::Root| match &r.target {
        RootTarget::Declaration(id) => {
            id.index() == decl || d.owner.is_some_and(|o| o.index() == id.index())
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
        // Dispatch is not lexical: any reachable reference to the name.
        for &site in index.reference_sites(d.name.as_str()) {
            if kept.push(Keeper::Reference { site }) {
                return kept.out;
            }
        }
        // Any whole-surface importer keeps every member (the engine cannot see
        // through a namespace import, so it degrades toward keep-alive).
        for &site in index.surface_importers(file) {
            if kept.push(Keeper::SurfaceImport { site }) {
                return kept.out;
            }
        }
        let surface_reach = match d.owner {
            Some(owner) => &f.evidence.declarations[owner.index()].reach,
            None => &d.reach,
        };
        let owner_surface_exported = matches!(surface_reach, Reach::Exported)
            || matches!(surface_reach, Reach::Scoped { scope } if region_of(scope).is_none());
        if entry_surface
            && owner_surface_exported
            && region.is_none()
            && kept.push(Keeper::EntrySurface)
        {
            return kept.out;
        }
        // Riding the owner: an importer binding the owner's name — from inside
        // the member's region when it has one; never for Private members.
        if !matches!(d.reach, Reach::Private)
            && let Some(o) = d.owner
        {
            let od = &f.evidence.declarations[o.index()];
            if !matches!(od.reach, Reach::Private) {
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
        // The lexical pool: own file plus viewers — or, additionally, the
        // bounded region (qualified in-region uses need no import).
        let seen = index.seen_by(file);
        for &site in index.reference_sites(d.name.as_str()) {
            let in_pool = site.file as usize == file
                || seen.binary_search(&site.file).is_ok()
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
        if exported && entry_surface && kept.push(Keeper::EntrySurface) {
            return kept.out;
        }
    }
    kept.out
}
