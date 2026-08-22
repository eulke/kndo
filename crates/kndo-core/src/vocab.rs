//! Language-neutral graph vocabulary (contracts/core-traits.md §1).
//!
//! If implementing a feature seems to require `if language == X` anywhere in the core, this
//! vocabulary is missing a concept — extend it here instead (RFC 0001 §2).

use smol_str::SmolStr;

use crate::adapter::Span;

// ---------------------------------------------------------------- interned ids

/// Interned; stable within a snapshot. Assigned by a deterministic post-collection sort,
/// never by completion order (RFC 0008 §4).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct FileId(pub u32);

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct SymbolId(pub u32);

/// A package consumed as a dependency — external, or an in-repo workspace member imported by
/// name (RFC 0011 §4; same declaration contract either way).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct DependencyId(pub u32);

/// A workspace unit: one manifest + the files it governs (RFC 0011).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct PackageId(pub u32);

// ---------------------------------------------------------------- file classification

/// A file's classification is two orthogonal axes, never one enum: a generated test file and
/// a vendored production file are both expressible. `FileRole` mirrors `RootKind` on purpose.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub enum FileRole {
    Production,
    Test,
    Tooling,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub enum FileOrigin {
    Authored,
    Generated,
    Vendored,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct FileClass {
    pub role: FileRole,
    pub origin: FileOrigin,
}

impl Default for FileClass {
    fn default() -> Self {
        FileClass {
            role: FileRole::Production,
            origin: FileOrigin::Authored,
        }
    }
}

// ---------------------------------------------------------------- symbols & roots

/// Kebab-case names double as `subject_kind` facet values (alongside `file | directory |
/// package | dependency | import | suppression`) in output and `category:subject` targeting
/// (RFC 0005 taxonomy rule 2).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Interface,
    Struct,
    Enum,
    EnumMember,
    TypeAlias,
    Const,
    Static,
    Variable,
    Field,
    Module,
    CssRule,
    CssVariable,
    Other(#[rkyv(with = crate::rkyv_support::SmolStrAsString)] SmolStr),
}

impl SymbolKind {
    /// The kebab-case facet name used in output and `category:subject` targeting.
    pub fn facet(&self) -> &str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::EnumMember => "enum-member",
            SymbolKind::TypeAlias => "type-alias",
            SymbolKind::Const => "const",
            SymbolKind::Static => "static",
            SymbolKind::Variable => "variable",
            SymbolKind::Field => "field",
            SymbolKind::Module => "module",
            SymbolKind::CssRule => "css-rule",
            SymbolKind::CssVariable => "css-variable",
            SymbolKind::Other(name) => name.as_str(),
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub enum RootKind {
    Production,
    Test,
    Tooling,
}

/// Analysis semantics per scope: RFC 0005 §5 (peer exempt from `unused`; optional demotes
/// findings to `possible`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum DependencyScope {
    Prod,
    Dev,
    Build,
    Peer,
    Optional,
}

// ---------------------------------------------------------------- edges

/// Reference subtype. `Implement`/`Override` drive dispatch-aware member liveness
/// (RFC 0005 §2); `Extend`/`TypeUse` distinguish type-level from value-level consumption.
/// Adapter-supplied per reference since RFC 0012 §5 (`RawReference::kind` — serde derives are
/// for the facts cache); untagged references are `Read`, exactly the pre-§5 behavior.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub enum RefKind {
    Call,
    Read,
    Write,
    Extend,
    Implement,
    Override,
    TypeUse,
}

/// Per-edge strength. Ordered by strength: `Possible < Probable < Certain`, so `max()` yields
/// the strongest evidence and "edges at least as strong as τ" is a simple `>=` (RFC 0005 §1).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Possible,
    Probable,
    Certain,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    Hash,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub enum NodeRef {
    File(FileId),
    Symbol(SymbolId),
}

