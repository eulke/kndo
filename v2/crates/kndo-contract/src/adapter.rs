//! The adapter side of the contract: what a language teaches kndo, and how. Adapters
//! implement [`LanguageAdapter`] against this crate only — the engine is one more
//! consumer. [`AdapterSpec`] carries id, semantics version, claim globs, the declared
//! evidence streams, and the manifest globs behind [`LanguageAdapter::roots`]; the
//! visibility ladder and cycle policy arrive with the languages that need them (each
//! with a default, a named core consumer, and a conformance case).

use crate::evidence::{EvidenceSink, EvidenceStreams, RootKind};
use crate::vocab::{Confidence, ProjectPath};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::collections::BTreeSet;

/// One file handed to extraction. Adapters never touch the filesystem.
pub struct SourceFile<'a> {
    pub path: &'a ProjectPath,
    pub content: &'a [u8],
}

/// What an adapter IS, as data. Built once, returned by reference, and folded into
/// every evidence cache key (`id`, `semantics_version`, `emits`) so a behavior change
/// invalidates exactly what it changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterSpec {
    id: SmolStr,
    semantics_version: u32,
    claims: Vec<SmolStr>,
    emits: EvidenceStreams,
    #[serde(default)]
    manifests: Vec<SmolStr>,
    #[serde(default)]
    extensions: Vec<SmolStr>,
}

impl AdapterSpec {
    /// The bridge-side constructor: a LOADED component's spec arrives as data, not
    /// statics, so the builder's `&'static str` economy cannot apply. Everything is
    /// taken verbatim — in particular `claims` is NOT derived from `extensions`
    /// here, because the guest-side builder already derived it and the wire carries
    /// the finished list. Native adapters use [`AdapterSpec::builder`].
    #[allow(clippy::too_many_arguments)]
    pub fn assemble(
        id: impl Into<SmolStr>,
        semantics_version: u32,
        claims: Vec<SmolStr>,
        emits: EvidenceStreams,
        manifests: Vec<SmolStr>,
        extensions: Vec<SmolStr>,
    ) -> AdapterSpec {
        AdapterSpec {
            id: id.into(),
            semantics_version,
            claims,
            emits,
            manifests,
            extensions,
        }
    }

