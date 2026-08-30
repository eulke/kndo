//! What an adapter reports about one file: evidence — raw material weighted by
//! confidence, prior to all judgment. Analyses weigh evidence and return verdicts;
//! without the evidence they need, they abstain.
//!
//! Adapters never build [`FileEvidence`] by hand: extraction writes through
//! [`EvidenceSink`], which validates at the call site and returns ids — attaching
//! metrics or membership is by [`DeclarationId`], so "must byte-match another span" style
//! conventions have nothing to exist for. The engine reads the finished value.

use crate::fingerprint::ContractFingerprint;
use crate::vocab::{Confidence, Span};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "kebab-case")]
pub enum SymbolKind {
    Function,
    Method,
    Type,
    Constant,
    Variable,
    Module,
    Other,
}

/// Whether a declaration is nameable beyond its file. The visibility *ladder* (the
/// per-language rungs between private and public) arrives with the AdapterSpec work;
/// this is the half every language shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "lowercase")]
pub enum Reach {
    Private,
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
}

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
    pub span: Span,
}

/// Where an import points. The specifier string as written lives inside the variant
/// that needs it; resolution happens engine-side through the adapter's resolver.
#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "lowercase")]
pub enum ImportTarget {
    /// A relative specifier (`./util`, `../lib/x`), as written.
    Relative(SmolStr),
    /// A package/bare specifier (`react`, `lodash/fp`), as written.
    Package(SmolStr),
}

#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
pub struct ImportBinding {
    pub imported: SmolStr,
    pub local: SmolStr,
}

/// The FORM of an import is one closed choice — not a bag of independent booleans
/// whose illegal combinations need documenting.
#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "kebab-case")]
pub enum ImportShape {
    Bindings(Vec<ImportBinding>),
    Namespace { local: SmolStr },
    SideEffect,
    Reexport(Vec<ImportBinding>),
    TypeOnly(Vec<ImportBinding>),
}

#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
pub struct Import {
    pub target: ImportTarget,
    pub shape: ImportShape,
    pub span: Span,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "lowercase")]
pub enum RootKind {
    Production,
    Test,
    Tooling,
}

/// What a root anchors: the whole file, or one declaration — by id.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ContractFingerprint)]
#[serde(rename_all = "lowercase")]
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

/// The finished evidence for one file. Built through [`EvidenceSink`]; read
/// everywhere.
#[derive(Debug, Clone, Serialize, Deserialize, ContractFingerprint)]
pub struct FileEvidence {
    pub declarations: Vec<Declaration>,
    pub references: Vec<Reference>,
    pub imports: Vec<Import>,
    pub roots: Vec<Root>,
    pub comments: Vec<CommentSpan>,
    pub metrics: Vec<(DeclarationId, FunctionMetrics)>,
    pub diagnostics: Vec<AdapterDiagnostic>,
}

/// The write side of extraction. Validates as evidence arrives — a span outside the
/// file is clamped and reported as a diagnostic (extraction degrades, never fails) —
/// and hands back the ids that make attachment misuse unrepresentable.
pub struct EvidenceSink {
    file_len: u32,
    out: FileEvidence,
}

impl EvidenceSink {
    pub fn new(file_len: u32) -> Self {
        EvidenceSink {
            file_len,
            out: FileEvidence {
                declarations: Vec::new(),
                references: Vec::new(),
                imports: Vec::new(),
                roots: Vec::new(),
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
            reach,
            owner: None,
        });
        id
    }

    /// Membership by id: no name lookup, no span matching, nothing to mis-resolve.
    pub fn member_of(&mut self, member: DeclarationId, owner: DeclarationId) {
        debug_assert_ne!(member, owner, "a declaration cannot own itself");
        debug_assert!(owner.index() < self.out.declarations.len());
        self.out.declarations[member.index()].owner = Some(owner);
    }

    /// Metrics attach to the declaration they describe — by id. (v1 matched by name
    /// against last-wins symbol tables and reported a method as a clone of itself.)
    pub fn metrics(&mut self, of: DeclarationId, m: FunctionMetrics) {
        debug_assert!(of.index() < self.out.declarations.len());
        self.out.metrics.push((of, m));
    }

    pub fn reference(&mut self, name: impl Into<SmolStr>, kind: RefKind, span: Span) {
        let span = self.clamp(span, "reference");
        self.out.references.push(Reference {
            name: name.into(),
            kind,
            span,
        });
    }

    pub fn import(
        &mut self,
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
        });
    }

    pub fn root(&mut self, target: RootTarget, kind: RootKind, confidence: Confidence) {
        if let RootTarget::Declaration(id) = target {
            debug_assert!(id.index() < self.out.declarations.len());
        }
        self.out.roots.push(Root {
            target,
            kind,
            confidence,
        });
    }

    pub fn comment(&mut self, span: Span, text: Span) {
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

    pub fn finish(self) -> FileEvidence {
        self.out
    }
}
