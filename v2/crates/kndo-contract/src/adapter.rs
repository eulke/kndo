//! The extraction-side shared types: the file handed to extraction, the project
//! context an extension resolves against, and what resolution/manifest reads return.
//! [`crate::extension::Extension`] is the one trait that consumes them; this module
//! holds the data shapes it shares with the engine.

use crate::evidence::RootKind;
use crate::vocab::{Confidence, ProjectPath};
use smol_str::SmolStr;
use std::collections::BTreeSet;

/// One file handed to extraction. Adapters never touch the filesystem.
pub struct SourceFile<'a> {
    pub path: &'a ProjectPath,
    pub content: &'a [u8],
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
    pub version_req: Option<SmolStr>,
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
