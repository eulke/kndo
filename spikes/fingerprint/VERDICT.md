# Fingerprint spike — verdict

**VIABLE.** A `#[derive(ContractFingerprint)]` built purely from macro-captured source
text plus trait recursion produces a stable 32-byte structural hash and can replace the
hand-bumped shape-version constant. 14 tests green (13 experiments + 1 compile-fail);
baseline over a contract-shaped tree (`FileEvidence` → declarations, recursive type
exprs, an import-shape enum, metrics, tuples, arrays, options):

```
34cb63e5a2277cb356b214ecb5ac9044e7279713a493f558025ec95a56cf4774
```

byte-identical across two separate compilations in different target dirs.
Toolchain: rustc 1.94.1. Crates: syn 2, quote, blake3; trybuild for the compile-fail.

## The folding rules (this is the v2 spec)

Every atom is length-prefixed (no separator injection). A type folds as:

1. `<type>` marker + its `TAG` (the type's declared name, from `stringify!`).
2. Derived types then fold `module_path!()` (source text, expands at the definition
   site), the kind marker (`struct`/`enum`), and per field: `field` + field name (or
   tuple index) + the field type's fold, recursively via
   `Fold::child::<FieldType>()`. Enums fold `variant` + variant name before each
   variant's fields. Declaration ORDER is part of the shape (deliberate: rkyv layout is
   order-sensitive).
3. Leaves (primitives, `String`, `str`) fold only their tag. Containers (`Vec`,
   `Option`, `Box`, maps, arrays with their length, tuples) fold tag + child shapes.
   Third-party types get a manual leaf impl with an explicit stable tag
   (`SmolLike(opaque)` is the reference).
4. **No `std::any::type_name`, no `TypeId`, anywhere** — their output is
   compiler-version-dependent; everything folded here is source text, so stability
   across rustc versions holds by construction.

Confirmed sensitivities (each a test): field rename, field type change, field reorder,
type rename (deliberate: WIT records and serde names hang off it), added enum variant,
generic argument shape (`Wrapper<u32>` ≠ `Wrapper<u64>`, and the bound
`T: ContractFingerprint` is added by the derive), defining module
(`sibling::Twin` ≠ `sibling2::Twin` — moving a type between modules is a
cache-invalidating contract change).

## The hard finding: recursion needs a cycle guard, and the guard is cheap

A shape-level recursion on `enum TypeExpr { Named(_, Vec<TypeExpr>), … }` recurses
forever with a naive trait walk. The guard: `Fold` keeps a stack of **derived-type**
tags; re-entering a type already on the stack folds a `<cycle>` + tag back-reference
instead of recursing. Two properties proven by test: it terminates (`Box<Self>`
included), and the back-reference is itself shape — a same-named twin whose inner list
holds a leaf instead of `Self` fingerprints differently. Only derived types join the
stack (containers/leaves cannot cycle), so `Vec<A>` inside `Vec<B>` never
false-triggers. Accepted edge: two same-named derived types on one fold path would
alias in the guard — the real contract crate keeps unique type names (enforceable with
a trivial test there).

## The cfg finding — opposite of the hypothesis, and better

Measured on rustc 1.94.1: a `#[cfg]` attribute on a field that **survives** evaluation
is still visible to the derive, so the derive refuses it with a `compile_error`
("contract fields are unconditional") — the no-platform-dependent-shape policy is
compile-time-enforced on every platform where the field exists (trybuild test). The
residue: a field whose cfg is **false** on the building platform is stripped before the
derive sees anything (proven: its fold body equals the field-less twin's), so a field
cfg'd off on *every* CI platform would silently not participate. Mitigation for the
real crate: the cross-target job compares `CONTRACT_FINGERPRINT` across the CI matrix.

## Gaps and risks for adoption in `kndo-contract`

- **serde/rkyv attributes are invisible to the shape.** `#[serde(rename)]` or a changed
  `#[rkyv(with = …)]` alters the wire/disk layout without moving this fingerprint. The
  real derive should additionally fold the *tokens* of a small allowlist of layout-
  affecting attributes (`serde`, `rkyv`) on the type and its fields. Straightforward
  (they're in the same token stream); left out of the spike deliberately.
- **The rkyv crate version still needs folding into the `CacheEnvelope`** — binary
  layout depends on it and no source-text hash can see it (already a decision).
- Lifetimes/references: untested; the contract owns its data (no borrowed fields), and
  the contract crate should enforce that by convention.
- `usize`/`isize` fold as themselves; if any persisted contract type ever contains one,
  it is platform-width — forbid them in persisted types instead (a leaf-impl choice:
  simply don't provide the impl in the real crate).

## Adoption sketch

`kndo-contract` re-exports the derive; `FileEvidence` and everything reachable derives
it; `CONTRACT_FINGERPRINT` is `FileEvidence::fingerprint()` computed once at startup
and folded into every evidence cache key and the graph key. The two deliberate knobs
(`semantics_version!` per adapter, `GRAPH_SEMANTICS_VERSION`) stay; every shape
constant disappears.
