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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub name: SmolStr,
    pub kind: UnitKind,
    /// Directories whose files this unit compiles, `/`-separated, without a
    /// trailing slash; empty means the manifest's own directory. A file
    /// belongs to the unit whose root is the LONGEST prefix of its path, so
    /// nested units (a workspace member inside a workspace) need no ordering
    /// rule beyond that.
    pub roots: Vec<SmolStr>,
    /// Paths inside those roots this unit does NOT compile — a manifest's
    /// `exclude`, a source set's filter. Same spelling as `roots`.
    pub excludes: Vec<SmolStr>,
    /// Files the build enters this unit through, already resolved. Each
    /// anchors a root of the unit's color.
    pub entries: Vec<ProjectPath>,
    /// The units this one compiles against, NAMED as the manifest spells them
    /// — the engine resolves each to a unit of this project (or to nothing,
    /// for an external artifact). A name alone is deliberate: the manifest
    /// declaring a dependency has not read the manifest declaring the unit,
    /// so it cannot spell a path it never saw.
    pub depends_on: Vec<SmolStr>,
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
    /// Files this manifest declares as run without being any unit's entry: a
    /// CI workflow's step, an action's `main`. A unit's own entries are on the
    /// unit, where their color comes from its kind.
    pub roots: Vec<ProjectRoot>,
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
        });
        sink.unit(Unit {
            name: "core".into(),
            kind: UnitKind::Library,
            roots: vec!["src".into()],
            excludes: Vec::new(),
            entries: vec![ProjectPath::new("src/lib.rs")],
            depends_on: Vec::new(),
        });
        let ev = sink.finish();
        assert_eq!(ev.units.len(), 1);
        assert_eq!(ev.units[0].name, "core");
        assert_eq!(ev.diagnostics.len(), 1);
    }
}
