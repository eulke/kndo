//! The extraction-side shared types: the file handed to extraction, the project
//! context an extension resolves against, and what resolution/manifest reads return.
//! [`crate::plugin::Plugin`] is the one trait that consumes them; this module
//! holds the data shapes it shares with the engine.

use crate::evidence::{EmbeddedRegion, RootKind};
use crate::manifest::VersionReq;
use crate::vocab::{Confidence, ProjectPath};
use smol_str::SmolStr;
use std::collections::BTreeSet;

/// One file handed to extraction, or one embedded region of it. Adapters
/// never touch the filesystem.
pub struct SourceFile<'a> {
    pub path: &'a ProjectPath,
    pub content: &'a [u8],
    /// `Some` when `content` is one embedded region of the file at `path`: its
    /// language named this extension, and its mode says how the region runs.
    /// `None` for the whole file. Spans an adapter reports are relative to
    /// `content` either way — the sink puts a region's into the file's
    /// coordinates.
    pub region: Option<&'a EmbeddedRegion>,
}

/// A package one manifest declares: the name the ecosystem imports it by, the file
/// its bare specifier resolves to — `None` for ecosystems whose packages have no
/// entry file (a Go module maps import prefixes to directories) — and the directory
/// subpaths resolve against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageEntry {
    pub name: SmolStr,
    pub entry: Option<ProjectPath>,
    /// `/`-separated directory of the declaring manifest; empty at the project root.
    pub dir: SmolStr,
    /// The other names this package answers to, stated by a manifest that
    /// RENAMES it: cargo's `dep = { package = "real" }`, npm's `npm:` alias, a
    /// go.mod `replace`. A specifier spelling an alias resolves to this entry,
    /// and a dependency declaration spelling one is in use.
    pub aliases: Vec<SmolStr>,
    /// The SUBPATHS this package hands out, as its manifest declares them —
    /// npm's `exports` map. Patterns are spelled the way a CONSUMER writes
    /// them (`pkg`, `pkg/client`, `pkg/dist/*`), because a consumer is who
    /// asks; targets are relative to `dir`. A package with no map hands out
    /// `entry` under its own name and nothing else, which is the empty list.
    ///
    /// Distinct from a manifest's own [`crate::manifest::PathAlias`] table:
    /// that one is INTERNAL (npm's `#` imports, tsconfig `paths`) and applies
    /// to files under the declaring directory, this one applies to whoever
    /// names the package, from anywhere.
    pub subpaths: Vec<crate::manifest::PathAlias>,
}

/// The manifest section a dependency declaration sits in, translated to the
/// engine's vocabulary by the declaring adapter. Closed by design like RootKind:
/// analyses read scopes as verdict-changing facts (peer is a contract with the
/// consumer, never a usage claim), so an unknown scope has no honest meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyScope {
    Prod,
    Dev,
    Build,
    Optional,
    Peer,
    /// Stated by the manifest as a transitive requirement (`// indirect` in
    /// go.mod): resolver bookkeeping the project's own code never imports, so
    /// it is never a usage claim and never accused of being unused.
    Transitive,
}

/// One dependency declaration as one manifest states it. `scope: None` means the
/// ecosystem has no sections (go.mod) or the source could not say (a WASM guest
/// speaking the names-only ABI); `version_req: None` means the manifest states no
/// comparable requirement — a workspace/path/git/BOM-managed coordinate — and a
/// comparison the adapter knows it could not perform must stay silent rather than
/// diverge from every real version.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DependencyDeclaration {
    pub name: SmolStr,
    pub scope: Option<DependencyScope>,
    pub version_req: Option<VersionReq>,
}

impl DependencyDeclaration {
    /// A declaration that carries the name and honestly nothing else — the shape
    /// for sources that state no sections or requirements this vocabulary can
    /// compare (go.mod, a names-only ABI guest, JVM builds pending BOM/catalog
    /// modeling). Activation reads the name; version-skew stays silent.
    pub fn name_only(name: SmolStr) -> Self {
        DependencyDeclaration {
            name,
            scope: None,
            version_req: None,
        }
    }
}

/// The project around a file, as the engine lets an adapter see it during import
/// resolution. Grows methods only.
pub struct ResolveContext<'a> {
    known_files: &'a BTreeSet<ProjectPath>,
    packages: Option<&'a std::collections::BTreeMap<SmolStr, PackageEntry>>,
    manifests: Option<&'a std::collections::BTreeMap<ProjectPath, &'a [u8]>>,
    project: Option<&'a ProjectView<'a>>,
}

