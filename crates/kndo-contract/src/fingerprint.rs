//! Structural fingerprinting: a 32-byte hash of type SHAPE, derived only from source
//! text (macro-captured tokens) plus trait recursion — deliberately no
//! `std::any::type_name` and no `TypeId`, whose output is compiler-dependent. The
//! spike that established the rules, including the cycle guard and the cfg findings,
//! is preserved at `v2/spikes/fingerprint/VERDICT.md`.

use smol_str::SmolStr;
use std::collections::{BTreeMap, HashMap};

/// The derive macro, re-exported under the trait's own name (the serde pattern:
/// macro and trait live in different namespaces, so one `use` imports both).
pub use kndo_contract_fingerprint::ContractFingerprint;

/// Accumulates the structural fold. Atoms are length-prefixed so the byte stream is
/// unambiguous.
pub struct Fold {
    hasher: blake3::Hasher,
    /// Cycle guard: derived types currently on the fold path. Only derived types
    /// participate — containers and leaves cannot cycle, and keeping them off the
    /// stack lets `Vec<A>` nest inside `Vec<B>` without a false back-reference.
    stack: Vec<&'static str>,
}

impl Fold {
    pub fn new() -> Self {
        Fold {
            hasher: blake3::Hasher::new(),
            stack: Vec::new(),
        }
    }

    pub fn atom(&mut self, s: &str) {
        self.hasher.update(&(s.len() as u32).to_le_bytes());
        self.hasher.update(s.as_bytes());
    }

    /// Fold a child type's shape; re-entry of a derived type already on the path
    /// folds a back-reference (itself part of the shape) instead of recursing.
    pub fn child<T: ContractFingerprint + ?Sized>(&mut self) {
        if T::CYCLIC_GUARD && self.stack.contains(&T::TAG) {
            self.atom("<cycle>");
            self.atom(T::TAG);
            return;
        }
        if T::CYCLIC_GUARD {
            self.stack.push(T::TAG);
        }
        self.atom("<type>");
        self.atom(T::TAG);
        T::fold(self);
        if T::CYCLIC_GUARD {
            self.stack.pop();
        }
    }

    pub fn finish(self) -> [u8; 32] {
        *self.hasher.finalize().as_bytes()
    }
}

impl Default for Fold {
    fn default() -> Self {
        Self::new()
    }
}

/// The folding contract. The derive implements it for contract types; leaves and
/// containers are implemented here.
pub trait ContractFingerprint {
    const TAG: &'static str;
    const CYCLIC_GUARD: bool = false;
    fn fold(f: &mut Fold);

    fn fingerprint() -> [u8; 32]
    where
        Self: Sized,
    {
        let mut f = Fold::new();
        f.child::<Self>();
        f.finish()
    }
}

macro_rules! leaf {
    ($($t:ty => $tag:literal),+ $(,)?) => {
        $(impl ContractFingerprint for $t {
            const TAG: &'static str = $tag;
            fn fold(_f: &mut Fold) {}
        })+
    };
}

leaf! {
    u8 => "u8", u16 => "u16", u32 => "u32", u64 => "u64", u128 => "u128",
    i8 => "i8", i16 => "i16", i32 => "i32", i64 => "i64", i128 => "i128",
    bool => "bool", char => "char", f32 => "f32", f64 => "f64",
    String => "String", str => "str",
    SmolStr => "SmolStr(opaque)",
}

macro_rules! container1 {
    ($($outer:ident => $tag:literal),+ $(,)?) => {
        $(impl<T: ContractFingerprint> ContractFingerprint for $outer<T> {
            const TAG: &'static str = $tag;
            fn fold(f: &mut Fold) { f.child::<T>(); }
        })+
    };
}

container1! { Vec => "Vec", Option => "Option", Box => "Box" }

impl<K: ContractFingerprint, V: ContractFingerprint> ContractFingerprint for HashMap<K, V> {
    const TAG: &'static str = "HashMap";
    fn fold(f: &mut Fold) {
        f.child::<K>();
        f.child::<V>();
    }
}

impl<K: ContractFingerprint, V: ContractFingerprint> ContractFingerprint for BTreeMap<K, V> {
    const TAG: &'static str = "BTreeMap";
    fn fold(f: &mut Fold) {
        f.child::<K>();
        f.child::<V>();
    }
}

impl<T: ContractFingerprint, const N: usize> ContractFingerprint for [T; N] {
    const TAG: &'static str = "array";
    fn fold(f: &mut Fold) {
        f.atom(&N.to_string());
        f.child::<T>();
    }
}

impl<A: ContractFingerprint, B: ContractFingerprint> ContractFingerprint for (A, B) {
    const TAG: &'static str = "tuple2";
    fn fold(f: &mut Fold) {
        f.child::<A>();
        f.child::<B>();
    }
}

impl<A: ContractFingerprint, B: ContractFingerprint, C: ContractFingerprint> ContractFingerprint
    for (A, B, C)
{
    const TAG: &'static str = "tuple3";
    fn fold(f: &mut Fold) {
        f.child::<A>();
        f.child::<B>();
        f.child::<C>();
    }
}
