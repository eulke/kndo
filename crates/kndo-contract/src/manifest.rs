//! What an extension reports about one MANIFEST: the project's own structure,
//! stated by the file that states it — never re-derived from a path convention.
//!
//! [`crate::evidence::FileEvidence`] is what one source file says; this is what
//! one manifest says. The pair is deliberate: an adapter that reads
//! `Package.swift` knows a target's `path:` because SwiftPM's manifest carries
//! it, and every convention an adapter would otherwise guess (source-set
//! layouts, `sys.path`, a Cargo target's auto-discovery) is a sentence in a
//! file some adapter can parse. Written through [`ManifestSink`] for the same
//! reason evidence is: validation at the call site, one constructor, nothing
//! to mis-assemble.
//!
//! The growth rules of [`crate::evidence`] apply here too — additive surface,
//! growable enums `#[non_exhaustive]`, every absence degrading toward
//! keep-alive — with one difference: manifest evidence is not cached by file
//! content, so it moves the graph semantics version rather than the contract
//! fingerprint.

use crate::adapter::{DependencyDeclaration, PackageEntry, ProjectRoot};
use crate::evidence::{AdapterDiagnostic, DiagnosticLevel, RootKind};
use crate::vocab::ProjectPath;
use smol_str::SmolStr;

/// What the build system makes of a unit — the fact that decides which color
/// its entries anchor and, later, how its names may reach. Grows if a build
/// system teaches us a kind; an unknown kind is tooling — reached, never a
/// production promise.
#[non_exhaustive]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum UnitKind {
    /// Compiled for others to import: a library crate, a Maven module's main
    /// source set, an npm package's published entry.
    Library,
    /// Compiled to run: a binary, an application target.
    Executable,
    Test,
    Bench,
    Example,
    /// Built to build something else: a Cargo build script, a codegen target.
    Tooling,
}

impl UnitKind {
    /// The color this unit's entries anchor. A library's and an executable's
    /// entries are production; a test's and a bench's are test; everything
    /// else — examples, tooling, a kind this build predates — is tooling.
    pub fn color(self) -> RootKind {
        match self {
            UnitKind::Library | UnitKind::Executable => RootKind::Production,
            UnitKind::Test | UnitKind::Bench => RootKind::Test,
            _ => RootKind::Tooling,
        }
    }
}

/// One thing the build system compiles as one artifact: a Cargo target, a
/// SwiftPM target, a Gradle module's source set, an npm workspace package, a
/// Go module. Identity is (declaring manifest, name) — parallel trees
/// legitimately give two units one name.
/// A version as an ecosystem's ordering compares it — three numbers and
/// nothing else. Pre-release and build metadata are deliberately absent: what
/// this type answers is "is this range below that one", and every ecosystem
/// answers that on the numeric triple.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub fn new(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    /// `1`, `1.2`, `1.2.3`, `1.2.3-rc1+meta` — the numbers, with what an
    /// ecosystem appends ignored. `None` where the text is not a version.
    pub fn parse(text: &str) -> Option<Version> {
        let core = text
            .trim()
            .split(['-', '+'])
            .next()
            .unwrap_or_default()
            .trim();
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let number = |p: Option<&str>| p.map_or(Some(0), |t| t.parse().ok());
        Some(Version {
            major,
            minor: number(parts.next())?,
            patch: number(parts.next())?,
        })
    }
}

/// What a manifest requires of a dependency: the text as written, and — where
/// the declaring adapter could normalize it — the half-open range `[lo, hi)`
/// its ecosystem reads that text as. `range: None` is the honest answer for a
/// requirement this vocabulary cannot compare (a path or git coordinate, a
/// BOM-managed version, a form the adapter does not model), and a comparison
/// that could not be performed stays silent rather than inventing an ordering.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct VersionReq {
    pub spelled: SmolStr,
    pub range: Option<(Version, Version)>,
}

