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

## 7. False negative: path references in prose

When `internal/perf-baseline.json` replaced `docs/perf-baseline.json`, four Markdown
documents kept pointing at the dead path and nothing flagged them: kndo extracts no
references from prose, so a path-shaped string in documentation participates in no
resolution and can go stale silently. Direction: a lightweight docs adapter (or plugin)
extracting path-shaped tokens from Markdown as `Possible`-confidence references — enough
for a "documentation references a path that no longer exists" hygiene verdict without
pretending prose is code.
