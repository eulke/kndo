//! The language-neutral vocabulary: paths, spans, confidence, and the open string
//! taxonomies (validated newtypes, because the `plugin:` namespace keeps them open).

use crate::fingerprint::ContractFingerprint;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

/// A `/`-separated, project-relative path. The one path spelling that crosses crate
/// boundaries; OS paths stay at the discovery edge.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ContractFingerprint,
)]
#[serde(transparent)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ProjectPath(SmolStr);

impl ProjectPath {
    pub fn new(path: impl Into<SmolStr>) -> Self {
        let s: SmolStr = path.into();
        debug_assert!(
            !s.contains('\\'),
            "ProjectPath is /-separated; normalize at the discovery edge"
        );
        ProjectPath(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A byte range within one file: `start..end`, end-exclusive. Bytes, not line/column —
/// tree-sitter yields them for free, overlap and containment are arithmetic, and a
/// central line index renders line/column only at the output edge.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    ContractFingerprint,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        debug_assert!(start <= end, "span start must not exceed end");
        Span { start, end }
    }

    pub fn contains(&self, other: &Span) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    pub fn overlaps(&self, other: &Span) -> bool {
        self.start < other.end && other.start < self.end
    }
}

/// Ordered by strength, so `max()` and threshold comparisons read naturally.
/// Closed by design: a fourth tier would change what every existing threshold means —
/// that is a semantic contract change, not growth.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    ContractFingerprint,
)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Confidence {
    Possible,
    Probable,
    Certain,
}

/// A finding category: a validated string newtype, not an enum, because the
/// `ext:<coordinate>/<rule>` namespace is open. First-party categories are the
/// associated constants.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Category(SmolStr);

impl Category {
    pub const CRAP: Category = Category(SmolStr::new_static("crap"));
    pub const CYCLIC: Category = Category(SmolStr::new_static("cyclic"));
    pub const DEEP_IMPORT: Category = Category(SmolStr::new_static("deep-import"));
    pub const DUPLICATE: Category = Category(SmolStr::new_static("duplicate"));
    pub const INTERNAL_ONLY: Category = Category(SmolStr::new_static("internal-only"));
    pub const PRIVATE_TYPE_LEAK: Category = Category(SmolStr::new_static("private-type-leak"));
    pub const STALE: Category = Category(SmolStr::new_static("stale"));
    pub const TEST_ONLY: Category = Category(SmolStr::new_static("test-only"));
    pub const UNDECLARED: Category = Category(SmolStr::new_static("undeclared"));
    pub const UNRESOLVED: Category = Category(SmolStr::new_static("unresolved"));
    pub const UNTESTED: Category = Category(SmolStr::new_static("untested"));
    pub const UNUSED: Category = Category(SmolStr::new_static("unused"));
    pub const VERSION_SKEW: Category = Category(SmolStr::new_static("version-skew"));

    /// Every first-party category — the ONE list [`Category::parse`] validates
    /// against; a new constant that misses this slice is unparseable, which its
    /// test catches.
    pub const FIRST_PARTY: &'static [Category] = &[
        Category::CRAP,
        Category::CYCLIC,
        Category::DEEP_IMPORT,
        Category::DUPLICATE,
        Category::INTERNAL_ONLY,
        Category::PRIVATE_TYPE_LEAK,
        Category::STALE,
        Category::TEST_ONLY,
        Category::UNDECLARED,
        Category::UNRESOLVED,
        Category::UNTESTED,
        Category::UNUSED,
        Category::VERSION_SKEW,
    ];

    /// The namespaced category of a plugin rule. The rule name must not contain
    /// `/`: coordinates legally do (`github.com/acme/x`), so a slash in the rule
    /// would make two different (coordinate, rule) pairs spell one category — an
    /// identity collision. Rule names are validated at declaration; this guards
    /// the invariant at the join.
    pub fn extension(coordinate: &str, rule: &str) -> Self {
        debug_assert!(
            !rule.contains('/'),
            "rule names must not contain '/' (validated at declaration)"
        );
        Category(SmolStr::from(format!("ext:{coordinate}/{rule}")))
    }

    /// The validating way in from user-written text (a suppression pragma, a config
    /// value): a first-party name or a well-formed `ext:<coordinate>/<rule>`.
    /// The frontier never constructs a raw string category.
    pub fn parse(s: &str) -> Option<Category> {
        if let Some(known) = Category::FIRST_PARTY.iter().find(|c| c.as_str() == s) {
            return Some(known.clone());
        }
        let rest = s.strip_prefix("ext:")?;
        let (coordinate, rule) = rest.split_once('/')?;
        (!coordinate.is_empty() && !rule.is_empty()).then(|| Category(SmolStr::from(s)))
    }

    pub fn is_plugin(&self) -> bool {
        self.0.starts_with("ext:")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The facet of a finding's subject; derived from [`crate::subject::Subject`], never
/// stored beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SubjectKind {
    File,
    Symbol,
    Package,
    Dependency,
    Directory,
    Suppression,
}

impl SubjectKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SubjectKind::File => "file",
            SubjectKind::Symbol => "symbol",
            SubjectKind::Package => "package",
            SubjectKind::Dependency => "dependency",
            SubjectKind::Directory => "directory",
            SubjectKind::Suppression => "suppression",
        }
    }
}
