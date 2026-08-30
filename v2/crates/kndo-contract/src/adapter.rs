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
}

impl AdapterSpec {
    pub fn builder(id: &'static str, semantics_version: u32) -> AdapterSpecBuilder {
        AdapterSpecBuilder {
            spec: AdapterSpec {
                id: SmolStr::new_static(id),
                semantics_version,
                claims: Vec::new(),
                emits: EvidenceStreams::none(),
                manifests: Vec::new(),
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
}

pub struct AdapterSpecBuilder {
    spec: AdapterSpec,
}

impl AdapterSpecBuilder {
    pub fn claims(mut self, globs: &[&'static str]) -> Self {
        self.spec.claims = globs.iter().map(|g| SmolStr::new_static(g)).collect();
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
/// that bare specifier resolves to, and the directory subpaths resolve against —
/// how workspace-internal imports link without leaving the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageEntry {
    pub name: SmolStr,
    pub entry: ProjectPath,
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
}

/// What an import specifier resolved to. `Unresolved` is the keep-alive default for
/// anything the adapter cannot place.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    File(ProjectPath),
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
}
