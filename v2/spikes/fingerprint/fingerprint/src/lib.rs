//! Structural fingerprint of a data contract: a 32-byte hash of type SHAPE — field
//! names, field order, field type shapes, recursively — derived only from
//! macro-captured tokens plus trait recursion. Deliberately no `std::any::type_name`
//! and no `TypeId` anywhere: their output is unspecified and compiler-dependent, so a
//! fingerprint built on them would not be stable across rustc versions. This one is
//! stable by construction: every byte folded comes from source text.

use std::collections::{BTreeMap, HashMap};

/// Accumulates the structural fold. Every atom is length-prefixed so the byte stream
/// is unambiguous (no separator injection).
pub struct Fold {
    hasher: blake3::Hasher,
    /// Cycle guard: tags of derived types currently being folded. Only derived types
    /// participate (CYCLIC_GUARD = true) — containers and leaves cannot cycle, and
    /// keeping them off the stack lets `Vec<A>` nest inside `Vec<B>` without a false
    /// back-reference.
    stack: Vec<&'static str>,
}

impl Fold {
    pub fn new() -> Self {
        Fold { hasher: blake3::Hasher::new(), stack: Vec::new() }
    }

    pub fn atom(&mut self, s: &str) {
        self.hasher.update(&(s.len() as u32).to_le_bytes());
        self.hasher.update(s.as_bytes());
    }

    /// Fold a child type's shape. On re-entry of a derived type already on the fold
    /// stack (a recursive contract type), folds a back-reference marker instead of
    /// recursing — the recursion terminates and the cycle itself is part of the shape.
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

pub trait ContractFingerprint {
    /// Stable type tag. For derived types this is the type's declared name (source
    /// text); the defining module path is folded as data inside `fold`, not here.
    /// Known edge, accepted for the spike: two same-named derived types meeting on one
    /// fold path would alias in the cycle guard — the contract crate enforces unique
    /// type names (one flat module tree) so this cannot arise there.
    const TAG: &'static str;
    /// True only for derived (user) types — the only ones that can recurse.
    const CYCLIC_GUARD: bool = false;
    /// Fold this type's shape (not its values) into the hasher.
    fn fold(f: &mut Fold);

    fn fingerprint() -> [u8; 32]
    where
        Self: Sized,
    {
        let mut f = Fold::new();
        f.child::<Self>();
        f.finish()
    }

    fn fingerprint_hex() -> String
    where
        Self: Sized,
    {
        Self::fingerprint().iter().map(|b| format!("{b:02x}")).collect()
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
    u8 => "u8", u16 => "u16", u32 => "u32", u64 => "u64", u128 => "u128", usize => "usize",
    i8 => "i8", i16 => "i16", i32 => "i32", i64 => "i64", i128 => "i128", isize => "isize",
    bool => "bool", char => "char", f32 => "f32", f64 => "f64",
    String => "String", str => "str",
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