/// One unit, as RESOLUTION asks about it: what the build compiles it from and
/// what it hands out. The read side of [`crate::manifest::Unit`] — an adapter
/// resolving a specifier needs the roots to walk and the name they hang under,
/// never the dependency list or the aggregator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitView {
    pub name: SmolStr,
    pub kind: crate::manifest::UnitKind,
    pub roots: Vec<crate::manifest::UnitRoot>,
    /// See [`crate::manifest::Unit::namespace_root`].
    pub namespace_root: Option<SmolStr>,
    /// See [`crate::manifest::Unit::is_published`], read once.
    pub published: bool,
    /// Every unit of this project this one compiles against, transitively,
    /// as indices into the view's unit list, ascending. A namespace two units
    /// both declare is two namespaces unless one of them can see the other,
    /// and this is the engine's already-resolved answer to which can.
    pub compiles_against: Vec<u32>,
}

/// Can a file compiled in `viewer` name something of `target`? True for the
/// unit itself, and for every unit it compiles against — a test module on the
/// classpath of the library it exercises reads that library's names as its own.
///
/// `closure` is `viewer`'s [`UnitView::compiles_against`]: transitive and
/// ascending, so the membership test is a binary search. One home, because two
/// crates ask it — the engine when a language says its namespaces span a
/// compilation, and resolution when a namespace is a name inside one.
pub fn unit_sees(closure: &[u32], viewer: u32, target: u32) -> bool {
    viewer == target || closure.binary_search(&target).is_ok()
}

/// What the project's MANIFESTS declared and its files' own clauses say, as
/// the five questions resolution asks — the engine's answers, so an adapter
/// never re-derives a source root from a path convention or re-parses another
/// ecosystem's alias table.
///
/// Every query is a lookup over data the engine assembled once. Absence is
/// honest throughout: no unit compiles a file the manifests never covered, no
/// alias rewrites a specifier no manifest named, and a language whose files
/// declare no namespace answers with none.
pub struct ProjectView<'a> {
    pub(crate) units: &'a [UnitView],
    pub(crate) unit_of: &'a std::collections::BTreeMap<ProjectPath, u32>,
    /// Each alias with the DIRECTORY of the manifest declaring it: the nearest
    /// declaration to the resolving file wins, the way every build system
    /// composes its own configuration.
    pub(crate) aliases: &'a [(SmolStr, crate::manifest::PathAlias)],
    pub(crate) namespaces: &'a std::collections::BTreeMap<ProjectPath, Vec<SmolStr>>,
    pub(crate) in_namespace: &'a std::collections::BTreeMap<Vec<SmolStr>, Vec<ProjectPath>>,
}

