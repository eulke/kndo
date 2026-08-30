//! The kndo contract: every type that crosses a crate boundary — the vocabulary,
//! finding identity, and the evidence adapters report. Adapters, plugins, and
//! frontends compile against this crate, never against the engine; the engine is one
//! more consumer. Shape changes version themselves: `contract_fingerprint()` is a
//! structural hash of everything reachable from [`evidence::FileEvidence`], folded
//! into every cache key, and the `contract_fingerprint_is_intentional` gate makes a
//! shape change a visible diff instead of a silent invalidation hazard.

// Lets the derive macro's generated `::kndo_contract::…` paths resolve inside this
// crate too (the serde pattern).
extern crate self as kndo_contract;

pub mod adapter;
pub mod evidence;
pub mod finding;
pub mod fingerprint;
pub mod subject;
pub mod vocab;

use fingerprint::ContractFingerprint as _;

/// The structural fingerprint of the evidence contract: blake3 over the shape (field
/// names, order, and type shapes, recursively) of [`evidence::FileEvidence`] and
/// everything reachable from it. Changing any of those types changes this value;
/// nothing else does.
pub fn contract_fingerprint() -> [u8; 32] {
    evidence::FileEvidence::fingerprint()
}

pub fn contract_fingerprint_hex() -> String {
    contract_fingerprint()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
