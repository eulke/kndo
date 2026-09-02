//! The contract-like type tree the experiments fingerprint: shaped after the real
//! contract (FileEvidence: vecs of declarations/references/imports, an import-shape
//! enum, metrics, a recursive type expression), so the spike measures the thing the
//! mechanism is for.

use fingerprint::{ContractFingerprint, Fold};
use fingerprint_derive::ContractFingerprint;

/// Third-party leaf stand-in (SmolStr-like): a type the derive cannot reach, given a
/// manual impl with an explicit stable tag — the escape hatch external types need.
pub struct SmolLike(pub String);

impl ContractFingerprint for SmolLike {
    const TAG: &'static str = "SmolLike(opaque)";
    fn fold(_f: &mut Fold) {}
}

#[derive(ContractFingerprint)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

#[derive(ContractFingerprint)]
pub enum Confidence {
    Possible,
    Probable,
    Certain,
}

#[derive(ContractFingerprint)]
pub enum TypeExprLike {
    Named(SmolLike, Vec<TypeExprLike>),
    Param(usize),
    Unknown,
}

#[derive(ContractFingerprint)]
pub struct Declaration {
    pub name: SmolLike,
    pub span: Span,
    pub exported: bool,
    pub yields: TypeExprLike,
}

#[derive(ContractFingerprint)]
pub enum ImportShape {
    Bindings(Vec<SmolLike>),
    Namespace { alias: SmolLike, opaque: bool },
    SideEffect,
}

#[derive(ContractFingerprint)]
pub struct FunctionMetrics {
    pub cyclomatic: u32,
    pub loc: u32,
    pub fingerprints: Vec<u64>,
}

#[derive(ContractFingerprint)]
pub struct FileEvidence {
    pub declarations: Vec<Declaration>,
    pub imports: Vec<(SmolLike, ImportShape)>,
    pub metrics: Vec<FunctionMetrics>,
    pub confidence: Option<Confidence>,
    pub spans: [Span; 2],
}
