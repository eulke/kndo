//! What an adapter reports about one file: evidence — raw material weighted by
//! confidence, prior to all judgment. Analyses weigh evidence and return verdicts;
//! without the evidence they need, they abstain.
//!
//! Adapters never build [`FileEvidence`] by hand: extraction writes through
//! [`EvidenceSink`], which validates at the call site and returns ids — attaching
//! metrics or membership is by [`DeclarationId`], so "must byte-match another span" style
//! conventions have nothing to exist for. The engine reads the finished value.
//!
//! ## The growth contract
//!
//! This module grows for as long as languages keep teaching us things, under four
//! rules that keep the growth safe:
//!
//! 1. **Pairing**: every OPTIONAL evidence stream is declared in [`EvidenceStreams`],
//!    so an empty stream is typed — "none exist" (declared) vs "this adapter doesn't
//!    know" (undeclared). Analyses abstain over undeclared streams instead of
//!    guessing. The mandatory spine (declarations, references, imports, roots) is not
//!    optional and not listed.
//! 2. **Default compatibility**: a new stream or capability defaults to
//!    not-declared/empty, which through degrade-toward-keep-alive reproduces the
//!    pre-capability behavior: absence can silence an analysis, never accuse.
//! 3. **Additive surface**: the sink only gains methods; growable enums are
//!    `#[non_exhaustive]` and every consumer's wildcard arm degrades toward
//!    keep-alive (an unknown `RefKind` counts as a use; an unknown `ImportShape`
//!    keeps its import alive). Enums documented "closed by design" are contracts
//!    whose extension is a semantic change, not growth.
//! 4. Every shape change moves the contract fingerprint — visible, versioned,
//!    deliberate.

use crate::fingerprint::ContractFingerprint;
use crate::subject::{Subject, SymbolSelector};
use crate::vocab::{Confidence, ProjectPath, Span};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

/// Index of a declaration within one file's evidence. Only the sink that owns the
/// file hands these out; there is no public constructor to forge one from an integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ContractFingerprint)]
#[serde(transparent)]
pub struct DeclarationId(u32);

impl DeclarationId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// An optional evidence stream — one whose absence would be ambiguous without a
/// declaration. Grows a variant whenever a language teaches us a new stream
/// (test spans, units, …); the default for every adapter is not-declared.
#[non_exhaustive]
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
pub enum EvidenceStream {
    Comments,
    Metrics,
    /// Attributes, annotations, decorators, pragmas — see [`Marker`].
    Markers,
    /// Supertype links — see [`Relation`].
    Relations,
    /// Whether each reference was written ON something — see
    /// [`Reference::on`]. Undeclared, every reference is a bare name AND
    /// means nothing by it, so a judgment that needs the difference keeps the
    /// answer it gave before the stream existed.
    Qualifiers,
}

/// The set of optional streams an adapter DECLARES it produces — the pairing rule.
/// Carried on every [`FileEvidence`] so analyses can tell "empty because none exist"
/// from "unjudgeable because unreported", per file, per claiming adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ContractFingerprint)]
#[serde(transparent)]
pub struct EvidenceStreams {
    set: Vec<EvidenceStream>,
}

impl EvidenceStreams {
    pub fn none() -> Self {
        EvidenceStreams { set: Vec::new() }
    }

    pub fn of(streams: &[EvidenceStream]) -> Self {
        let mut set: Vec<EvidenceStream> = streams.to_vec();
        set.sort();
        set.dedup();
        EvidenceStreams { set }
    }

    pub fn contains(&self, stream: EvidenceStream) -> bool {
        self.set.contains(&stream)
    }

    /// The declared streams, sorted — what a wire spelling serializes; membership
    /// and iteration come from the ONE set, so a new stream variant can never be
    /// silently stripped by an enumeration someone else kept.
    pub fn iter(&self) -> impl Iterator<Item = EvidenceStream> + '_ {
        self.set.iter().copied()
    }
}

/// Grows as languages need it; a consumer's wildcard arm treats an unknown kind as a
/// plain symbol and never triggers kind-specific accusations.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "kebab-case")]
pub enum SymbolKind {
    Function,
    Method,
    Type,
    Constant,
    Variable,
    Module,
    /// The adapter's own word for a kind the taxonomy lacks — carried, never dropped.
    Other(SmolStr),
}