    pub fn builder(id: &'static str, semantics_version: u32) -> AdapterSpecBuilder {
        AdapterSpecBuilder {
            spec: AdapterSpec {
                id: SmolStr::new_static(id),
                semantics_version,
                claims: Vec::new(),
                emits: EvidenceStreams::none(),
                manifests: Vec::new(),
                extensions: Vec::new(),
            },
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn semantics_version(&self) -> u32 {
        self.semantics_version
    }

    /// The claim globs — the one source of what this adapter owns; the engine matches
    /// against these, so a glob listed here is never merely descriptive.
    pub fn claims(&self) -> &[SmolStr] {
        &self.claims
    }

    pub fn emits(&self) -> &EvidenceStreams {
        &self.emits
    }

    /// Globs naming the manifest files this adapter can read roots from. Manifests are
    /// consulted, never claimed: the engine hands each match to
    /// [`LanguageAdapter::roots`] and anchors what comes back.
    pub fn manifests(&self) -> &[SmolStr] {
        &self.manifests
    }

    /// The file extensions this adapter speaks (no leading dot), in
    /// resolution-candidate priority order — THE one declaration: the claim globs
    /// derive from it at build time, and the adapter's own resolution, script
    /// detection, and any other extension-conditional logic read this same list
    /// instead of keeping copies.
    pub fn extensions(&self) -> &[SmolStr] {
        &self.extensions
    }
}

pub struct AdapterSpecBuilder {
    spec: AdapterSpec,
}

impl AdapterSpecBuilder {
    /// Declare the extensions this adapter speaks (no leading dot), in
    /// resolution-candidate priority order. Each derives a `**/*.<ext>` claim glob —
    /// declaring extensions IS claiming them; `claims` stays for patterns that are
    /// not extension-shaped.
    pub fn extensions(mut self, extensions: &[&'static str]) -> Self {
        for ext in extensions {
            self.spec.extensions.push(SmolStr::new_static(ext));
            self.spec.claims.push(SmolStr::from(format!("**/*.{ext}")));
        }
        self
    }

    pub fn claims(mut self, globs: &[&'static str]) -> Self {
        self.spec
            .claims
            .extend(globs.iter().map(|g| SmolStr::new_static(g)));
        self
    }

    /// Omitted ⇒ `EvidenceStreams::none()` — the default-compatibility rule.
    pub fn emits(mut self, streams: EvidenceStreams) -> Self {
        self.spec.emits = streams;
        self
    }

    /// Omitted ⇒ no manifests consulted and [`LanguageAdapter::roots`] never called —
    /// the default-compatibility rule.
    pub fn manifests(mut self, globs: &[&'static str]) -> Self {
        self.spec.manifests = globs.iter().map(|g| SmolStr::new_static(g)).collect();
        self
    }

    pub fn build(self) -> AdapterSpec {
        self.spec
    }
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
}

/// The project around a file, as the engine lets an adapter see it during import
/// resolution. Grows methods only.
pub struct ResolveContext<'a> {
    known_files: &'a BTreeSet<ProjectPath>,
    packages: Option<&'a std::collections::BTreeMap<SmolStr, PackageEntry>>,
}

impl<'a> ResolveContext<'a> {
    pub fn new(known_files: &'a BTreeSet<ProjectPath>) -> Self {
        ResolveContext {
            known_files,
            packages: None,
        }
    }

    pub fn with_packages(
        known_files: &'a BTreeSet<ProjectPath>,
        packages: &'a std::collections::BTreeMap<SmolStr, PackageEntry>,
    ) -> Self {
        ResolveContext {
            known_files,
            packages: Some(packages),
        }
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
            .filter(|p| {
                p.dir.is_empty() || {
                    path.as_str()
                        .strip_prefix(p.dir.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
                }
            })
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

pub trait LanguageAdapter: Send + Sync {
    fn spec(&self) -> &AdapterSpec;

    /// Never fails: extraction degrades through the sink's diagnostics.
    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink);

    /// Resolve a relative import specifier written in `from` against the project.
    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution;

    /// The roots one manifest declares. Called for every discovered file matching
    /// [`AdapterSpec::manifests`]; the default — no manifests, no roots — reproduces
    /// pre-capability behavior. Unparseable or dangling entries degrade to absence:
    /// a root that anchors nothing accuses nothing.
    fn roots(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<ProjectRoot> {
        let _ = (manifest, cx);
        Vec::new()
    }

    /// The packages one manifest declares, fed back to every adapter's `resolve`
    /// through [`ResolveContext::package`]. Same default and degradations as `roots`.
    fn packages(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<PackageEntry> {
        let _ = (manifest, cx);
        Vec::new()
    }

    /// The dependency NAMES one manifest declares, every section alike — what
    /// plugin activation (`ManifestDependency`) evaluates against, through the same
    /// discovered-manifest pipeline `roots` and `packages` already ride; no second
    /// manifest walk exists. The default — no names — keeps such rules unmatched,
    /// so a plugin stays off rather than guessing on. Same degradations as `roots`.
    fn manifest_dependencies(&self, manifest: &SourceFile<'_>) -> Vec<SmolStr> {
        let _ = manifest;
        Vec::new()
    }

    /// The files whose names `path` can see WITHOUT an import — the rest of its
    /// compilation unit, in the languages whose unit is bigger than the file (every
    /// non-test sibling of a Go file's package; a test file sees the whole package).
    /// The engine draws one reachability edge per mate and pools references over
    /// the visibility this declares, so a mate's use keeps a declaration no import
    /// ever names. Depends only on `path` and the file SET, never on content —
    /// which is what lets a persisted graph trust it while only contents change.
    /// The default — no mates, names scoped to the file — reproduces pre-capability
    /// behavior.
    fn unit_mates(&self, path: &ProjectPath, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
        let _ = (path, cx);
        Vec::new()
    }
}
