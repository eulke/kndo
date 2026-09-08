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
pub mod manifest;
pub mod plugin;
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

/// A one-to-one mapping between two spellings of the same enum — the contract's
/// vocabulary and the wire's, in either direction.
///
/// Every such function is the same code with two type names swapped, which is
/// what `duplicate` says about them when there are more than a couple. The
/// variants are the only fact; the macro writes the rest. `else` supplies the
/// arm a non-exhaustive source needs, so a variant this build cannot spell
/// degrades toward a named neighbour instead of failing to compile.
#[macro_export]
macro_rules! variant_map {
    ($(#[$m:meta])* $vis:vis fn $name:ident($from:ty => $to:ty) { $($v:ident),+ $(,)? }) => {
        $(#[$m])*
        $vis fn $name(value: $from) -> $to {
            match value { $(<$from>::$v => <$to>::$v,)+ }
        }
    };
    ($(#[$m:meta])* $vis:vis fn $name:ident($from:ty => $to:ty) { $($v:ident),+ $(,)? } else $fallback:expr) => {
        $(#[$m])*
        $vis fn $name(value: $from) -> $to {
            match value { $(<$from>::$v => <$to>::$v,)+ _ => $fallback }
        }
    };
}