/// Both conversions between [`SymbolKind`] and a MIRROR enum with the same
/// variant names — the generated wire type on either side of the ABI. The table
/// lives here once: a side invokes the macro on its own generated type and gets
/// `symbol_kind_to_wire` and `symbol_kind_from_wire`, so the two sides cannot
/// drift by hand. `$wire` is an identifier: bring the generated type into scope
/// under one (`use … as WireSymbolKind`) and name that.
#[macro_export]
macro_rules! symbol_kind_conversions {
    ($wire:ident) => {
        fn symbol_kind_to_wire(kind: &$crate::evidence::SymbolKind) -> $wire {
            match kind {
                $crate::evidence::SymbolKind::Function => $wire::Function,
                $crate::evidence::SymbolKind::Method => $wire::Method,
                $crate::evidence::SymbolKind::Type => $wire::Type,
                $crate::evidence::SymbolKind::Constant => $wire::Constant,
                $crate::evidence::SymbolKind::Variable => $wire::Variable,
                $crate::evidence::SymbolKind::Module => $wire::Module,
                $crate::evidence::SymbolKind::Other(name) => $wire::Other(name.to_string()),
                // The contract enum is `#[non_exhaustive]`: a kind this wire
                // build does not know yet crosses under `other` in its debug
                // spelling, never as a dropped declaration.
                other => $wire::Other(format!("{other:?}")),
            }
        }

        fn symbol_kind_from_wire(kind: $wire) -> $crate::evidence::SymbolKind {
            match kind {
                $wire::Function => $crate::evidence::SymbolKind::Function,
                $wire::Method => $crate::evidence::SymbolKind::Method,
                $wire::Type => $crate::evidence::SymbolKind::Type,
                $wire::Constant => $crate::evidence::SymbolKind::Constant,
                $wire::Variable => $crate::evidence::SymbolKind::Variable,
                $wire::Module => $crate::evidence::SymbolKind::Module,
                $wire::Other(name) => $crate::evidence::SymbolKind::Other(name.into()),
            }
        }
    };
}

impl SymbolKind {
    /// The one text spelling, the adapter's own word included.
    pub fn as_str(&self) -> &str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Type => "type",
            SymbolKind::Constant => "constant",
            SymbolKind::Variable => "variable",
            SymbolKind::Module => "module",
            SymbolKind::Other(word) => word.as_str(),
        }
    }
}

/// How far a declaration's name legally reaches. Not a ladder: `Scoped` carries
/// the ADAPTER'S OWN WORD for a bounded region ("package", "module", "crate",
/// "in:a::b") — core never parses or compares tokens, it asks the adapter for
/// the region's files ([`crate::extension::Extension::seen_from`]) and judges by
/// the SET. Private and Exported stay the shared halves; an unanswerable token
/// degrades to Exported treatment (keep-alive). Growing this enum was the
/// deliberate semantic contract change recorded for M6.c — the 2026-08 audit
/// measured that no false-positive fix ever wanted a rung; every one wanted
/// scope SHAPE.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Reach {
    Private,
    /// Nameable within the file's own namespace — the node it declares itself
    /// into ([`FileEvidence::namespace`]) — or an ancestor `up` levels above
    /// it. Java's package-private is `Namespace { up: 0 }`; Rust's
    /// `pub(super)` is `up: 1`. The pool is the namespace's files, so a test
    /// laid out anywhere at all still shares the package it declares.
    Namespace {
        up: u32,
    },
    /// Nameable beyond its file, only within a region the declaring adapter can
    /// enumerate from paths and manifests — never from contents. The
    /// path-derived predecessor of `Namespace`, retired as each adapter
    /// declares its namespaces.
    Scoped {
        scope: SmolStr,
    },
    Exported,
}

