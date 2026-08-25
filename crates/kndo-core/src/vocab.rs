//! Language-neutral graph vocabulary.
//!
//! If implementing a feature seems to require `if language == X` anywhere in the core, this
//! vocabulary is missing a concept — extend it here instead.

use smol_str::SmolStr;

// ---------------------------------------------------------------- shared primitives
//
// `ProjectPath`/`Span`/`Diagnostic`/`DiagnosticLevel` live here rather than in `adapter.rs`
// because they're not adapter-specific — the query/engine/plugin layers carry them too, and
// vocab.rs is the crate's one shared-vocabulary module. `adapter.rs` re-exports all four so
// existing adapter code (`crate::adapter::ProjectPath`, ...) keeps compiling unchanged.

/// Project-relative path with `/` separators, the only path form that crosses the adapter
/// boundary (case handling and symlink resolution are the core's discovery concern).
#[derive(
    Debug,
    Clone,
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
#[serde(transparent)]
pub struct ProjectPath(#[rkyv(with = crate::rkyv_support::SmolStrAsString)] pub SmolStr);

/// 1-indexed line/column span, `start` inclusive, `end` exclusive. Serializes as the
/// `[line, col]` pair shape the output schema uses, not an
/// object — tuples serialize as JSON arrays by default.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Span {
    pub start: (u32, u32),
    pub end: (u32, u32),
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    /// The run could not do what was asked (the exit-2 tier): a requested mode is
    /// impossible (`--diff` base that doesn't resolve), not merely degraded. Frontends exit 2
    /// when any error-level diagnostic is present — reporting zero findings because the
    /// analysis never ran must never read as a clean pass (a Warn here
    /// would let a typo'd base ref fail open in CI).
    Error,
    Warn,
    Info,
}

impl std::fmt::Display for DiagnosticLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DiagnosticLevel::Error => "error",
            DiagnosticLevel::Warn => "warning",
            DiagnosticLevel::Info => "info",
        })
    }
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    /// The file this diagnostic is about, when there is one — `None` for project-level
    /// diagnostics (e.g. "cannot walk the project root"). A diagnostic merged from many
    /// files without this field would be unattributable; adapters emit diagnostics scoped
    /// to the file they're extracting, the core fills this in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<ProjectPath>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

// ---------------------------------------------------------------- interned ids

/// Interned; stable within a snapshot. Assigned by a deterministic post-collection sort,
/// never by completion order.
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
/// name (same declaration contract either way).
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

/// A workspace unit: one manifest + the files it governs.
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
/// (the taxonomy rule).
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
    /// A type's constructor (Java/Kotlin `<init>`, Swift `init`): instantiation references
    /// the *type*, never the constructor symbol, so a constructor's liveness follows its
    /// class — `unused` exempts the kind outright.
    Constructor,
    /// An expansion symbol (Rust `macro_rules!`, a C preprocessor macro): invoked
    /// *textually*, outside the language's module-visibility model, and its body executes
    /// at the expansion sites, not where the template is written. Visibility-scope analyses
    /// therefore cannot trust observed use sites for this kind — neither for the macro
    /// itself nor as the *origin* of references attributed to it.
    Macro,
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
            SymbolKind::Constructor => "constructor",
            SymbolKind::Macro => "macro",
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

/// Analysis semantics per scope: peer is exempt from `unused`; optional demotes
/// findings to `possible`.
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

/// Reference subtype. `Implement`/`Override` drive dispatch-aware member liveness;
/// `Extend`/`TypeUse` distinguish type-level from value-level consumption.
/// Adapter-supplied per reference (`RawReference::kind` — serde derives are
/// for the facts cache); untagged references are `Read`, the undifferentiated default.
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
/// the strongest evidence and "edges at least as strong as τ" is a simple `>=`.
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
/// `Wildcard`'s plausible-target-set expansion — is the one normatively defined tiered
/// algorithm (`reachability`), not left to each analysis to reinvent.
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
    /// May cross `Package` boundaries.
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
    /// A **liveness** edge to a whole file: "if `from` is alive, `to` is in
    /// use" — the shape a template/asset relationship has (`res.render("index")` →
    /// `views/index.ejs`, a CSS class used from an HTML template) when the target file has no
    /// symbols to reference. Today produced only by plugin file-target contributions (the
    /// plugin edge sink; adapters keep emitting `ImportsFile` for real imports). Contract
    ///: liveness evidence, never architecture evidence — reachability consumes it
    /// exactly like `ImportsFile`, while `cyclic` and every analysis that would *create* a
    /// finding from an edge's existence ignore it. A false edge can therefore only ever
    /// suppress findings, preserving the zero-false-positive bar by construction.
    ReferencesFile {
        from: NodeRef,
        to: FileId,
    },
    /// A **process-boundary invocation** (the invoked-program rule): `from`
    /// executes the file `to` as a program — a test running its own workspace binary
    /// (`env!("CARGO_BIN_EXE_…")`), resolved through the manifest's named executable
    /// targets. Unlike `ImportsFile` (importing runs only load-time code), *executing* a
    /// program runs its entry point: reachability traverses this edge to the target file
    /// AND to every Production `Root` target declared inside it, so the invoked program's
    /// whole call tree inherits the invoker's colors. Same liveness-only contract as
    /// [`Self::ReferencesFile`]: never architecture evidence — `cyclic` and every analysis
    /// that would *create* a finding from an edge's existence ignore it, so a false edge
    /// can only ever suppress findings.
    InvokesFile {
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
    /// The core's surface-closure phase: Root edges derived from
    /// named re-exports out of surface files and from surface-transitive members of surface
    /// types. A distinct provenance so the incremental patch can strip and recompute the
    /// whole closure exactly (it is cross-file by nature — no single owner file's change
    /// scopes it).
    Surface,
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
    /// edge carried one (the `EdgeRef.site` — "an agent can jump straight to the
    /// proving line"). `None` for edges with no distinct
    /// evidence site of their own — role-derived and manifest-declared roots are markers, not
    /// spanned facts.
    pub span: Option<Span>,
    /// The file whose facts produced this edge (ownership is explicit, never
    /// inferred from edge shape — shapes provably lie: a narrowed-dynamic or opaque-namespace
    /// `Wildcard` points *from the target* but is produced by the importer, and a barrel's
    /// re-export `Root` promotion targets a symbol in another file; manifest-derived edges
    /// are owned by the manifest's own FileId). This is the incremental patch's exact removal
    /// set (`owner ∈ changed set`), and the invalidation hook plugin contributions will use.
    /// Last in declaration order deliberately: the canonical sort keys on semantics first.
    pub owner: FileId,
}

