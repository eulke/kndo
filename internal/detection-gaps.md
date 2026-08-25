# Known detection gaps

Gaps in kndo's own detection, catalogued from a full triage of the self-check corpus (every
finding on kndo's own repository classified against the source as genuine or
false). Each entry records the root cause, where to see it in this codebase, and the
direction a fix would take — so the next person hitting one of these recognizes it as a
*known* limit with a design sketch, not fresh noise. Genuine-verdict policy questions
(what the analyses *should* claim) belong in RFC 0005; this file is strictly about recall
and precision mechanics.

The inline `kndo:allow` pragmas and `kndo.toml` `[[rule]]` entries in this repository that
cite this file are the acknowledged instances of these gaps.

## 1. WASM-boundary reachability (host imports)

The functions `crates/kndo-core/src/plugin_host.rs` hands to the component runtime are
called only *through* the WASM boundary: the caller is generated bridge code inside
wasmtime, not any source file the graph walks, so no reference edge can exist and the
functions read as `untested`/`unused` — while `plugin_compliance.rs` builds and runs a real
guest against exactly these functions every CI run. Root cause: reachability's evidence
universe is source references; a host-side ABI surface is reached from outside it.
Direction: an adapter-declared (or plugin-declared) "externally invoked" fact for
host-import surfaces, the same shape `annotate_symbols`' externally-consumed marking
already has — the mechanism exists, what's missing is a producer that recognizes the
boundary. Acknowledged with a file pragma in `plugin_host.rs`.

## 2. Proc-macro derive dispatch (rkyv)

`crates/kndo-core/src/rkyv_support.rs`'s wrapper impls (`SmolStrAsString`) are invoked by
code a derive macro generates (`#[rkyv(with = …)]`): the call sites exist only in the
macro expansion, which extraction never sees. The attr-ident scan keeps the *type* alive
(an identifier inside an attribute is a read), but the impl *members* the generated code
calls have no incoming edges. Root cause: dispatch through generated code is invisible to
source-level extraction by construction. Direction: adapter-curated knowledge per derive
ecosystem — "a type named in `#[rkyv(with = …)]` has its trait-impl members
machinery-invoked," the same curated-list pattern as `implicitly_invoked` for operator
overloads/formatting hooks. Acknowledged with a file pragma in `rkyv_support.rs`.

## 3. Field-access recall on inferred-typed locals

A struct whose fields are only ever read through a local of *inferred* type never shows
field usage: in `let entry = parse_entry(..); entry.path`, the receiver's type comes from
the callee's return type, which the extraction-side `TypeEnv` doesn't chase — so the field
reference lands in the duck fallback (or nowhere), and `internal-only` sees "no use
requires this visibility". Live examples in this repo: `gitutil::TreeEntry`
(consumed in `discovery.rs` through locals), `rollup::DirRollup`, and the plugin sink item
types (`ContributedRoot`/`ContributedEdge`, read via `root_sink.items` in `graph.rs`).
Direction: propagate declared types through let-bindings whose initializer has a known
shape (a call to a function with a declared return type, a struct literal) — bounded type
propagation, not inference. Acknowledged with declaration pragmas at the affected types.

## 4. Recall asymmetry — the regression corpus

The mirror image of #3, kept as a regression corpus for whoever implements it:
`gitutil::BlobFetcher::spawn` has the same consumed-through-inferred-locals shape and
draws **no** finding today (its usage happens to resolve through a path the fallback
catches). Any change to reference recall should check both directions on these sites — the
goal is symmetric behavior, not moving the false positives around.

## 5. Duplicate-group label ambiguity

A structural-clone group's instance labels are `path#Owner.name` selectors. Two identical
methods with the same name on the same owner type but in *different impl blocks* (the
generated-vs-hand-written split of `HostViewData` members in `kndo-plugin-api`) produce
two indistinguishable labels — the finding is correct, but the reader cannot tell which
impl block each instance lives in. Direction: selectors (and `related` entries) could
carry the impl block's span or a disambiguating qualifier when `qualified_name` collides
within one file. Cosmetic; recorded so the "same label twice" report isn't mistaken for a
detector bug.

## 6. The kndo-core module cycle (legal structure, silent by policy)