#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
pub struct Declaration {
    pub name: SmolStr,
    pub kind: SymbolKind,
    pub span: Span,
    pub reach: Reach,
    /// Set via [`EvidenceSink::member_of`] — by id, never by name lookup.
    pub owner: Option<DeclarationId>,
    /// The region that is a PROMISE to consumers — a callable's parameter and
    /// return-type text, never its body. Set via [`EvidenceSink::signature`];
    /// `None` (the default) means the adapter does not delimit signatures for
    /// this declaration, and signature-scoped analyses (`private-type-leak`)
    /// stay silent for it — degrade toward silence, never toward accusation.
    pub signature_span: Option<Span>,
    /// The module-system name this declaration is exported under when it differs
    /// from its local name (`export default`, `export { local as alias }`) — what
    /// importers actually bind. Set via [`EvidenceSink::exported_as`].
    pub exported_as: Option<SmolStr>,
    /// What the language reads beyond the identifier to tell same-named
    /// declarations apart, as it spells it: Java's parameter types
    /// `(int, String)`, Swift's argument labels `(_:with:)`. Never parameter
    /// names or a return type — renaming a parameter does not make a new
    /// method — and never the identifier, which references carry alone. Set
    /// via [`EvidenceSink::signature`]; `None` where the language has none or
    /// the adapter does not spell it, in which case `nth` alone keeps two
    /// same-named declarations apart.
    pub signature: Option<SmolStr>,
    /// Position among this file's declarations that share owner name, name and
    /// signature, in source order. Computed by the sink when the evidence is
    /// finished, never set by an adapter: it is what makes every selector in
    /// a file unique when nothing the language says would.
    pub nth: u32,
}

/// Grows as languages need it; an unknown kind in a consumer's wildcard arm counts as
/// a use (keep-alive), never as evidence for an accusation.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "lowercase")]
pub enum RefKind {
    Call,
    Read,
    Write,
    Extend,
    Implement,
    TypeUse,
}

#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
pub struct Reference {
    pub name: SmolStr,
    pub kind: RefKind,
    /// What this name was read FROM, as written — `Some("LongMath")` for
    /// `LongMath.FLOOR_SQRT_MAX_LONG`, `Some("queue")` for `queue.head`, and
    /// `None` for a bare name: a local, a parameter, an unqualified call, or a
    /// receiver that IS the enclosing declaration (Java's `this`), which
    /// reaches the same members a bare name does.
    ///
    /// Meaningful only where the claiming adapter declares
    /// [`EvidenceStream::Qualifiers`]; undeclared, it is always `None` and a
    /// consumer must read the stream before believing it.
    pub on: Option<SmolStr>,
    pub span: Span,
}

/// Where an import points. The specifier string as written lives inside the variant
/// that needs it; resolution happens engine-side through the adapter's resolver.
/// Grows as languages need it; an unknown target resolves to Unresolved-keep-alive.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "lowercase")]
pub enum ImportTarget {
    /// A relative specifier (`./util`, `../lib/x`), as written.
    Relative(SmolStr),
    /// A package/bare specifier (`react`, `lodash/fp`), as written.
    Package(SmolStr),
}