// ---------------------------------------------------------------- finding taxonomy

/// A finding's section — the taxonomy RFC 0018 §"reserved" closes: exactly these five, plus
/// `Convention` reserved for plugin-contributed findings (`category` stays an open
/// `plugin:<coordinate>/<rule>` namespace — RFC 0018 §2.1 — but no finding may claim a group
/// outside this set). [`Group::DISPLAY_ORDER`] is the single source of section ordering —
/// every renderer reads it instead of keeping its own copy (a duplicated 4-entry copy of this
/// list, missing `Convention`, is exactly how `agent_format.rs` and the CLI's `render.rs`
/// used to missort every plugin finding under an unnamed fallback section).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Group {
    Defect,
    Waste,
    Risk,
    Hygiene,
    /// Reserved for plugin-contributed findings — a core analysis never emits it.
    Convention,
}

impl Group {
    pub const DISPLAY_ORDER: [Group; 5] = [
        Group::Defect,
        Group::Waste,
        Group::Risk,
        Group::Hygiene,
        Group::Convention,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Group::Defect => "defect",
            Group::Waste => "waste",
            Group::Risk => "risk",
            Group::Hygiene => "hygiene",
            Group::Convention => "convention",
        }
    }
}

impl std::fmt::Display for Group {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A finding's category — an open namespace (RFC 0018 §2.1: a plugin's own category is
/// `plugin:<coordinate>/<rule>`, never a bare or off-namespace name), so unlike [`Group`] this
/// is a validated string newtype rather than a closed enum. The core categories are
/// associated consts; [`Category::plugin`] builds the namespaced form; [`Category::is_plugin`]
/// tells the two apart without a string-prefix check at every call site.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
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
    pub const UNTESTED: Category = Category(SmolStr::new_static("untested"));
    pub const UNUSED: Category = Category(SmolStr::new_static("unused"));
    pub const VERSION_SKEW: Category = Category(SmolStr::new_static("version-skew"));

    pub fn new(raw: impl Into<SmolStr>) -> Category {
        Category(raw.into())
    }

    /// RFC 0018 §2.1's namespaced form for a plugin-contributed finding.
    pub fn plugin(coordinate: &str, rule: &str) -> Category {
        Category(SmolStr::new(format!("plugin:{coordinate}/{rule}")))
    }

    /// Whether this category is in the reserved `plugin:` namespace — the one axis
    /// [`Category`] doesn't close: everything *else* is core, by construction (adapters and
    /// analyses never emit a `plugin:`-prefixed category; the host enforces the prefix on the
    /// plugin side, RFC 0018 §2.1).
    pub fn is_plugin(&self) -> bool {
        self.0.starts_with("plugin:")
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Every `&str` method (`.starts_with()`, `.contains()`, …) works directly on a `Category` —
/// it's a validated string, not an opaque token.
impl std::ops::Deref for Category {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq<str> for Category {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Category {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl From<&str> for Category {
    fn from(raw: &str) -> Category {
        Category::new(raw)
    }
}

impl From<String> for Category {
    fn from(raw: String) -> Category {
        Category::new(raw)
    }
}

/// A finding's subject facet (`category:subject` targeting, RFC 0005) — open like
/// [`Category`]: most values are the fixed `file | directory | dependency | package |
/// suppression` set, but a symbol-subject finding's facet is [`SymbolKind::facet`], which
/// itself carries an open [`SymbolKind::Other`] tail for languages the closed variants don't
/// cover — so this can never be a closed enum either.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct SubjectKind(SmolStr);

impl SubjectKind {
    pub const DEPENDENCY: SubjectKind = SubjectKind(SmolStr::new_static("dependency"));
    pub const DIRECTORY: SubjectKind = SubjectKind(SmolStr::new_static("directory"));
    pub const FILE: SubjectKind = SubjectKind(SmolStr::new_static("file"));
    pub const PACKAGE: SubjectKind = SubjectKind(SmolStr::new_static("package"));
    pub const SUPPRESSION: SubjectKind = SubjectKind(SmolStr::new_static("suppression"));

    pub fn new(raw: impl Into<SmolStr>) -> SubjectKind {
        SubjectKind(raw.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Display for SubjectKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::ops::Deref for SubjectKind {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq<str> for SubjectKind {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for SubjectKind {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl From<&str> for SubjectKind {
    fn from(raw: &str) -> SubjectKind {
        SubjectKind::new(raw)
    }
}

impl From<String> for SubjectKind {
    fn from(raw: String) -> SubjectKind {
        SubjectKind::new(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_orders_by_strength() {
        assert!(Confidence::Certain > Confidence::Probable);
        assert!(Confidence::Probable > Confidence::Possible);
        // "at least as strong as τ" is >= (tiered reachability)
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
