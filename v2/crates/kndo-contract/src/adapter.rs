//! The adapter side of the contract: what a language teaches kndo, and how. Adapters
//! implement [`LanguageAdapter`] against this crate only — the engine is one more
//! consumer. This is the M1 slice of [`AdapterSpec`]: id, semantics version, claim
//! globs and the declared evidence streams; the ladder, cycle policy and manifest
//! capabilities arrive with the first real language (each with a default, a named core
//! consumer, and a conformance case).

use crate::evidence::{EvidenceSink, EvidenceStreams};
use crate::vocab::ProjectPath;
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
}

impl AdapterSpec {
    pub fn builder(id: &'static str, semantics_version: u32) -> AdapterSpecBuilder {
        AdapterSpecBuilder {
            spec: AdapterSpec {
                id: SmolStr::new_static(id),
                semantics_version,
                claims: Vec::new(),
                emits: EvidenceStreams::none(),
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

    pub fn build(self) -> AdapterSpec {
        self.spec
    }
}

/// The project around a file, as the engine lets an adapter see it during import
/// resolution. Grows methods only.
pub struct ResolveCtx<'a> {
    known_files: &'a BTreeSet<ProjectPath>,
}

impl<'a> ResolveCtx<'a> {
    pub fn new(known_files: &'a BTreeSet<ProjectPath>) -> Self {
        ResolveCtx { known_files }
    }

    pub fn contains(&self, path: &ProjectPath) -> bool {
        self.known_files.contains(path)
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

pub trait LanguageAdapter: Send + Sync {
    fn spec(&self) -> &AdapterSpec;

    /// Never fails: extraction degrades through the sink's diagnostics.
    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink);

    /// Resolve a relative import specifier written in `from` against the project.
    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveCtx<'_>) -> Resolution;
}