impl ImportTarget {
    /// The specifier as the author wrote it, whatever its form — the one
    /// spelling a subject carries and a position is counted over.
    pub fn as_written(&self) -> &str {
        match self {
            ImportTarget::Relative(s) | ImportTarget::Package(s) => s.as_str(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
pub struct ImportBinding {
    pub imported: SmolStr,
    pub local: SmolStr,
}

/// The FORM of an import is one closed choice — not a bag of independent booleans
/// whose illegal combinations need documenting. Grows as languages need it; an
/// unknown shape keeps its import (and whatever it binds) alive.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "kebab-case")]
pub enum ImportShape {
    Bindings(Vec<ImportBinding>),
    Namespace {
        local: SmolStr,
    },
    SideEffect,
    Reexport(Vec<ImportBinding>),
    /// The target's whole exported surface re-exported (`export * from`): consumers
    /// cannot see through it, so it keeps that surface alive.
    ReexportAll,
    /// Everything the target exports, imported unbound (`use x::*`): nothing names
    /// what was taken, so the whole surface stays alive.
    Glob,
    /// The specifier is spelled in the file — a string literal a loader or a
    /// runtime may resolve — never imported by the language: enough for a
    /// declared dependency to count as used, never a reachability edge.
    Mention,
}

/// WHEN an import runs — a fact the adapter reads off the syntax, never a
/// judgment. `Load`: at module load, in the order the language links
/// (`import x`, `require()` at top level, `use`, `mod`). `Lazy`: when the code
/// around it executes — a dynamic `import()`, a `require()` inside a function
/// or a branch, a Python import in a function body. `Erased`: never — the
/// import exists for the type checker alone (`import type`, a `TYPE_CHECKING`
/// block). Reachability keeps every timing (a type used is a type kept); the
/// `cyclic` analysis judges load-time edges only, because an initialization
/// hazard needs initialization. Grows if a language teaches a fourth moment;
/// a consumer's wildcard arm reads an unknown timing as `Lazy` — reached,
/// never a hazard.
#[non_exhaustive]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, ContractFingerprint,
)]
#[serde(rename_all = "lowercase")]
pub enum Timing {
    #[default]
    Load,
    Lazy,
    Erased,
}

#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
pub struct Import {
    pub target: ImportTarget,
    pub shape: ImportShape,
    pub span: Span,
    pub confidence: Confidence,
    pub timing: Timing,
    /// Position among this file's imports of the same target as written, in
    /// source order. Computed by the sink when the evidence is finished,
    /// never set by an adapter: a file that imports `./x` twice states two
    /// imports, and a finding on each must be two findings.
    pub nth: u32,
}

/// Closed by design: the role taxonomy (production/test/tooling) is a reporting
/// contract — extending it changes what every color-based verdict means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "lowercase")]
pub enum RootKind {
    Production,
    Test,
    Tooling,
}

/// What a root anchors: the whole file, or one declaration — by id. Grows if roots
/// ever anchor something else; an unknown target keeps the whole file alive.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "kebab-case")]
pub enum RootTarget {
    WholeFile,
    Declaration(DeclarationId),
}

#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
pub struct Root {
    pub target: RootTarget,
    pub kind: RootKind,
    pub confidence: Confidence,
}

/// What a marker sits on. Grows if a language attaches markers to something
/// else (a parameter, a statement); an unknown target marks nothing dispatch
/// can act on — silence, never an accusation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "kebab-case")]
pub enum MarkerTarget {
    /// The whole file — an inner attribute (`#![…]`), a module-level pragma.
    File,
    Declaration(DeclarationId),
}

/// A marker: an attribute, annotation, decorator or pragma the source attaches
/// to a declaration or to the whole file — the SYNTAX as data, never its
/// meaning. `path` is the marker's name as written, segments joined in the
/// language's own spelling (`test`, `tokio::test`, `org.junit.Test`); `args`
/// holds its top-level arguments as written, each trimmed with inner
/// whitespace runs collapsed to one space (`dead_code`, `feature = "x"`,
/// `rename_all = "camelCase"`). What a marker MEANS is a
/// [`crate::extension::DispatchRule`] on the claiming extension's spec: the
/// engine matches the rules against the markers and derives roots and
/// exemptions, so a framework's test attribute is one line of data and never
/// a branch in an adapter.
#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
pub struct Marker {
    pub on: MarkerTarget,
    pub path: SmolStr,
    pub args: Vec<SmolStr>,
    pub span: Span,
}

/// A comment's extent plus the extent of its text (delimiters stripped). Adapters
/// report where comments ARE; the engine owns what a `kndo:` pragma inside one means —
/// so every adapter, WASM included, gets suppression for free and the grammar has one
/// owner.
#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
pub struct CommentSpan {
    pub span: Span,
    pub text: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
pub struct FunctionMetrics {
    pub cyclomatic: u32,
    pub loc: u32,
    pub token_count: u32,
    pub fingerprints: Vec<u64>,
}

/// Closed by design: three levels are the reporting contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum DiagnosticLevel {
    Info,
    Warn,
    Error,
}

/// An adapter telling the run something went sideways in this file (a parse error, a
/// clamped span). Extraction itself never fails — it degrades and says so.
#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
pub struct AdapterDiagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
    pub span: Option<Span>,
}

/// How a declaration relates to a named type. Grows as languages teach us
/// links; an unknown kind relates nothing, which keeps the pair independent —
/// silence, never an accusation.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "lowercase")]
pub enum RelationKind {
    /// A class extends a base class; an interface extends another.
    Extends,
    /// A type implements an interface, conforms to a protocol, satisfies a
    /// trait bound — one word for "promises another type's surface".
    Implements,
}

/// A typed link from a declaration to a NAMED type, as the file writes it:
/// `class A extends B implements C`. The name is unresolved on purpose — the
/// engine resolves it the way it resolves a reference, and an adapter that
/// tried to resolve would be re-deriving the project.
///
/// What the engine reads from it: a member whose owner relates to a type
/// declaring the same name is a WITNESS of that type's surface — an override,
/// an interface method, a protocol requirement. A witness is alive while its
/// owner is, and the member it witnesses can never narrow below it.
#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
pub struct Relation {
    pub from: DeclarationId,
    pub kind: RelationKind,
    /// The supertype's name as the language spells it at the use site, with
    /// generics and qualification stripped — the same spelling a reference to
    /// that type carries, so the two resolve alike.
    pub to: SmolStr,
    pub span: Span,
}

/// The finished evidence for one file. Built through [`EvidenceSink`]; read
/// everywhere.
#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
pub struct FileEvidence {
    /// The pairing rule's carrier: which optional streams the claiming adapter
    /// declared. Analyses read it to abstain over what was never reported.
    pub declared: EvidenceStreams,
    /// The namespace this file declares itself into, as SEGMENTS — `package
    /// com.foo;` is `["com", "foo"]`. Segments, not a joined name, so the
    /// engine can walk the tree without knowing the language's separator.
    /// Empty means the file declares none, and every namespace-reaching
    /// declaration in it pools nothing beyond itself. What identifies a
    /// namespace is the adapter's to say: Java's clause, a Go package's
    /// directory-qualified name, a Rust module's mount chain.
    pub namespace: Vec<SmolStr>,
    pub declarations: Vec<Declaration>,
    pub references: Vec<Reference>,
    pub imports: Vec<Import>,
    pub roots: Vec<Root>,
    pub markers: Vec<Marker>,
    pub relations: Vec<Relation>,
    pub comments: Vec<CommentSpan>,
    pub metrics: Vec<(DeclarationId, FunctionMetrics)>,
    pub diagnostics: Vec<AdapterDiagnostic>,
}

impl FileEvidence {
    /// Each declaration beside its id — the read-side counterpart of the sink
    /// handing ids out at write time, for a consumer that must NAME a declaration
    /// it found by inspection (the engine anchoring an outside root on one). Ids
    /// stay honest: this is the only way to obtain one after extraction, and every
    /// id it yields names a declaration this evidence actually holds.
    pub fn declarations_with_ids(&self) -> impl Iterator<Item = (DeclarationId, &Declaration)> {
        self.declarations
            .iter()
            .enumerate()
            .map(|(i, d)| (DeclarationId(i as u32), d))
    }

