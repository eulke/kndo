//! The subject of a finding, as one closed enum. Its kind, its location, and the
//! finding's identity are DERIVED from it — there is no parallel field to keep in
//! agreement by hand, and an impossible subject does not typecheck.

use crate::vocab::{Category, ProjectPath, Span, SubjectKind};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

/// The address of one declaration inside its file — a KEY, unique within the
/// file by construction (the evidence sink assigns `nth`), so two declarations
/// can never share a finding identity or a query address. Built in one place,
/// [`crate::evidence::FileEvidence::selector_of`]; every consumer that names a
/// symbol goes through it and never re-spells the parts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolSelector {
    /// The declaration this one is a member of, by name; `None` for a free
    /// declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<SmolStr>,
    pub name: SmolStr,
    /// What the language reads beyond the identifier to tell same-named
    /// declarations apart, as it spells it — see
    /// [`crate::evidence::Declaration::signature`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<SmolStr>,
    /// Position among the file's declarations sharing owner, name and
    /// signature, in source order — zero for the common case, and the reason
    /// two declarations a language cannot tell apart by name (a Python
    /// redefinition, an overload whose adapter states no signature) still
    /// have two addresses.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub nth: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// `#k` for the k-th of several a file holds under one spelling; nothing for
/// the first, so the common case reads and hashes as it always did.
fn with_position(mut spelling: String, nth: u32) -> String {
    if nth > 0 {
        spelling.push_str(&format!(" #{}", nth + 1));
    }
    spelling
}

impl SymbolSelector {
    /// A free declaration known to be the only one of its name — a hand-built
    /// subject in a test or a plugin. Evidence never goes through here: a
    /// declaration's selector comes from
    /// [`crate::evidence::FileEvidence::selector_of`], which is what makes it
    /// unique.
    pub fn free(name: &str) -> SymbolSelector {
        SymbolSelector {
            owner: None,
            name: SmolStr::new(name),
            signature: None,
            nth: 0,
        }
    }

    /// A member known to be the only one of its name on its owner — see
    /// [`SymbolSelector::free`].
    pub fn member(owner: &str, name: &str) -> SymbolSelector {
        SymbolSelector {
            owner: Some(SmolStr::new(owner)),
            name: SmolStr::new(name),
            signature: None,
            nth: 0,
        }
    }

    /// The one display spelling: `Owner.name`, the signature verbatim when
    /// there is one (`Owner.name(int, String)`), and `#k` for the k-th of
    /// several a language cannot tell apart (`Owner.name#2`). The inverse
    /// (parsing) deliberately does not exist — a consumer that wants to find
    /// the declaration behind a spelling compares renders.
    pub fn render(&self) -> String {
        let mut out = match &self.owner {
            Some(owner) => format!("{owner}.{}", self.name),
            None => self.name.to_string(),
        };
        if let Some(signature) = &self.signature {
            out.push_str(signature);
        }
        if self.nth > 0 {
            out.push('#');
            out.push_str(&(self.nth + 1).to_string());
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// One import statement, addressed by the specifier AS WRITTEN — identity
    /// survives the code moving (the span is carried for lines, never for
    /// identity, the same rule Symbol follows) — and, for a file that writes
    /// the same specifier twice, by its position among those.
    Import {
        path: ProjectPath,
        specifier: SmolStr,
        /// Position among the file's imports of this specifier, in source
        /// order; zero for the common case. Assigned by the evidence sink.
        #[serde(default, skip_serializing_if = "is_zero")]
        nth: u32,
        span: Span,
    },
    /// One `kndo:allow` pragma, addressed by what it allows and, for a file
    /// that writes the same allow twice, by its position among those.
    Suppression {
        path: ProjectPath,
        categories: Vec<Category>,
        #[serde(default, skip_serializing_if = "is_zero")]
        nth: u32,
        span: Span,
    },
}

impl Subject {
    /// The one display spelling of WHERE a finding points — path plus whatever
    /// the variant knows beyond it. Frontends print this instead of keeping
    /// their own (lossy) tables: a dependency finding keeps its dependency
    /// NAME, not just its manifest.
    pub fn render(&self) -> String {
        match self {
            Subject::File { path } | Subject::Directory { path } => path.as_str().to_string(),
            Subject::Package { manifest, name } => format!("{} ({name})", manifest.as_str()),
            Subject::Dependency {
                owner_manifest,
                name,
            } => format!("{} ({name})", owner_manifest.as_str()),
            Subject::Symbol { path, .. }
            | Subject::Import { path, .. }
            | Subject::Suppression { path, .. } => {
                format!("{} — {}", path.as_str(), self.label())
            }
        }
    }

    /// What this subject is WITHIN its path — the part a display puts after
    /// the path and a line: a symbol's selector, `import 'x'`, `allow unused`,
    /// each with `#k` when a file holds the same spelling more than once.
    /// Empty for a file or a directory, which are their path.
    pub fn label(&self) -> String {
        match self {
            Subject::Import { specifier, nth, .. } => {
                with_position(format!("import '{specifier}'"), *nth)
            }
            _ => self.identity_part(),
        }
    }

    /// What identity hashes beside the category, the kind and the path: the
    /// same spelling `label` shows, minus display dressing an import wears —
    /// so an identity never moves for a wording change, and two subjects of
    /// one file that read alike differ here by their position.
    pub fn identity_part(&self) -> String {
        match self {
            Subject::File { .. } | Subject::Directory { .. } => String::new(),
            Subject::Symbol { selector, .. } => selector.render(),
            Subject::Package { name, .. } | Subject::Dependency { name, .. } => name.to_string(),
            Subject::Import { specifier, nth, .. } => with_position(specifier.to_string(), *nth),
            Subject::Suppression {
                categories, nth, ..
            } => {
                let listed: Vec<&str> = categories.iter().map(Category::as_str).collect();
                with_position(format!("allow {}", listed.join(", ")), *nth)
            }
        }
    }

    pub fn kind(&self) -> SubjectKind {
        match self {
            Subject::File { .. } => SubjectKind::File,
            Subject::Symbol { .. } => SubjectKind::Symbol,
            Subject::Package { .. } => SubjectKind::Package,
            Subject::Dependency { .. } => SubjectKind::Dependency,
            Subject::Directory { .. } => SubjectKind::Directory,
            Subject::Import { .. } => SubjectKind::Import,
            Subject::Suppression { .. } => SubjectKind::Suppression,
        }
    }

    /// The path this subject anchors to (every subject has one).
    pub fn path(&self) -> &ProjectPath {
        match self {
            Subject::File { path }
            | Subject::Symbol { path, .. }
            | Subject::Directory { path }
            | Subject::Import { path, .. }
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
        part(&subject.identity_part());
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