impl VersionReq {
    /// The requirement as written, with no range derived — every ecosystem's
    /// fallback and the only honest reading of a form the adapter cannot map.
    pub fn spelled(text: impl Into<SmolStr>) -> VersionReq {
        VersionReq {
            spelled: text.into(),
            range: None,
        }
    }

    /// Do two requirements name ranges that cannot both hold? `None` wherever
    /// either side states no range: unknown is never a conflict.
    pub fn disjoint(&self, other: &VersionReq) -> Option<bool> {
        let (a, b) = (self.range.as_ref()?, other.range.as_ref()?);
        Some(a.1 <= b.0 || b.1 <= a.0)
    }
}

/// A specifier the build system rewrites: `tsconfig` paths and `baseUrl`, an
/// npm `exports`/`imports` map, Sass `loadPaths`, a go.mod `replace`, a page's
/// importmap. Consulted by `resolve` through the project context.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct PathAlias {
    /// The specifier this rewrites, as the manifest writes it. At most one
    /// `*`, and it CAPTURES: `#types/*`, `@app/*`, `./dist/client/*`. A
    /// pattern with no `*` is a bare prefix and whatever follows it is the
    /// capture — which is how `@app/` → `src/app` keeps working, and why
    /// `~` alone still rewrites `~/x`.
    pub pattern: SmolStr,
    /// What it rewrites to, in the order the build tries them.
    pub targets: Vec<AliasTarget>,
}

/// One rewriting a [`PathAlias`] offers, and the conditions it holds under.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct AliasTarget {
    /// Where the pattern rewrites to, as a PROJECT path — the declaring
    /// adapter joins its manifest's directory, because only it knows whether
    /// the table's paths were written relative to the manifest, to a source
    /// root, or to the project. A `*` receives whatever the pattern
    /// captured — `#types/*` →
    /// `./types/*.d.ts` sends `#types/hot` to `types/hot.d.ts`; a template
    /// with no `*` takes the capture appended, which is the directory form
    /// (`@app/` → `src/app` answers `src/app/x`).
    ///
    /// EMPTY is a deliberate dead end, not an omission: npm's `null` target
    /// says a subpath is NOT exported, and "the manifest refused this
    /// specifier" is a different fact from "no alias named it" — the first
    /// stops the search, the second falls through.
    pub template: SmolStr,
    /// Every condition that must hold for this target, in the manifest's own
    /// spelling (`import`, `require`, `types`, `node`, `browser`). Empty
    /// applies always — npm's `default`, and every alias table that has no
    /// conditions at all.
    ///
    /// The engine does NOT pick a runtime: it cannot know one, so every
    /// condition's target is a possible resolution and all of them are
    /// offered, in declaration order. What the conditions buy is that the
    /// caller can SEE which branch it took — a target reached only under
    /// `types` resolves to a declaration file the runtime never loads.
    pub conditions: Vec<SmolStr>,
}

impl PathAlias {
    /// What this alias rewrites `specifier` to, in the order the build tries
    /// them — empty where the pattern does not name it. Each pair is the
    /// target as declared (its conditions readable) and the path it rewrites
    /// to, which is EMPTY for a refusing target.
    ///
    /// The one implementation: a manifest's own alias table and a package's
    /// subpath map are the same matching, and a second copy of it would be a
    /// second answer.
    pub fn rewrite(&self, specifier: &str) -> Vec<(&AliasTarget, SmolStr)> {
        let Some(captured) = self.capture(specifier) else {
            return Vec::new();
        };
        self.targets
            .iter()
            .map(|target| (target, fill(&target.template, captured)))
            .collect()
    }

    /// What the pattern captures from `specifier`, or `None` where it does not
    /// match. A pattern with a `*` captures what stands there; one without is
    /// a prefix, and the capture is everything past it — the directory form
    /// every alias table wrote before conditional subpaths existed.
    pub fn capture<'s>(&self, specifier: &'s str) -> Option<&'s str> {
        match self.pattern.split_once('*') {
            Some((head, tail)) => specifier
                .strip_prefix(head)?
                .strip_suffix(tail)
                .filter(|_| specifier.len() >= head.len() + tail.len()),
            None => specifier.strip_prefix(self.pattern.as_str()),
        }
    }
}