    /// The address of one declaration — THE place a declaration becomes a
    /// selector, so that an analysis, a query, an expectation and a plugin
    /// all spell it identically and none can forget a part. Unique within the
    /// file: the sink assigned `nth` for exactly that.
    pub fn selector_of(&self, id: DeclarationId) -> SymbolSelector {
        let d = &self.declarations[id.index()];
        SymbolSelector {
            owner: d.owner.map(|o| self.declarations[o.index()].name.clone()),
            name: d.name.clone(),
            signature: d.signature.clone(),
            nth: d.nth,
        }
    }

    /// The finding subject for one declaration: the file, its address, its
    /// span (carried for lines, never for identity).
    pub fn subject_of(&self, path: &ProjectPath, id: DeclarationId) -> Subject {
        Subject::Symbol {
            path: path.clone(),
            selector: self.selector_of(id),
            span: self.declarations[id.index()].span,
        }
    }

    /// The finding subject for one import statement, by its index in
    /// `imports` — THE place an import becomes a subject, so its specifier
    /// and its position among same-specifier imports are never re-spelled
    /// by an analysis.
    pub fn import_subject(&self, path: &ProjectPath, index: usize) -> Subject {
        let import = &self.imports[index];
        Subject::Import {
            path: path.clone(),
            specifier: SmolStr::new(import.target.as_written()),
            nth: import.nth,
            span: import.span,
        }
    }
}

/// The write side of extraction. Validates as evidence arrives — a span outside the
/// file is clamped and reported as a diagnostic (extraction degrades, never fails) —
/// and hands back the ids that make attachment misuse unrepresentable.
pub struct EvidenceSink {
    file_len: u32,
    out: FileEvidence,
}

impl EvidenceSink {
    /// `declares` comes from the claiming adapter's spec — the sink keeps the
    /// declaration truthful: writes to an undeclared optional stream are dropped and
    /// reported (the fix is one declaration in the spec, and conformance shows the
    /// warning immediately).
    pub fn new(file_len: u32, declares: EvidenceStreams) -> Self {
        EvidenceSink {
            file_len,
            out: FileEvidence {
                declared: declares,
                namespace: Vec::new(),
                declarations: Vec::new(),
                references: Vec::new(),
                imports: Vec::new(),
                roots: Vec::new(),
                markers: Vec::new(),
                relations: Vec::new(),
                comments: Vec::new(),
                metrics: Vec::new(),
                diagnostics: Vec::new(),
            },
        }
    }

