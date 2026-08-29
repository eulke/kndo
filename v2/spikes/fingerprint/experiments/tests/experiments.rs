//! Each test is one question from the spike spec; VERDICT.md summarizes the answers.

use experiments::*;
use fingerprint::{ContractFingerprint, Fold};
use fingerprint_derive::ContractFingerprint;

#[test]
fn e1_baseline_is_stable_and_printable() {
    let a = FileEvidence::fingerprint_hex();
    let b = FileEvidence::fingerprint_hex();
    assert_eq!(a, b, "two computations in one process must agree");
    println!("BASELINE {a}");
}

mod renamed_field {
    use super::*;
    #[derive(ContractFingerprint)]
    pub struct Span {
        pub begin: u32, // was: start
        pub end: u32,
    }
}

#[test]
fn e2_field_rename_changes_the_hash() {
    assert_ne!(Span::fingerprint(), renamed_field::Span::fingerprint());
}

mod retyped_field {
    use super::*;
    #[derive(ContractFingerprint)]
    pub struct Span {
        pub start: u64, // was: u32
        pub end: u32,
    }
}

#[test]
fn e3_field_type_change_changes_the_hash() {
    assert_ne!(Span::fingerprint(), retyped_field::Span::fingerprint());
}

mod reordered_fields {
    use super::*;
    #[derive(ContractFingerprint)]
    pub struct Span {
        pub end: u32, // order swapped: rkyv layout is order-sensitive, so shape is too
        pub start: u32,
    }
}

#[test]
fn e4_field_reorder_changes_the_hash() {
    assert_ne!(Span::fingerprint(), reordered_fields::Span::fingerprint());
}

#[derive(ContractFingerprint)]
pub struct SpanRenamed {
    pub start: u32,
    pub end: u32,
}

#[test]
fn e5_type_rename_changes_the_hash() {
    // Deliberate: a type rename is a contract change (WIT records, docs, and
    // serde-facing names hang off it), so the type's own name participates.
    assert_ne!(Span::fingerprint(), SpanRenamed::fingerprint());
}

mod grown_enum {
    use super::*;
    #[derive(ContractFingerprint)]
    pub enum Confidence {
        Possible,
        Probable,
        Certain,
        Verified, // new variant
    }
}

#[test]
fn e6_added_variant_changes_the_hash() {
    assert_ne!(Confidence::fingerprint(), grown_enum::Confidence::fingerprint());
}

#[derive(ContractFingerprint)]
pub struct Wrapper<T> {
    pub inner: Vec<T>,
}

#[test]
fn e7_generic_instantiations_fold_the_argument_shape() {
    assert_ne!(Wrapper::<u32>::fingerprint(), Wrapper::<u64>::fingerprint());
    assert_eq!(Wrapper::<u32>::fingerprint(), Wrapper::<u32>::fingerprint());
    assert_ne!(Wrapper::<Span>::fingerprint(), Wrapper::<SpanRenamed>::fingerprint());
}

#[test]
fn e8_recursive_type_terminates_and_its_recursion_is_shape() {
    // TypeExprLike contains Vec<TypeExprLike>: the trait recursion must terminate via
    // the cycle guard (this test completing at all proves it) …
    let recursive = TypeExprLike::fingerprint();

    // … and the back-reference must itself be part of the shape: an identical enum
    // whose inner list holds a leaf instead of Self must differ.
    #[derive(ContractFingerprint)]
    pub enum TypeExprFlat {
        Named(SmolLike, Vec<SmolLike>),
        Param(usize),
        Unknown,
    }
    // Same variant names, same field counts — only the self-reference differs. Note
    // TAG differs too (TypeExprLike vs TypeExprFlat); isolate the recursion by
    // comparing against a same-named type in a sibling module below.
    assert_ne!(recursive, TypeExprFlat::fingerprint());
}

mod flat_twin {
    use super::*;
    // Same NAME as the recursive one, same variants — but no self-reference. Isolates
    // "the cycle is shape" from "the name is shape".
    #[derive(ContractFingerprint)]
    pub enum TypeExprLike {
        Named(SmolLike, Vec<SmolLike>),
        Param(usize),
        Unknown,
    }
}

#[test]
fn e8b_self_reference_alone_distinguishes_shapes() {
    assert_ne!(TypeExprLike::fingerprint(), flat_twin::TypeExprLike::fingerprint());
}

#[derive(ContractFingerprint)]
pub enum BoxCycle {
    Leaf(u32),
    Node(Box<BoxCycle>),
}

#[test]
fn e9_box_recursion_terminates() {
    let _ = BoxCycle::fingerprint();
}

#[test]
fn e10_third_party_leaf_is_opaque_and_stable() {
    #[derive(ContractFingerprint)]
    pub struct HoldsLeaf {
        pub name: SmolLike,
    }
    let _ = HoldsLeaf::fingerprint(); // compiles + folds via the manual leaf impl
}

mod cfg_experiment {
    use super::*;

    // cfg(any()) is always FALSE: the field is stripped before the derive ever sees
    // it — the derive cannot detect this half, which is why it exists as a shape risk.
    #[derive(ContractFingerprint)]
    pub struct WithCfgFalse {
        pub always: u32,
        #[cfg(any())]
        pub gated: u32,
    }

    #[derive(ContractFingerprint)]
    pub struct JustAlways {
        pub always: u32,
    }
}

#[test]
fn e11a_a_surviving_cfg_field_is_a_compile_error() {
    // Measured finding (rustc 1.94.1), opposite of the pre-spike hypothesis: a cfg
    // attribute on a field that survives evaluation IS visible to the derive, so the
    // guard fires at compile time on any platform where the field exists.
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/cfg_gated_field.rs");
}

#[test]
fn e11b_a_cfg_false_field_is_invisible_to_the_derive() {
    use cfg_experiment::*;
    // The cfg-false field is gone pre-derive: the fold body equals the field-less
    // twin's (TAGs differ, so compare the bodies directly). This is the residual risk
    // — a field cfg'd off on EVERY build platform never errors and never folds — and
    // the reason CI compares fingerprints across targets as well.
    let mut a = Fold::new();
    WithCfgFalse::fold(&mut a);
    let mut b = Fold::new();
    JustAlways::fold(&mut b);
    assert_eq!(a.finish(), b.finish());
}

mod sibling {
    use fingerprint_derive::ContractFingerprint;
    #[derive(ContractFingerprint)]
    pub struct Twin {
        pub x: u32,
    }
}

mod sibling2 {
    use fingerprint_derive::ContractFingerprint;
    #[derive(ContractFingerprint)]
    pub struct Twin {
        pub x: u32,
    }
}

#[test]
fn e12_module_path_participates() {
    // Same name, same shape, different defining module: distinct fingerprints — a
    // type move between modules is a (cache-invalidating) contract change.
    assert_ne!(sibling::Twin::fingerprint(), sibling2::Twin::fingerprint());
}