/// The template with its `*` filled by the capture — or, where it has none,
/// the capture joined onto it as a directory. An empty template stays empty:
/// a refusal rewrites to nothing.
fn fill(template: &str, captured: &str) -> SmolStr {
    if template.is_empty() {
        return SmolStr::default();
    }
    match template.split_once('*') {
        Some((head, tail)) => SmolStr::new(format!("{head}{captured}{tail}")),
        None if captured.is_empty() => SmolStr::new(template),
        None => SmolStr::new(format!(
            "{}/{}",
            template.trim_end_matches('/'),
            captured.trim_start_matches('/')
        )),
    }
}

impl AliasTarget {
    /// The unconditional form — what every alias table without conditions
    /// writes.
    pub fn always(template: impl Into<SmolStr>) -> AliasTarget {
        AliasTarget {
            template: template.into(),
            conditions: Vec::new(),
        }
    }

    /// A dead end: the manifest names this specifier and refuses it.
    pub fn refused() -> AliasTarget {
        AliasTarget::always("")
    }

    /// Whether this target resolves to anything at all — see `template`.
    pub fn refuses(&self) -> bool {
        self.template.is_empty()
    }
}

/// One directory a unit compiles, and whether the unit takes what is nested
/// under it. A non-recursive root is a real shape — a target that compiles the
/// files of one directory and leaves its subdirectories to another unit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct UnitRoot {
    /// `/`-separated, without a trailing slash; empty is the manifest's own
    /// directory.
    pub path: SmolStr,
    pub recursive: bool,
}

impl<T: Into<SmolStr>> From<T> for UnitRoot {
    /// A bare path is RECURSIVE — every build system's default, and what a
    /// manifest means when it names a source directory and stops.
    fn from(path: T) -> UnitRoot {
        UnitRoot {
            path: path.into(),
            recursive: true,
        }
    }
}

/// One unit this unit compiles against, and whether it also sees that unit's
/// unit-reaching names. One type rather than two lists, because a dependency
/// and a friendship are one fact the manifest states once and they cannot be
/// allowed to disagree.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct UnitDep {
    /// The unit as the manifest SPELLS it — the engine resolves it to a unit of
    /// this project, or to nothing for an external artifact. A name alone is
    /// deliberate: the manifest declaring a dependency has not read the
    /// manifest declaring the unit, so it cannot spell a path it never saw.
    pub unit: SmolStr,
    /// The dependent may use the target's unit-reaching names: a Kotlin test
    /// source set over its main, a Swift test target that `@testable import`s.
    /// The build system's statement and never inferred — a Rust integration
    /// test is not a friend of the library it tests, so Cargo never says it.
    pub friend: bool,
}

impl UnitDep {
    pub fn on(unit: impl Into<SmolStr>) -> UnitDep {
        UnitDep {
            unit: unit.into(),
            friend: false,
        }
    }