impl<'a> ProjectView<'a> {
    /// Assembled by the engine, which owns the indices this borrows.
    pub fn new(
        units: &'a [UnitView],
        unit_of: &'a std::collections::BTreeMap<ProjectPath, u32>,
        aliases: &'a [(SmolStr, crate::manifest::PathAlias)],
        namespaces: &'a std::collections::BTreeMap<ProjectPath, Vec<SmolStr>>,
        in_namespace: &'a std::collections::BTreeMap<Vec<SmolStr>, Vec<ProjectPath>>,
    ) -> ProjectView<'a> {
        ProjectView {
            units,
            unit_of,
            aliases,
            namespaces,
            in_namespace,
        }
    }

    /// The unit compiling `path` — `None` where no manifest covers it.
    pub fn unit_of(&self, path: &ProjectPath) -> Option<&'a UnitView> {
        self.unit_of.get(path).map(|&u| &self.units[u as usize])
    }

    /// Every unit the manifests declared, in the project's own order — what a
    /// language whose imports name no file has to search: a dotted module path
    /// is resolved against the source roots that EXIST, and which those are is
    /// the manifests' answer, never a walk of the tree looking for a shape.
    pub fn units(&self) -> impl Iterator<Item = &'a UnitView> + '_ {
        self.units.iter()
    }

    /// The directories that unit compiles, in path order; empty where no unit
    /// covers the file, which is a language's cue to fall back on nothing.
    pub fn source_roots_of(&self, path: &ProjectPath) -> &'a [crate::manifest::UnitRoot] {
        self.unit_of(path).map_or(&[], |u| &u.roots)
    }

    /// What a specifier rewrites to, in the order the build tries them —
    /// `tsconfig` paths, an npm `exports`/`imports` map, Sass `loadPaths`, a
    /// go.mod `replace`. The pattern's capture fills each target's `*`, or is
    /// appended where the target has none: `@app/x` under `@app/ → src/app`
    /// answers `src/app/x`, and `#types/hot` under `#types/* → ./types/*.d.ts`
    /// answers `types/hot.d.ts`.
    ///
    /// Empty means NO alias named the specifier. An alias that names it and
    /// REFUSES it (npm's `null` subpath) answers with one refusing target, so
    /// a caller can tell "the manifest said no" from "the manifest said
    /// nothing" — see [`crate::manifest::AliasTarget::refuses`].
    ///
    /// `from` decides WHICH declaration applies when several match: the one
    /// declared nearest above the resolving file.
    pub fn alias(&self, from: &ProjectPath, specifier: &str) -> Vec<SmolStr> {
        self.alias_targets(from, specifier)
            .iter()
            .filter(|(t, _)| !t.refuses())
            .map(|(_, path)| path.clone())
            .collect()
    }

    /// [`alias`](Self::alias) with the conditions each rewriting held under,
    /// and the refusals kept — what a caller reads when it needs to know
    /// WHICH branch of a conditional table it took, or that the table refused
    /// the specifier outright. The path of a refusing target is empty.
    pub fn alias_targets(
        &self,
        from: &ProjectPath,
        specifier: &str,
    ) -> Vec<(&'a crate::manifest::AliasTarget, SmolStr)> {
        let mut best: Option<(usize, usize, &crate::manifest::PathAlias)> = None;
        for (dir, alias) in self.aliases {
            if !crate::vocab::is_under(dir, from.as_str()) {
                continue;
            }
            if alias.capture(specifier).is_none() {
                continue;
            }
            // The longest pattern wins first, and the nearest manifest breaks
            // a tie — two aliases spelling one pattern are two build
            // configurations, and the inner one is the one in force.
            let key = (alias.pattern.len(), dir.len());
            if best.is_none_or(|(p, d, _)| (p, d) < key) {
                best = Some((key.0, key.1, alias));
            }
        }
        let Some((_, _, alias)) = best else {
            return Vec::new();
        };
        alias.rewrite(specifier)
    }

    /// The namespace a file declared, as its own clause spelled it — empty for
    /// a language whose files declare none.
    pub fn namespace_of(&self, path: &ProjectPath) -> &'a [SmolStr] {
        self.namespaces.get(path).map_or(&[], Vec::as_slice)
    }

    /// Every file declaring exactly this namespace, in path order — how a
    /// language whose imports name a namespace rather than a file (Kotlin's
    /// `com.example.Thing`) finds what to resolve to.
    pub fn files_in_namespace(&self, from: &ProjectPath, segments: &[SmolStr]) -> Vec<ProjectPath> {
        let all = self
            .in_namespace
            .get(segments)
            .map_or(&[][..], Vec::as_slice);
        // A namespace is a name inside a COMPILATION. Two units writing one
        // package clause are two packages when neither compiles against the
        // other — a GWT super-source tree replacing a class, a shaded copy, an
        // Android variant — and answering with both would let a name in one
        // stand in for a name the other never sees.
        let Some(&owner) = self.unit_of.get(from) else {
            return all.to_vec();
        };
        all.iter()
            .filter(|p| match self.unit_of.get(*p) {
                None => true,
                Some(&u) => unit_sees(&self.units[owner as usize].compiles_against, owner, u),
            })
            .cloned()
            .collect()
    }
}

impl<'a> ResolveContext<'a> {
    pub fn new(known_files: &'a BTreeSet<ProjectPath>) -> Self {
        ResolveContext {
            known_files,
            packages: None,
            manifests: None,
            project: None,
        }
    }

    pub fn with_packages(
        known_files: &'a BTreeSet<ProjectPath>,
        packages: &'a std::collections::BTreeMap<SmolStr, PackageEntry>,
    ) -> Self {
        ResolveContext {
            known_files,
            packages: Some(packages),
            manifests: None,
            project: None,
        }
    }

    /// The same, with the project model the engine assembled — what `resolve`
    /// gets, and the only door to a source root, an alias or a namespace.
    pub fn with_project(
        known_files: &'a BTreeSet<ProjectPath>,
        packages: &'a std::collections::BTreeMap<SmolStr, PackageEntry>,
        project: &'a ProjectView<'a>,
    ) -> Self {
        ResolveContext {
            known_files,
            packages: Some(packages),
            manifests: None,
            project: Some(project),
        }
    }

