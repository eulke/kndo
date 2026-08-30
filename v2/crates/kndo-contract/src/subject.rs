//! The subject of a finding, as one closed enum. Its kind, its location, and the
//! finding's identity are DERIVED from it — there is no parallel field to keep in
//! agreement by hand, and an impossible subject does not typecheck.

use crate::vocab::{Category, ProjectPath, Span, SubjectKind};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

/// Names a symbol within a file. The one place the member-of relation is spelled;
/// rendering (`Owner.name`) is the output edge's job, never a parsing convention.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolSelector {
    Free(SmolStr),
    Member { owner: SmolStr, name: SmolStr },
}

impl SymbolSelector {
    /// The display spelling; the inverse (parsing) deliberately does not exist.
    pub fn render(&self) -> String {
        match self {
            SymbolSelector::Free(name) => name.to_string(),
            SymbolSelector::Member { owner, name } => format!("{owner}.{name}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Subject {
    File {
        path: ProjectPath,
    },
    Symbol {
        path: ProjectPath,
        selector: SymbolSelector,
        span: Span,
    },
    Package {
        manifest: ProjectPath,
        name: SmolStr,
    },
    Dependency {
        owner_manifest: ProjectPath,
        name: SmolStr,
    },
    Directory {
        path: ProjectPath,
    },
    Suppression {
        path: ProjectPath,
        span: Span,
    },
}

impl Subject {
    pub fn kind(&self) -> SubjectKind {
        match self {
            Subject::File { .. } => SubjectKind::File,
            Subject::Symbol { .. } => SubjectKind::Symbol,
            Subject::Package { .. } => SubjectKind::Package,
            Subject::Dependency { .. } => SubjectKind::Dependency,
            Subject::Directory { .. } => SubjectKind::Directory,
            Subject::Suppression { .. } => SubjectKind::Suppression,
        }
    }

    /// The path this subject anchors to (every subject has one).
    pub fn path(&self) -> &ProjectPath {
        match self {
            Subject::File { path }
            | Subject::Symbol { path, .. }
            | Subject::Directory { path }
            | Subject::Suppression { path, .. } => path,
            Subject::Package { manifest, .. } => manifest,
            Subject::Dependency { owner_manifest, .. } => owner_manifest,
        }
    }
}

/// A finding's stable identity. Baselines and suppressions depend on it, so it is a
/// type, derived from typed parts — never five adjacent strings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FindingId(SmolStr);

impl FindingId {
    /// Identity = category + subject (kind, path, selector) + a per-analysis
    /// discriminator. Spans deliberately do not participate: moving code must not
    /// change identity.
    pub fn derive(category: &Category, subject: &Subject, discriminator: &str) -> Self {
        let mut h = blake3::Hasher::new();
        let mut part = |s: &str| {
            h.update(&(s.len() as u32).to_le_bytes());
            h.update(s.as_bytes());
        };
        part(category.as_str());
        part(subject.kind().as_str());
        part(subject.path().as_str());
        let symbol = match subject {
            Subject::Symbol { selector, .. } => selector.render(),
            Subject::Package { name, .. } | Subject::Dependency { name, .. } => name.to_string(),
            _ => String::new(),
        };
        part(&symbol);
        part(discriminator);
        let hex: String = h.finalize().as_bytes()[..6]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        FindingId(SmolStr::from(format!("kndo-{hex}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