`kndo-core`'s module graph contains one large file-level import cycle — dozens of files
(engine ↔ graph ↔ analysis ↔ suppression all reference each other's types). Under Rust's
`Idiomatic` cycle tolerance this emits **nothing**: an idiomatic cycle is true information
about legal structure, and kndo does not dress information up as a defect (tolerated
cycles don't feed health's cycles axis either). Listed here so the *absence* of a cyclic
finding on this repo isn't triaged as a detector gap — the structure is real and remains
visible through the graph itself (`kndo query`/doctor), just never as a finding.

## 7. Intra-package visibility leaks (narrower than the four-bucket ladder)

`private-type-leak` accuses only items whose visibility rung is `surface_transitive` — items
that genuinely cross the package boundary. An item visible to a *sibling module* that names a
type private to its own module is a real leak by the letter of the language's rules, and is
now silent: Rust's `pub(super) fn unset_waker` in tokio's `task::state`, returning a
`state.rs`-private alias that its sibling `task::harness` caller cannot spell.

Root cause: the region such an item is visible to sits strictly between `File` and `Package`,
and `VisibilityScope` has no rung there — adapters over-approximate a top-level `pub(super)`
as `pub(crate)` for exactly that reason (`kndo-adapter-rust`'s `restriction_level`). With that
approximation the model cannot tell the tokio case apart from the far more common inverse,
where every caller that can reach the item can also name the type (ripgrep's
`flags::parse::lookup` against the private `Flag` trait, serde's `de::deserialize_custom`
against `Parameters`) — a field audit found the harmless shape three times for each real one.
RFC 0012 §2 is explicit about which way to degrade when the model cannot prove the accusation,
so the gate stays until the scope exists.

Direction: a module-subtree scope in the ladder (`VisibilityScope::Module`, anchored on the
declaring file's `unit` — Rust already keys units on the module path, RFC 0012 §8), plus
adapters emitting `pub(super)`/`pub(in path)` as that rung instead of collapsing it upward.
`scope_contains_site` gains one arm; the containment comparison this analysis already performs
then decides the case exactly, with no gate needed.

## 8. A brace-imported submodule is not a usable qualifier

`use crate::internals::{attr, check, Ctxt, Derive};` followed by `check::check(cx, …)` binds
nothing: `check` reads as dead, and so does everything only it reaches. In serde this killed
`internals::check` and the whole family of `check_*` helpers it calls.

Extraction is correct and was verified — the Rust adapter emits the import with
`specifier: "crate::internals"` and bindings `[attr, check, Ctxt, Derive]`, plus the reference
`check` with `scope_context: Some("check")`. The gap is in resolution: assembly registers a
qualifier for the specifier's own last segment (`internals` → `internals/mod.rs`) but not for
each brace member, so the qualifier `check` matches nothing and the qualified reference falls
to the duck fallback, which finds no member of that name. The one-hop that would close it is
real and available in the data: `internals/mod.rs` itself imports `check.rs` under the name
`check` (its `mod check;`), so the chain is *binding on the target's own import table*.

Direction, and the reason it is not simply done: the obvious shortcut — extend the specifier
with the binding name and re-resolve `crate::internals::check` — requires the core to know
that `::` joins path segments, which is exactly the language knowledge the ignorance rule
forbids it. (One such separator is already hardcoded in `assemble.rs`'s qualifier fallback;
that is a wart to remove, not a precedent to widen.) The language-blind version needs no
separator at all: resolve every file's imports into a `file → (binding name → target file)`
map first, then let a qualifier that binds to file F and names one of F's own import bindings
follow it one hop. That splits phase 3b's single per-file pass into an import pass and a
reference pass — a restructure, not a patch, which is why it is recorded here rather than
attempted in passing.

Worth weighing before scheduling: `use path::{submodule, Type}` is a very common Rust shape,
so this likely suppresses real recall across every Rust codebase, including kndo's own.

## 9. False negative: path references in prose

When `internal/perf-baseline.json` replaced `docs/perf-baseline.json`, four Markdown
documents kept pointing at the dead path and nothing flagged them: kndo extracts no
references from prose, so a path-shaped string in documentation participates in no
resolution and can go stale silently. Direction: a lightweight docs adapter (or plugin)
extracting path-shaped tokens from Markdown as `Possible`-confidence references — enough
for a "documentation references a path that no longer exists" hygiene verdict without
pretending prose is code.
