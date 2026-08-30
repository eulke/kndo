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

## Adapters write through `EvidenceSink`; metrics attach by `DeclarationId`

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

## 2026-08-29 — M0: the v2 workspace grows under v2/ until the root swap

Greenfield in place means the root eventually belongs to v2; until then the v2
workspace lives at `v2/` (its own Cargo workspace, pinned `rust-toolchain.toml`), v1
stays intact as the quarry and oracle producer, and neither workspace depends on the
other. The swap — v2 moved to the root, v1 retired — is a later mechanical step, not an
M0 concern.

## 2026-08-29 — CI is generated from the gate registry; the rc tag is deferred

`.github/workflows/v2.yml` is rendered by `cargo xtask gen-ci` from kndo-gates'
registry and guarded by the `generated_ci_is_current` gate — one spelling of the
invariant list (v1 kept three hand-synced spellings, including a named gate that had
never once run). The M0 exit criterion is proven by the package+verify-install matrix
on every push; the ceremonial `v0.0.0-rc` tag is deferred because v1's release.yml
triggers on any `v*` tag, and lands when that pipeline is retired.

## 2026-08-30 — Windows out of the CI matrix for now (owner decision)

tree-sitter-scss's build script hands MSVC a flag it refuses — the same upstream defect
that killed the v1 Windows release target after shipping unbuilt — and carrying hacks
for it now buys nothing. The matrix is linux + macOS; Windows re-enters when the CSS
grammar question is resolved (upstream fix, grammar swap, or an experiment that says
the SCSS adapter isn't worth the platform). Owner-decided scope change, recorded per
the scope rule.

## 2026-08-30 — Toolchain pinned to 1.98.0

Latest stable, and the exact version v1's CI already ran. `rust-toolchain.toml` +
`toolchain:` in the generated workflow move together (the generator is the single
source); a bump is its own PR with the full suite.

## 2026-08-30 — The adapter growth contract

Adapters must absorb new language knowledge for years without re-creating v1's
descriptor grab-bag. Four rules, now structural in `kndo-contract`: (1) **pairing** —
every optional evidence stream is declared in `EvidenceStreams`, carried on each
`FileEvidence`; the sink drops-and-reports writes to undeclared streams, and analyses
abstain over undeclared streams instead of guessing (the HTML/untested flood
generalized: v1's `declares_units_of_testing` was this rule invented ad hoc, once).
(2) **Default compatibility** — a new stream or capability defaults to
not-declared/empty, which through degrade-toward-keep-alive reproduces pre-capability
behavior: absence can silence, never accuse. (3) **Additive surface** —
`EvidenceSink`/`ResolveContext` only gain methods; growable enums (`SymbolKind`,
`RefKind`, `ImportShape`, `ImportTarget`, `RootTarget`) are `#[non_exhaustive]` with
keep-alive wildcard arms, while `Confidence`, `Reach`, `RootKind`, `DiagnosticLevel`
are documented closed-by-design (extension is a semantic change, not growth).
(4) **The growth triage** — evidence stream vs spec data vs `ResolveContext` query vs
toolkit — is a CLAUDE.md judgment block. Also applied: `SymbolKind::Other(SmolStr)`
carries the adapter's word instead of dropping it. The open visibility-ladder question
(v1's linear rungs could not express Rust's module-and-descendants privacy) goes to
EXPERIMENTS.md, measured when the v2 Rust adapter lands — not designed dry.

## 2026-08-30 — Timings live beside the report, never inside it

Performance is measured at three layers, none of which touches the byte-compared
envelope: (1) `PhaseTimings` on `Snapshot` — the engine times each phase
(discover/claim/extract/assemble/analyze); frontends render it in verbose/human output
and serve exposes it as its own metadata, both outside the `Report`; (2) the bench
harness (`xtask bench`, lands with real languages) measures around `analyze()` on
synthetic fixtures with a per-machine baseline — deliberately not a CI gate, because
v1 measured the same tree varying 22%/108% across containers; (3) standard profilers
on the binary. Evidence for keeping the envelope clock-free: v1's gates compare
envelopes byte-for-byte, and the one run-varying field that slipped in
(`health.previous`) forced a permanent field-stripping carve-out in
`threads_determinism`. Also applied this change: `ResolveCtx` → `ResolveContext`
(the spell-it-out rule, third application).

## 2026-08-30 — Roots are evidence from three sources, and the corpus decided each one

The manifest capability landed measured: vite went 771 → 739 → 702 and lodash
50 → 18 across the three recorded corpus runs as each root source was added, with
`corpus-findings/COMPARISON.md` explaining every remaining difference against the v1
oracle. The shape: (1) in-file roots stay extraction evidence (a shebang, a
`/test/`+`/tests/`+`__tests__`+`*.test.*` convention as Test-Probable, `*.config.*`
and rc-dotfiles as Tooling-Probable) — path-conditional, so the evidence cache key
now folds the file path; (2) manifest roots (`package.json` `main`/`module`/
`browser`/`bin`, `exports` AND `imports` string leaves, wildcard entries expanded via
`ResolveContext::files_with_prefix`, `.d.ts` companions, npm-`scripts` source paths
as Tooling) anchor engine-side on `GraphFile.anchored`, never in cached evidence,
because a manifest change must not invalidate the target file's extraction;
(3) workspace packages (`LanguageAdapter::packages` → `ResolveContext::package`)
link bare specifiers to sibling entries. A whole-file root hands its exported surface
to whoever rooted it — the 4-finding gates fixture proved private declarations in an
entry file stay judged. Per-import targeting replaced the M1 bind-to-every-target
approximation in the same change (`GraphFile.import_targets`), and `require()`/
dynamic `import()` with literal arguments count as imports (Namespace when bound,
SideEffect otherwise). Killed en route: rooting `dist/*` build artifacts (kept
accused — they are checked-in outputs nothing references; v1 only kept them through
HTML edges v2 does not claim yet).