    pub fn friend(unit: impl Into<SmolStr>) -> UnitDep {
        UnitDep {
            unit: unit.into(),
            friend: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub name: SmolStr,
    pub kind: UnitKind,
    /// Directories whose files this unit compiles — see [`UnitRoot`]. A file
    /// belongs to the unit whose root is the LONGEST prefix of its path, so
    /// nested units (a workspace member inside a workspace) need no ordering
    /// rule beyond that.
    pub roots: Vec<UnitRoot>,
    /// Paths inside those roots this unit does NOT compile — a manifest's
    /// `exclude`, a source set's filter. Same spelling as `roots`.
    pub excludes: Vec<SmolStr>,
    /// Files the build enters this unit through, already resolved. Each
    /// anchors a root of the unit's color.
    pub entries: Vec<ProjectPath>,
    /// The units this one compiles against, each with whether it is also a
    /// FRIEND — see [`UnitDep`].
    pub depends_on: Vec<UnitDep>,
    /// Whether this unit's exported API is consumed outside the project — see
    /// [`Publication`]; [`Unit::is_published`] is the one reading of it.
    pub publication: Publication,
    /// The name this unit's namespaces hang under, where the ecosystem has one
    /// — a Python distribution's root package, a Rust crate's name. `None`
    /// where the unit's files carry their own namespace clause.
    pub namespace_root: Option<SmolStr>,
}

/// What a manifest says about a unit's consumers outside the project.
/// Declared where the build system has a word for it (`publish = false`,
/// `private: true`); `Unstated` where it is silent, under which a library is
/// published and everything else is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Publication {
    Published,
    Unpublished,
    #[default]
    Unstated,
}

impl Unit {
    /// Does the outside world consume this unit's exported API? Only a
    /// library hands one out: an executable's or a test's exports are its
    /// own, whatever a registry says about the artifact. A library is
    /// published unless the manifest says otherwise.
    pub fn is_published(&self) -> bool {
        self.kind == UnitKind::Library && self.publication != Publication::Unpublished
    }
}

/// The finished evidence for one manifest. Built through [`ManifestSink`].
#[derive(Debug, Clone, Default)]
pub struct ManifestEvidence {
    pub units: Vec<Unit>,
    pub packages: Vec<PackageEntry>,
    pub dependencies: Vec<DependencyDeclaration>,
    /// Names this manifest spells outside its dependency declarations — a
    /// `scripts` entry invoking a binary, a tool config listing a plugin — so
    /// a dependency it names is in use with no import, and a package it names
    /// is not undeclared.
    pub mentions: Vec<SmolStr>,
    /// The specifier prefixes this manifest rewrites to directories — see
    /// [`PathAlias`]. Read by resolution through the project context, so an
    /// adapter never re-parses another ecosystem's alias table.
    pub aliases: Vec<PathAlias>,
    /// Files this manifest declares as run without being any unit's entry: a
    /// CI workflow's step, an action's `main`. A unit's own entries are on the
    /// unit, where their color comes from its kind.
    pub roots: Vec<ProjectRoot>,
    /// Paths THIS manifest excludes from the project: an `.eslintignore`
    /// entry, a `tool.setuptools` exclude, a `.gitignore`-shaped list an
    /// adapter reads. Language-wide ignores are spec data — this is what one
    /// project says about itself, and a file it names stays discovered (so a
    /// dependency it imports is still in use) and is never claimed.
    pub ignores: Vec<SmolStr>,
    /// The manifests this one aggregates: Maven's `<modules>`, Cargo's
    /// `workspace.members`, Gradle's `include`. It is what makes a unit
    /// dependency resolvable when two units share a name — guava declares
    /// `guava` twice, once per reactor, and a sibling's dependency on it means
    /// the one its OWN aggregator lists.
    pub members: Vec<ProjectPath>,
    pub diagnostics: Vec<AdapterDiagnostic>,
}

/// The write side of manifest extraction: the one constructor, validating as
/// evidence arrives. Degrades like [`crate::evidence::EvidenceSink`] — a
/// malformed write is dropped and described, never a failed run.
#[derive(Default)]
pub struct ManifestSink {
    out: ManifestEvidence,
}

impl ManifestSink {
    pub fn new() -> Self {
        ManifestSink::default()
    }

    /// A unit, with the directories it compiles and the files it is entered
    /// through. A unit with no roots compiles the manifest's own directory —
    /// stated by the engine, which knows where the manifest is, so an adapter
    /// never spells a path it did not read.
    pub fn unit(&mut self, unit: Unit) {
        if unit.name.is_empty() {
            self.diagnostic(
                DiagnosticLevel::Warn,
                "unit with an empty name dropped (adapter defect)",
            );
            return;
        }
        self.out.units.push(unit);
    }

    pub fn package(&mut self, package: PackageEntry) {
        self.out.packages.push(package);
    }