    fn clamp(&mut self, mut span: Span, what: &str) -> Span {
        if span.end > self.file_len {
            self.out.diagnostics.push(AdapterDiagnostic {
                level: DiagnosticLevel::Warn,
                message: format!(
                    "{what} span {}..{} exceeds file length {} — clamped (adapter defect)",
                    span.start, span.end, self.file_len
                ),
                span: None,
            });
            span.end = self.file_len;
            span.start = span.start.min(span.end);
        }
        span
    }

    pub fn declaration(
        &mut self,
        name: impl Into<SmolStr>,
        kind: SymbolKind,
        span: Span,
        reach: Reach,
    ) -> DeclarationId {
        let span = self.clamp(span, "declaration");
        let id = DeclarationId(self.out.declarations.len() as u32);
        self.out.declarations.push(Declaration {
            name: name.into(),
            kind,
            span,
            signature: None,
            nth: 0,
            reach,
            owner: None,
            exported_as: None,
            signature_span: None,
        });
        id
    }

    /// True when `id` names a declaration this sink issued; otherwise the write is
    /// dropped with a diagnostic — the same posture the wire boundary promises, so
    /// a stale id from another file's sink degrades instead of panicking.
    fn valid_id(&mut self, id: DeclarationId, what: &str) -> bool {
        if id.index() < self.out.declarations.len() {
            return true;
        }
        self.out.diagnostics.push(AdapterDiagnostic {
            level: DiagnosticLevel::Warn,
            message: format!(
                "{what} names declaration index {} outside this file's {} — dropped \
                 (adapter defect: a DeclarationId from another sink?)",
                id.index(),
                self.out.declarations.len()
            ),
            span: None,
        });
        false
    }

    /// Membership by id: no name lookup, no span matching, nothing to mis-resolve.
    pub fn member_of(&mut self, member: DeclarationId, owner: DeclarationId) {
        debug_assert_ne!(member, owner, "a declaration cannot own itself");
        if !self.valid_id(member, "member_of") || !self.valid_id(owner, "member_of owner") {
            return;
        }
        self.out.declarations[member.index()].owner = Some(owner);
    }

    /// The signature region (see [`Declaration::signature_span`]) — by id.
    pub fn signature_span(&mut self, of: DeclarationId, span: Span) {
        if !self.valid_id(of, "signature_span") {
            return;
        }
        self.out.declarations[of.index()].signature_span = Some(span);
    }

    /// The language's own spelling of what tells this declaration from a
    /// same-named one — see [`Declaration::signature`]. By id, so a signature
    /// can never attach to the wrong declaration.
    pub fn signature(&mut self, of: DeclarationId, signature: impl Into<SmolStr>) {
        if !self.valid_id(of, "signature") {
            return;
        }
        self.out.declarations[of.index()].signature = Some(signature.into());
    }

