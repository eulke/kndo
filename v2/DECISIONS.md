# Decisions

Append-only. One dated entry per decision, with the measurement or incident that backs
it. Code never cites entries by number — an entry is a record, not a normative document.
Inherited v1 decisions carry the evidence that earned them; v1 commit hashes refer to
this repository's history.

## 2026-08-29 — Greenfield in place, full scope, on this repository

The v2 replaces the v1 at the root: cost accepted in exchange for root-cause fixes. The
window that makes this cheap is now — pre-1.0, nothing published, no external users —
and closes at the first release. Owner directives: (1) the plan executes at full scope;
a milestone, gate, or design piece is cut only by explicit owner decision recorded here;
(2) no new repository — the rebuild happens in this repo, and v1's tree becomes the
read-only quarry being harvested (fixtures, oracle, decisions).

## 2026-08-29 — Product name stays `kndo`

Verified today: npm `kndo` still squatted by an unrelated package (`kndo-cli` free, the
shim keeps it). crates.io/Homebrew unreachable through this session's proxy; the
2026-08-18 check (crates.io free) stands as most recent — re-verify from an unproxied
machine before M0 reserves names.

## 2026-08-29 — The adapter payload is called *evidence*, not *facts*

`FileEvidence`, `ManifestEvidence`, `EvidenceSink`, the `evidence/` cache, the
`file-evidence` WIT record. Rationale: completes the register the product already used
(analyses return verdicts or abstain; "never falsely accuse"; plugin edges are "liveness
evidence") and fixes two flaws of "facts" — weak evidence is a natural concept where a
"possible fact" is an oxymoron, and every new field must answer *evidence for which
verdict?*. Alternatives weighed: Observations (honest, no metaphor), Extraction
(neutral), Index (collides with graph indexes), Claims (collides with file claiming).

## 2026-08-29 — Naming audit outcomes

`RunOutcome` is the gate result (`Pass | FailFindings | FailBudget | Refused`), owning
`exit_code()`; it absorbed a redundant `Outcome` enum — `analyze()` returns
`Result<Snapshot, Refusal>`. `Verdict` stays reserved for the analysis level
(`Judged | Abstained`). `Snapshot` names only the immutable in-memory value; the cache
artifact is a *graph cache entry*. `RunMode` restored (a `Scope` rename collided with
`VisibilityScope`). `ImportSpecifier` (the ESM standard term) ends the double "spec"
with `AdapterSpec`. Micros: `PluginContext`, `DiagnosticCode`/`DiagnosticLevel`,
`kndo-sdk` (serves adapter authors too), `GRAPH_SEMANTICS_VERSION`.

## 2026-08-29 — Cache versioning: one derived fingerprint, two deliberate knobs

`CONTRACT_FINGERPRINT` is derived structurally from the contract types (spike verdict in
`spikes/fingerprint/`); the deliberate knobs are per-adapter `semantics_version!` and
`GRAPH_SEMANTICS_VERSION`. The `CacheEnvelope` also folds the rkyv version — binary
layout depends on it and the textual fingerprint cannot see it. Evidence: v1 kept seven
hand-bumped constants and recorded the incident — twelve hand edits for two facts in one
day, "the right knob existed and I did not find it" (commit 6883e4d), with silent
under-invalidation as the failure mode.

## 2026-08-29 — The oracle bar: same-or-better, every difference explained

v1's findings over the pinned corpus (`corpus/corpus.toml`, `oracle/`) are the reference
baseline. Every v2 milestone that emits findings diffs against it; a difference is
either an explained improvement or a regression to fix. Set before writing v2 so the
rewrite is measured against reality from day one.

---

# Inherited from v1 (decision → evidence that earned it)

## Rust core, tree-sitter parsing, single static binary, zero-config

The <500 ms warm budget ruled out runtimes; tree-sitter gave error-tolerant parsing with
one mental model (its syntactic precision ceiling is why edges carry `Confidence`).
Spike measured 73 ms warm composite against the 500 ms budget (6.8× headroom); mmap CSR
graph load 1.1 ms for 1M edges. Defaults are part of the contract; `doctor` is the
mandatory companion to auto-detection.

## The ignorance rule

Core never names a language; language knowledge arrives as adapter data. Held for the
entire v1 without exception — the one architectural rule that never produced an
incident. v2 adds the governance its descriptor lacked: a capability ships with a
default, a named core consumer, and a conformance case, or not at all.

## Determinism as a hard contract

Byte-identical output at any thread count on any machine. The only large v1 area with
zero post-hoc bugs: two-phase id assignment, deterministic reduce, sorted tie-breaks,
fuel instead of wall-clock deadlines (epoch_deadline rejected, not deferred). Kept
whole, including its costs (no ambient runtime queries; no wall-clock adaptivity).

## Zero false positives as a shipping bar; abstention; degrade-toward-keep-alive

A rule that cannot reach zero FP does not ship. An analysis missing its required input
abstains (a first-class verdict surfaced in output) instead of guessing; every fallback
degrades toward keeping code alive, never toward accusing it. The dogfood gate pairs
with "zero means measured, not merely unjudged".

## Two-tier extensibility; WASM components; plugin containment

First-party compiled in, third-party as sandboxed WASM components (no stable Rust ABI;
dlopen breaks the single-binary story). Plugin identity is the coordinate
(github.com/owner/repo — nothing to squat). Plugin findings are namespaced + advisory;
plugin edges are liveness evidence, never architecture evidence; health structurally
excludes them. Fuel + 256 MiB memory ceiling; every guest failure degrades to "this
component contributed nothing this round".

## Suppression marks, never deletes; baseline only shrinks; deltas are derived

The full finding set is computed as if no pragma existed, then marked — this makes
"actively suppressing" and "stale" mutually exclusive by construction. The committed
baseline never grows automatically. Diff mode reports derived effects
(after − before ∪ before − after) so fixes show as positive deltas.

## One producer for the release artifact; nothing unverified until a tag

`xtask package` owns the artifact definition; every channel is checked against it (not
against each other); CI installs from its own artifact including the corrupted-checksum
path. Earned by the incident where three of four channels were broken at once and
nothing could have said so before a tag (commit 44e89ee).

## Version-dependent language data is generated, never hand-maintained

Stdlib module lists are generated at development time (`gen-stdlib`); the ambient
installed runtime is never queried at analysis time (determinism).

---

# v2 design decisions (from the redesign report)

## The contract is a minimal crate; adapters never see the core

`kndo-contract` holds vocabulary, evidence, findings, ids, spans, the fingerprint
derive. Adapters depend on contract + toolkit only. Evidence: v1 adapters compiled
against the whole 43k-line core, and the third-party-adapter promise was never
demonstrated with a real language.

## Adapters write through `EvidenceSink`; metrics attach by `DeclId`

The sink validates invariants at the call site and returns ids, killing the
match-by-span convention (v1's metrics-by-name collapse: a method reported as a clone of
itself — audit commit 73040fd). Same symmetry as the plugin sinks that worked.

## `Subject` enum; identity is typed

`subject_kind`, location, and `FindingId` derive from one closed enum;
`SymbolSelector { Free, Member }` replaces the "Owner.name" string convention (v1:
formatted/parsed at 6+ sites; finding_id was five positional &strs for seven days with a
shipped missort — commit ffcce56).

## Session/Snapshot: the engine produces immutable values

`Session` owns lock/cache/effects; `Snapshot` is immutable and Send+Sync; CLI, serve,
MCP and LSP are loops over the same value. Resolves v1's structural conflict between
"no daemon by design" and the serve/LSP roadmap without touching determinism.

## Spans are byte offsets; lines/columns are render

tree-sitter yields bytes for free; overlap/containment become arithmetic; a central
line index renders line/col at the edge.

## Suppression grammar lives in core

Adapters report comment spans as evidence; core parses `kndo:allow` centrally — WASM and
non-tree-sitter adapters get suppression for free. v1 kept the grammar in the toolkit
and the validation in core, so a WASM adapter had to reimplement the grammar.

## Config keys are declared once, with their merge policy

A registry generates the parser, the init template, the docs table, and the live-tables
list. Evidence: v1's per-field precedence (union vs override vs clamp) lived only in doc
comments, and every hand-maintained mirror it had was guarded by a test that existed
because the mirror had already drifted.

## Gates are auto-enumerated; the toolchain is pinned; reality is the harness

`kndo-gates` registers gates by macro; workflow fragments and docs are generated from
the registry (v1: CLAUDE.md list ↔ ci.yml steps edited "together", plus a named gate
that had never once passed in CI). `rust-toolchain.toml` pins local == CI (v1 paid the
1.94-vs-1.98 gap twice, three pushes each). Foreign corpus nightly; dogfood from day 1;
every promised target compiles in CI before it is promised.

## 2026-08-29 — Fingerprint spike: VIABLE, with two measured findings

The structural-fingerprint derive works from source text alone (no type_name/TypeId):
baseline stable across separate compilations, all sensitivities confirmed by test
(rename/retype/reorder/variant/generic/module — 14 tests). Finding 1: shape-level
recursion needs a cycle guard (a fold-stack of derived-type tags emitting a
back-reference), and the back-reference is itself shape. Finding 2, opposite of the
hypothesis: a surviving `#[cfg]` field attribute IS visible to the derive on rustc
1.94.1, so "contract fields are unconditional" is compile-time-enforced; the residue (a
field cfg'd off on every platform) is covered by comparing the fingerprint across the
CI matrix. Adoption note: the real derive must also fold serde/rkyv attribute tokens —
they change wire/disk layout without changing field shapes. Full record:
`v2/spikes/fingerprint/VERDICT.md`.