    /// The context a manifest reader gets: every discovered manifest's content
    /// beside its own, for a build system that composes manifests — a Maven
    /// pom inherits its parent's build settings. Manifests only: a reader
    /// never reads source.
    pub fn with_manifests(
        known_files: &'a BTreeSet<ProjectPath>,
        manifests: &'a std::collections::BTreeMap<ProjectPath, &'a [u8]>,
    ) -> Self {
        ResolveContext {
            known_files,
            packages: None,
            manifests: Some(manifests),
            project: None,
        }
    }

    /// What the manifests declared, as resolution asks it — see
    /// [`ProjectView`]. `None` outside a resolve, where no project exists yet.
    pub fn project(&self) -> Option<&'a ProjectView<'a>> {
        self.project
    }

    /// The content of another discovered manifest — `None` outside a manifest
    /// read, and for a path no adapter claims as a manifest.
    pub fn manifest(&self, path: &ProjectPath) -> Option<&'a [u8]> {
        self.manifests?.get(path).copied()
    }

    pub fn contains(&self, path: &ProjectPath) -> bool {
        self.known_files.contains(path)
    }

    /// The workspace package a bare specifier names, when one of this project's own
    /// manifests declares it.
    pub fn package(&self, name: &str) -> Option<&PackageEntry> {
        self.packages?.get(name)
    }

    /// The package whose directory contains `path` — the innermost one, when
    /// manifests nest. How an adapter answers "which crate/module does the file I am
    /// resolving from belong to" (`crate::` paths, module-relative imports).
    pub fn package_of(&self, path: &ProjectPath) -> Option<&PackageEntry> {
        self.packages?
            .values()
            .filter(|p| path.is_under(&p.dir))
            .max_by_key(|p| p.dir.len())
    }

    /// Known files whose path starts with `prefix`, in path order — what a manifest's
    /// wildcard entry (`"./types/*"`) expands against.
    pub fn files_with_prefix<'p>(
        &'p self,
        prefix: &'p str,
    ) -> impl Iterator<Item = &'a ProjectPath> + 'p {
        self.known_files
            .range(ProjectPath::new(prefix)..)
            .take_while(move |p| p.as_str().starts_with(prefix))
    }

    /// Every known file, in path order — the enumeration a WASM bridge snapshots
    /// across the boundary so a guest-side context can answer the same queries from
    /// the same data. Deterministic by construction.
    pub fn known_files(&self) -> impl Iterator<Item = &'a ProjectPath> + '_ {
        self.known_files.iter()
    }

    /// Every declared package, in name order — the other half of the same snapshot.
    pub fn packages(&self) -> impl Iterator<Item = &'a PackageEntry> + '_ {
        self.packages.into_iter().flat_map(|m| m.values())
    }
}

/// What an import specifier resolved to. `Unresolved` is the keep-alive default for
/// anything the adapter cannot place.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    File(ProjectPath),
    /// One import, several files — the unit some ecosystems import is a directory
    /// (a Go package is every file in its dir). The adapter names the exact set;
    /// the engine draws one edge per file.
    Files(Vec<ProjectPath>),
    Unresolved,
}