    /// The exported alias, when it differs from the local name — by id, so the alias
    /// can never attach to the wrong declaration.
    pub fn exported_as(&mut self, of: DeclarationId, name: impl Into<SmolStr>) {
        if !self.valid_id(of, "exported_as") {
            return;
        }
        // The alias is a module-system fact, orthogonal to reach level (Kotlin's
        // `internal` + `@JvmName` coexist) — except on Private, where nothing can
        // bind it: that write is a defect, dropped here at the ONE constructor so
        // the inert combination cannot exist in finished evidence.
        if matches!(self.out.declarations[of.index()].reach, Reach::Private) {
            self.out.diagnostics.push(AdapterDiagnostic {
                level: DiagnosticLevel::Warn,
                message: format!(
                    "exported_as on a Private declaration (index {}) dropped — nothing \
                     can bind a private name (adapter defect)",
                    of.index()
                ),
                span: None,
            });
            return;
        }
        self.out.declarations[of.index()].exported_as = Some(name.into());
    }

    /// True when the write may proceed; otherwise drops it with a diagnostic so the
    /// declaration stays truthful and the adapter author sees the defect at once.
    fn declared(&mut self, stream: EvidenceStream) -> bool {
        if self.out.declared.contains(stream) {
            return true;
        }
        self.out.diagnostics.push(AdapterDiagnostic {
            level: DiagnosticLevel::Warn,
            message: format!(
                "write to undeclared stream {stream:?} dropped — declare it in the \
                 adapter's spec (adapter defect)"
            ),
            span: None,
        });
        false
    }

    /// Metrics attach to the declaration they describe — by id. (v1 matched by name
    /// against last-wins symbol tables and reported a method as a clone of itself.)
    pub fn metrics(&mut self, of: DeclarationId, m: FunctionMetrics) {
        if !self.valid_id(of, "metrics") {
            return;
        }
        if self.declared(EvidenceStream::Metrics) {
            self.out.metrics.push((of, m));
        }
    }

    /// The namespace this file declares itself into — see
    /// [`FileEvidence::namespace`]. Last write wins: a file declares one.
    pub fn namespace(&mut self, segments: impl IntoIterator<Item = SmolStr>) {
        self.out.namespace = segments.into_iter().collect();
    }

    pub fn reference(&mut self, name: impl Into<SmolStr>, kind: RefKind, span: Span) {
        self.reference_on(name, kind, None, span);
    }

    /// A reference and what it was read from — see [`Reference::on`]. The
    /// receiver is kept only where [`EvidenceStream::Qualifiers`] is declared;
    /// undeclared it is dropped, and the reference lands bare like every other.
    pub fn reference_on(
        &mut self,
        name: impl Into<SmolStr>,
        kind: RefKind,
        on: Option<SmolStr>,
        span: Span,
    ) {
        let span = self.clamp(span, "reference");
        // A BARE reference writes nothing to the stream, so an adapter that
        // never qualifies anything is not defective for calling this — only a
        // receiver offered without the declaration is.
        let on = on.filter(|_| self.declared(EvidenceStream::Qualifiers));
        self.out.references.push(Reference {
            name: name.into(),
            kind,
            on,
            span,
        });
    }

    /// An import that runs at load time — the common case, and the default the
    /// growth contract promises: an adapter that never learned about timing
    /// reports what every adapter reported before it existed.
    pub fn import(
        &mut self,
        target: ImportTarget,
        shape: ImportShape,
        span: Span,
        confidence: Confidence,
    ) {
        self.import_at(Timing::Load, target, shape, span, confidence);
    }

    /// An import with its moment stated — see [`Timing`]. A type-only import is
    /// spelled as its bindings at `Erased`: what it binds is the same evidence,
    /// when it runs is the only difference.
    pub fn import_at(
        &mut self,
        timing: Timing,
        target: ImportTarget,
        shape: ImportShape,
        span: Span,
        confidence: Confidence,
    ) {
        let span = self.clamp(span, "import");
        self.out.imports.push(Import {
            target,
            shape,
            span,
            confidence,
            timing,
            nth: 0,
        });
    }