/// How per-edge confidence combines into a node's `(color, confidence)` — including
/// `Wildcard`'s plausible-target-set expansion — is the tiered algorithm normatively defined
/// in RFC 0005 §1, not left to each analysis to reinvent.
#[derive(
    Debug,
    Clone,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    Hash,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub enum EdgeKind {
    /// May cross `Package` boundaries (RFC 0011 §4).
    ImportsFile {
        from: FileId,
        to: FileId,
    },
    ImportsDependency {
        from: FileId,
        to: DependencyId,
    },
    /// `from` is `NodeRef` rather than `SymbolId`: without enclosing-scope tracking during
    /// extraction, an adapter can know *which file* contains a reference without knowing
    /// *which declaration* — `NodeRef::File` for that file-granularity case (safe for
    /// reachability: a reachable file referencing a symbol makes that symbol reachable
    /// regardless of which of the file's own functions did the referencing), `NodeRef::Symbol`
    /// once an adapter tracks enclosing scope precisely enough to say more.
    References {
        from: NodeRef,
        to: SymbolId,
        kind: RefKind,
    },
    Declares {
        file: FileId,
        symbol: SymbolId,
    },
    Root {
        kind: RootKind,
        target: NodeRef,
    },
    /// Dynamic construct; resolved against a plausible target set, not a fixed target.
    Wildcard {
        from: FileId,
    },
    /// A **liveness** edge to a whole file (RFC 0017 §5.4): "if `from` is alive, `to` is in
    /// use" — the shape a template/asset relationship has (`res.render("index")` →
    /// `views/index.ejs`, a CSS class used from an HTML template) when the target file has no
    /// symbols to reference. Today produced only by plugin file-target contributions (the
    /// plugin edge sink; adapters keep emitting `ImportsFile` for real imports). Contract
    /// (RFC 0005): liveness evidence, never architecture evidence — reachability consumes it
    /// exactly like `ImportsFile`, while `cyclic` and every analysis that would *create* a
    /// finding from an edge's existence ignore it. A false edge can therefore only ever
    /// suppress findings, preserving the zero-false-positive bar by construction.
    ReferencesFile {
        from: NodeRef,
        to: FileId,
    },
}

/// Identity of the component whose facts produced an edge/annotation — for attribution in
/// output (`"sources": ["adapter:js-ts"]`).
#[derive(
    Debug,
    Clone,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    Hash,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub enum Provenance {
    Adapter(#[rkyv(with = crate::rkyv_support::SmolStrAsString)] SmolStr),
    Plugin(#[rkyv(with = crate::rkyv_support::SmolStrAsString)] SmolStr),
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    Hash,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct Edge {
    pub kind: EdgeKind,
    pub confidence: Confidence,
    pub source: Provenance,
    /// The extraction-time span this edge's evidence came from — the import statement, the
    /// reference site, the declaration itself for `Declares` — when the fact that produced this
    /// edge carried one (contracts/output-schema.md §8's `EdgeRef.site`, RFC 0007 design tenet
    /// 4: "an agent can jump straight to the proving line"). `None` for edges with no distinct
    /// evidence site of their own — role-derived and manifest-declared roots are markers, not
    /// spanned facts.
    pub span: Option<Span>,
    /// The file whose facts produced this edge (RFC 0013 §2: ownership is explicit, never
    /// inferred from edge shape — shapes provably lie: a narrowed-dynamic or opaque-namespace
    /// `Wildcard` points *from the target* but is produced by the importer, and a barrel's
    /// re-export `Root` promotion targets a symbol in another file; manifest-derived edges
    /// are owned by the manifest's own FileId). This is the incremental patch's exact removal
    /// set (`owner ∈ changed set`), and the invalidation hook plugin contributions will use.
    /// Last in declaration order deliberately: the canonical sort keys on semantics first.
    pub owner: FileId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_orders_by_strength() {
        assert!(Confidence::Certain > Confidence::Probable);
        assert!(Confidence::Probable > Confidence::Possible);
        // "at least as strong as τ" is >= (RFC 0005 §1 tiered reachability)
        assert!(Confidence::Certain >= Confidence::Possible);
    }

    #[test]
    fn symbol_kind_facets_are_kebab_case() {
        assert_eq!(SymbolKind::EnumMember.facet(), "enum-member");
        assert_eq!(SymbolKind::TypeAlias.facet(), "type-alias");
        assert_eq!(
            SymbolKind::Other(SmolStr::new("sql-query")).facet(),
            "sql-query"
        );
    }
}