/// An entry point a manifest declares, already resolved to a project file. The engine
/// anchors a whole-file root on it at assembly — never cached with the target file's
/// evidence, because the manifest can change while the target does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRoot {
    pub file: ProjectPath,
    pub kind: RootKind,
    pub confidence: Confidence,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{AliasTarget, PathAlias, UnitKind, UnitRoot};
    use std::collections::BTreeMap;

    /// One unit rooted at `src`, two aliases — the outer manifest's and a
    /// nested one's — and two files declaring one namespace.
    struct Fixture {
        units: Vec<UnitView>,
        unit_of: BTreeMap<ProjectPath, u32>,
        aliases: Vec<(SmolStr, PathAlias)>,
        namespaces: BTreeMap<ProjectPath, Vec<SmolStr>>,
        in_namespace: BTreeMap<Vec<SmolStr>, Vec<ProjectPath>>,
    }

    fn fixture() -> Fixture {
        let alias = |pattern: &str, targets: &[&str]| PathAlias {
            pattern: pattern.into(),
            targets: targets.iter().map(|t| AliasTarget::always(*t)).collect(),
        };
        Fixture {
            units: vec![UnitView {
                name: "app".into(),
                kind: UnitKind::Library,
                roots: vec![UnitRoot::from("src")],
                namespace_root: Some("app".into()),
                published: true,
                compiles_against: Vec::new(),
            }],
            unit_of: [(ProjectPath::new("src/a.ts"), 0)].into_iter().collect(),
            aliases: vec![
                (
                    SmolStr::default(),
                    alias("@app/", &["src/app", "src/vendor"]),
                ),
                (SmolStr::default(), alias("@app/deep/", &["src/deep"])),
                (SmolStr::new("inner"), alias("@app/", &["inner/own"])),
                (SmolStr::default(), alias("~", &["*"])),
            ],
            namespaces: [
                (ProjectPath::new("src/a.ts"), vec![SmolStr::new("com")]),
                (ProjectPath::new("src/b.ts"), vec![SmolStr::new("com")]),
            ]
            .into_iter()
            .collect(),
            in_namespace: [(
                vec![SmolStr::new("com")],
                vec![ProjectPath::new("src/a.ts"), ProjectPath::new("src/b.ts")],
            )]
            .into_iter()
            .collect(),
        }
    }

    fn view(f: &Fixture) -> ProjectView<'_> {
        ProjectView::new(
            &f.units,
            &f.unit_of,
            &f.aliases,
            &f.namespaces,
            &f.in_namespace,
        )
    }

    #[test]
    fn the_unit_answers_what_it_compiles_and_what_it_hangs_under() {
        let f = fixture();
        let view = view(&f);
        let unit = view
            .unit_of(&ProjectPath::new("src/a.ts"))
            .expect("covered");
        assert_eq!(unit.name, "app");
        assert!(unit.published);
        assert_eq!(unit.namespace_root.as_deref(), Some("app"));
        assert_eq!(
            view.source_roots_of(&ProjectPath::new("src/a.ts")),
            [UnitRoot::from("src")]
        );
        // A file no manifest covers has no unit and no roots — never a guess.
        assert!(view.unit_of(&ProjectPath::new("scripts/x.ts")).is_none());
        assert!(
            view.source_roots_of(&ProjectPath::new("scripts/x.ts"))
                .is_empty()
        );
    }

    #[test]
    fn an_alias_rewrites_the_prefix_and_the_nearest_longest_one_wins() {
        let f = fixture();
        let view = view(&f);
        let at = |from: &str, spec: &str| -> Vec<String> {
            view.alias(&ProjectPath::new(from), spec)
                .iter()
                .map(SmolStr::to_string)
                .collect()
        };
        // The prefix is replaced and the rest kept, in the order the build
        // tries the targets.
        assert_eq!(at("main.ts", "@app/lib"), ["src/app/lib", "src/vendor/lib"]);
        // The LONGEST prefix wins: `@app/deep/x` is not `@app/`'s `deep/x`.
        assert_eq!(at("main.ts", "@app/deep/x"), ["src/deep/x"]);
        // Two declarations of one prefix are two build configurations, and the
        // nearest above the resolving file is the one in force.
        assert_eq!(at("inner/caller.ts", "@app/lib"), ["inner/own/lib"]);
        assert_eq!(
            at("outer/caller.ts", "@app/lib"),
            ["src/app/lib", "src/vendor/lib"],
            "the nested declaration covers only the files under it"
        );
        // A bare `*` template rewrites to the capture alone — `~x` at the root.
        assert_eq!(at("main.ts", "~x"), ["x"]);
        // A specifier no alias names rewrites to nothing, which is silence.
        assert!(at("main.ts", "./sibling").is_empty());
    }

    #[test]
    fn a_namespace_answers_both_ways_and_absence_is_empty() {
        let f = fixture();
        let view = view(&f);
        assert_eq!(
            view.namespace_of(&ProjectPath::new("src/a.ts")),
            [SmolStr::new("com")]
        );
        assert_eq!(
            view.files_in_namespace(&ProjectPath::new("src/a.ts"), &[SmolStr::new("com")]),
            [ProjectPath::new("src/a.ts"), ProjectPath::new("src/b.ts")]
        );
        // A language whose files declare no namespace answers with none, and a
        // namespace nothing declares holds no files.
        assert!(view.namespace_of(&ProjectPath::new("src/c.ts")).is_empty());
        assert!(
            view.files_in_namespace(&ProjectPath::new("src/a.ts"), &[SmolStr::new("org")])
                .is_empty()
        );
    }
}