    pub fn root(&mut self, target: RootTarget, kind: RootKind, confidence: Confidence) {
        if let RootTarget::Declaration(id) = target
            && !self.valid_id(id, "root")
        {
            return;
        }
        self.out.roots.push(Root {
            target,
            kind,
            confidence,
        });
    }

    /// A marker on a declaration (by id) or on the whole file — see [`Marker`].
    /// An optional stream: declare [`EvidenceStream::Markers`] or the write
    /// drops with a diagnostic.
    pub fn marker(
        &mut self,
        on: MarkerTarget,
        path: impl Into<SmolStr>,
        args: Vec<SmolStr>,
        span: Span,
    ) {
        if let MarkerTarget::Declaration(id) = on
            && !self.valid_id(id, "marker")
        {
            return;
        }
        if !self.declared(EvidenceStream::Markers) {
            return;
        }
        let span = self.clamp(span, "marker");
        self.out.markers.push(Marker {
            on,
            path: path.into(),
            args,
            span,
        });
    }

    /// A supertype link — see [`Relation`]. An optional stream: declare
    /// [`EvidenceStream::Relations`] or the write drops with a diagnostic.
    pub fn relation(
        &mut self,
        from: DeclarationId,
        kind: RelationKind,
        to: impl Into<SmolStr>,
        span: Span,
    ) {
        if !self.valid_id(from, "relation") {
            return;
        }
        if !self.declared(EvidenceStream::Relations) {
            return;
        }
        let span = self.clamp(span, "relation");
        self.out.relations.push(Relation {
            from,
            kind,
            to: to.into(),
            span,
        });
    }

    pub fn comment(&mut self, span: Span, text: Span) {
        if !self.declared(EvidenceStream::Comments) {
            return;
        }
        let span = self.clamp(span, "comment");
        let text = self.clamp(text, "comment text");
        self.out.comments.push(CommentSpan { span, text });
    }

    pub fn diagnostic(
        &mut self,
        level: DiagnosticLevel,
        message: impl Into<String>,
        span: Option<Span>,
    ) {
        self.out.diagnostics.push(AdapterDiagnostic {
            level,
            message: message.into(),
            span,
        });
    }

    /// Closes the evidence, and with it the guarantee every consumer of a
    /// selector relies on: no two declarations in this file share an address.
    /// A declaration's `nth` is how many earlier ones share its owner name,
    /// name and signature — zero for nearly all, and the only thing keeping
    /// two apart where the language spells nothing else.
    pub fn finish(mut self) -> FileEvidence {
        let mut seen_targets: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        for import in &mut self.out.imports {
            let n = seen_targets
                .entry(import.target.as_written().to_string())
                .or_insert(0);
            import.nth = *n;
            *n += 1;
        }
        let mut seen: std::collections::HashMap<(Option<SmolStr>, SmolStr, Option<SmolStr>), u32> =
            std::collections::HashMap::new();
        let owner_names: Vec<Option<SmolStr>> = self
            .out
            .declarations
            .iter()
            .map(|d| {
                d.owner
                    .map(|o| self.out.declarations[o.index()].name.clone())
            })
            .collect();
        for (d, owner) in self.out.declarations.iter_mut().zip(owner_names) {
            let key = (owner, d.name.clone(), d.signature.clone());
            let n = seen.entry(key).or_insert(0);
            d.nth = *n;
            *n += 1;
        }
        self.out
    }
}

/// What one file's coverage report states, before any project mapping: hit counts
/// keyed by 1-based line. Test-execution evidence at the WIRE level — what an
/// ingesting extension returns from [`crate::extension::Extension::ingest`] and
/// what the engine maps onto the project once it supplies the sources.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FileRecords {
    /// Instrumented lines → hit count.
    pub lines: std::collections::BTreeMap<u32, u64>,
    /// Function records: (declaration line, hit count).
    pub functions: Vec<(u32, u64)>,
}

/// Every file a report mentions, by the report's own (separator-normalized) path.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CoverageRecords {
    pub files: std::collections::BTreeMap<crate::vocab::ProjectPath, FileRecords>,
}