    pub fn dependency(&mut self, declaration: DependencyDeclaration) {
        self.out.dependencies.push(declaration);
    }

    pub fn mention(&mut self, name: impl Into<SmolStr>) {
        self.out.mentions.push(name.into());
    }

    /// A manifest this one aggregates — see [`ManifestEvidence::members`].
    pub fn member(&mut self, manifest: ProjectPath) {
        self.out.members.push(manifest);
    }

    /// A file this manifest runs — see [`ManifestEvidence::roots`].
    pub fn root(&mut self, root: ProjectRoot) {
        self.out.roots.push(root);
    }

    /// A specifier this manifest rewrites — see [`PathAlias`]. An alias
    /// with no target rewrites to nothing, so it is dropped: resolution would
    /// read it as "this prefix resolves", and the honest answer is silence.
    pub fn alias(&mut self, alias: PathAlias) {
        if alias.pattern.is_empty() || alias.targets.is_empty() {
            self.diagnostic(
                DiagnosticLevel::Warn,
                "path alias with no pattern or no target dropped (adapter defect)",
            );
            return;
        }
        self.out.aliases.push(alias);
    }

    /// A path this manifest excludes — see [`ManifestEvidence::ignores`].
    pub fn ignore(&mut self, path: impl Into<SmolStr>) {
        self.out.ignores.push(path.into());
    }

    pub fn diagnostic(&mut self, level: DiagnosticLevel, message: impl Into<String>) {
        self.out.diagnostics.push(AdapterDiagnostic {
            level,
            message: message.into(),
            span: None,
        });
    }

    pub fn finish(self) -> ManifestEvidence {
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_decides_the_color_its_entries_anchor() {
        assert_eq!(UnitKind::Library.color(), RootKind::Production);
        assert_eq!(UnitKind::Executable.color(), RootKind::Production);
        assert_eq!(UnitKind::Test.color(), RootKind::Test);
        assert_eq!(UnitKind::Bench.color(), RootKind::Test);
        assert_eq!(UnitKind::Example.color(), RootKind::Tooling);
        assert_eq!(UnitKind::Tooling.color(), RootKind::Tooling);
    }

    #[test]
    fn the_sink_drops_a_nameless_unit_and_says_so() {
        let mut sink = ManifestSink::new();
        sink.unit(Unit {
            name: SmolStr::default(),
            kind: UnitKind::Library,
            roots: Vec::new(),
            excludes: Vec::new(),
            entries: Vec::new(),
            depends_on: Vec::new(),
            publication: Publication::Unstated,
            namespace_root: None,
        });
        sink.unit(Unit {
            name: "core".into(),
            kind: UnitKind::Library,
            roots: vec![UnitRoot::from("src")],
            excludes: Vec::new(),
            entries: vec![ProjectPath::new("src/lib.rs")],
            depends_on: Vec::new(),
            publication: Publication::Unstated,
            namespace_root: None,
        });
        let ev = sink.finish();
        assert_eq!(ev.units.len(), 1);
        assert_eq!(ev.units[0].name, "core");
        assert_eq!(ev.diagnostics.len(), 1);
    }

    #[test]
    fn only_a_library_is_published_and_only_unless_the_manifest_says_otherwise() {
        let unit = |kind: UnitKind, publication: Publication| Unit {
            name: "u".into(),
            kind,
            roots: Vec::new(),
            excludes: Vec::new(),
            entries: Vec::new(),
            depends_on: Vec::new(),
            publication,
            namespace_root: None,
        };
        assert!(unit(UnitKind::Library, Publication::Unstated).is_published());
        assert!(unit(UnitKind::Library, Publication::Published).is_published());
        assert!(!unit(UnitKind::Library, Publication::Unpublished).is_published());
        assert!(!unit(UnitKind::Executable, Publication::Unstated).is_published());
        assert!(
            !unit(UnitKind::Executable, Publication::Published).is_published(),
            "a published binary hands out no API"
        );
        assert!(!unit(UnitKind::Test, Publication::Unstated).is_published());
    }
}
