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

## 2026-08-30 — M2 closes: patch ≡ full in three shapes, and the corpus is a CI gate

The persisted graph (`.kndo/cache/graph.bin`, keyed by contract fingerprint +
`GRAPH_SEMANTICS_VERSION` + the adapter set as data) patches surgically only when the
file set, every claim, and every manifest are unchanged — measured by `manifest_state`,
a hash over the discovered manifests. Anything else falls back to full assembly from
cached evidence, and the incremental gate holds patched ≡ full across content-change,
added file, and deleted file. The harvested v1 js fixture corpus (22 projects) replays
under `adapter_conformance_fixtures_are_byte_identical`: precision fixtures pin zero
accusations, future-capability fixtures (generated files, variable dynamic imports,
CJS object exports) pin today's honest behavior so their diff announces the capability
that changes it. The corpus job runs the same three commands exercised locally before
it landed (clone at pins → measure → `git diff --exit-code corpus-findings/`), so the
committed measurement is enforced, not decorative. M2 exit numbers: vite 702 vs oracle
unused 844 (shared-verdict overlap 274 files + 5 public-API symbols; every delta
bucketed in `corpus-findings/COMPARISON.md`), lodash 18 vs 13 files (dist/vendor/HTML
edges explained), dogfood zero via honest abstention.

## 2026-08-30 — The duplicate floor is 60 normalized tokens, measured

Winnowing (k=5, w=4) over kind-normalized leaves lives in the toolkit; the js-ts
adapter emits it per function (semantics_version 2). The floor experiment on the
corpus: MIN_TOKENS 100 → vite 158 duplicates; 60 → 174 (+16), every sampled addition
a true clone (create-vite template Apps, byte-identical vite.configs), lodash
unchanged, and the harvested duplicate-structural fixture's designed clones (76
tokens) covered. Oracle: vite 179 — the residue is css/json subjects v2 does not
claim. 60 wins; it becomes a config key when the registry lands. Byte-identical
files are the same category's file-level judgment (hash_hex equality), and members
of an exact-duplicate group are excluded from the structural pass so a copied file
does not also duplicate every function inside itself.

## 2026-08-30 — M3 closes: judged categories, measured evidence strengths, one schema

Reachability went per-color and shared (one flood per root kind, every analysis reads
the same answer; unused held byte-stable at 702 on vite through the refactor).
Evidence strength is explicit in confidence now: coverage-measured untested is
Certain (lcov FN/FNDA first — a declaration line executes at module load, so DA
alone would call every loaded function tested; the conformance fixture is REAL
output from Node 22's built-in lcov reporter), graph-heuristic untested is Probable,
duplicate is Certain on fingerprint-set equality. The stale rule sharpened on first
contact with reality: this repo's own v1 pragma `kndo:allow-file crap` proved that
"not judged" must cover not-yet-built categories, not only abstained ones — the
dogfood gate caught it before the commit went green, which is that gate doing its
job. The envelope grew fixed/baselined/suppressed (SCHEMA kndo-v2/m3) and its JSON
schema is generated from the types and validated against real reports in the same
gate. M3 exit vs oracle: duplicate 174/179 (residue = unclaimed css/json subjects);
test-only 122 vs 27 and untested 25 vs 222 share one named cause — production color
starved by dist indirection — recorded in COMPARISON.md with its M4 experiment
(mapping built entries to sources), not patched around.

## 2026-08-30 — Extensions are declared once; the audit's four warts closed

Owner call: adapter-specific facts spelled in several places must have one
declaration. `AdapterSpec` gains `extensions` (no dot, resolution-priority order) and
declaring them IS claiming them — the builder derives the `**/*.<ext>` globs, so the
engine's matching and the adapter's own consumers read one list. js-ts now derives
its resolution candidates (with the one spelled `.d.ts` insertion), npm-script
detection and wildcard filters from the spec; MockAdapter's resolve reads its spec
instead of respelling `.kmock`. Corpus held byte-identical through the refactor —
the point was shape, not behavior. Closed in the same change, from the standing
audit: the generated schema now stamps `run.schema` as a CONST of the same `SCHEMA`
the report writes (the §7.2 detail v1's drift incident motivated — any other
envelope version fails validation); the pragma grammar takes v1-style free-text
reasons (leading words that parse are the categories, the first non-category ends
the list — the ~20 false Warn diagnostics the repo's own pragmas produced are gone,
and the dogfood gate now asserts a diagnostics-clean run so the class cannot
return); and `import_targets` carries debug asserts for its index-parallel
invariant at both write sites.

## 2026-08-30 — M4.b: the Rust adapter, and what the dogfood forced

Rust lands as the third language (`rust`, v1's id, tree-sitter-rust 0.24) on the
M4.a growth pack, and turning it on made the repository itself the dogfood subject:
the root `.ignore` now quarries v1 (`/crates/`, `/xtask/`, `/action/`, `/examples/`,
`/spikes/`, plus v2's frozen M-1 spike) and the gate's ACCEPTED abstention list is
EMPTY — every analysis judges kndo's own code, and zero findings is fully measured.
First contact reported 33 findings on ourselves; each one became a fix, a rule, or
a recorded gap, never an allowlist entry.

Decisions the corpus and fixtures settled, in the order the measurements forced
them:

- **untested's graph heuristic is test REACHABILITY at file granularity** (v1
  parity). The name-reference heuristic accused transitively-tested code —
  violating its own "under-accuse, never over" doc — so where coverage is silent, a
  production-reachable file no test reaches through imports is the finding, on the
  file, `Probable`. Manifest-anchored Production entries are wiring the heuristic
  skips (nothing can import a binary's main; the question applies to what it leads
  to); ingested coverage still judges per function, `Certain`, wiring included.
  Measured: vite 25 symbol findings → 6 file findings (same truths, the evidence's
  granularity); ripgrep 2, both real static blind spots (integration tests spawn
  the binary). Three js conformance fixtures regenerated under this change,
  reviewed leaf by leaf.
- **A name-binding keeps its declaration whatever the reach.** Rust's privacy unit
  is the module tree — `super::ENCODINGS` from a child module legally binds the
  parent's private static — so `bound_by_name` moved out of the exported-only
  guard. vite held byte-identical: importing a genuinely unexported js name is
  broken code, and broken code is never license to accuse.
- **`mod foo;` is an edge, not a declaration** (import-statement posture), and
  **`pub mod` in a lib tree is `ReexportAll`** — a lib's pub-mod tree is its
  published surface (`published-lib-surface` went from 3 findings to v1's exact 1).
  In files nothing can import (`main.rs`, `src/bin/`, `examples/`, `tests/`,
  `benches/`, `build.rs`) a pub mod publishes to no one and stays a mute edge —
  that distinction is what keeps `test-only-and-cycle`'s dead `rally` honest.
- **A `use` leaf emits a pair**: the named binding (per-item precision, privates
  included) plus the namespace record (the whole-surface keep that survives alias
  hops the resolver cannot follow). The cost is pinned in `test-only-and-cycle`:
  `rally`, which v1's per-symbol reachability accused, is over-kept by the pong
  surface until per-symbol propagation exists (same future as the ladder).
- **The crate root is found by ancestor scan** — the nearest ancestor directory
  holding `lib.rs`/`main.rs`. ripgrep's `[[bin]] path = "crates/core/main.rs"` in a
  lib-less root package had broken every `crate::` path under `crates/core`
  (24 false unused); the package-geometry fallback remains for entry-less trees.
- **`#[path = "…"] mod` redirects the edge, and `use` paths through the alias
  substitute it** (`use self::imp::*` where `mod imp` points at `disabled.rs` —
  the standard cfg-platform idiom; uses process after all mods for that reason).
- **A top-level `fn main` roots itself by convention**, `Probable` — Production,
  or Tooling in `build.rs`/`examples/` (cargo's own path semantics). Manifest
  roots stay `Certain`; this covers targets extraction cannot see declared.
- **Strings carry two kinds of hidden uses**: inline format arguments
  (`"{VERSION}"`) and attribute-string item names (`schemars(schema_with = "f")`,
  `serde(with = "m")`) — both now emit references. Trait impls declare nothing
  (accusing `fmt` would accuse the trait bound); `macro_rules!`, struct fields and
  enum variants stay undeclared on purpose (textual scope and derive dispatch are
  invisible to the grammar — never accuse what it cannot prove dead).
- **Pragmas START their comment**: `kndo:allow` mid-sentence is prose about a
  pragma, never a pragma or a diagnostic — the dogfood's own doc comments proved
  the need.
- **The dogfood's dedupe demands were real**: core's twice-written `line_starts`
  collapsed into one, and both adapters' cloned test harnesses promoted into
  `kndo-testkit` (`extract_evidence`, `declaration_named`, `resolve_in`) — the
  second-copy rule applied to ourselves. `RunOutcome::exit_code` exposed the
  member rule's re-export blindness; members of an owner bound by name are
  published surface (`owner_bound`), which `published-lib-surface` also demanded.

ripgrep at the pin: 143 findings — unused 3 (all verified true at the source),
duplicate 135 (real Type-2 clones of one test scaffold; presentation question
recorded, floor unchanged), test-only 3, untested 2. The 25 v1 fixtures replay
byte-pinned through the conformance gate beside the 22 js ones; divergences from
v1's expected verdicts are each named in COMPARISON.md (`rally`, symbol-level
test-only, the guest example v2 anchors because its manifest declares it).

## 2026-08-30 — M4.c: the Go adapter, and the package as the unit

Go lands as the fourth language (`go`, v1's id, tree-sitter-go 0.25), and its
shape — the package, not the file, is the compilation unit — grew the contract
one capability and exercised M4.a's other half:

- **`AdapterSpec::reference_scope`** (`File` default, `Directory` for Go), the
  capability rule in full: a default that reproduces pre-capability behavior, a
  named core consumer (`unused` pools references per directory, so a sibling's
  use keeps a sibling's declaration, private or not), and a conformance case
  (`multi-file-package`). The graph copies the scope per file
  (`GRAPH_SEMANTICS_VERSION` 3) so analyses never reach back into the adapter set.
- **The synthetic `"."` edge**: every file imports its own package — a mute
  binding to the non-test siblings, resolved as `Resolution::Files`. Reachability
  crosses the package the way the compiler does; `_test.go` targets are excluded
  so the production color never leaks through a test file.
- **Import resolution is module-path prefix matching**: the longest go.mod-declared
  prefix wins at `/` boundaries, the remainder maps into the module directory, the
  resolution is the package's files. go.mod packages are entry-less by
  construction — the M4.a `PackageEntry.entry: Option` decision, vindicated.
- **Three never-accuse facts, each a fixture**: `internal/` is the language's own
  fence and everything non-internal in a bin-less module is importable published
  surface (library-mode whole-file roots, Probable; `internal-package` matches
  v1's single finding exactly); `// Code generated … DO NOT EDIT.` files declare
  nothing accusable while their imports and references stay evidence
  (`generated-file`, zero, as v1); and methods are NEVER declared — structural
  interfaces dispatch on any method invisibly, which gin proved on first contact
  (`IsEmpty`, `MarshalYAML`: alive through encoding/json and yaml, accused by the
  name heuristic, fixed by dropping the class, not the cases).
- The dogfood caught the third test-harness clone (the `import` finder copied
  from the rust tests into the go tests) — promoted to `kndo-testkit` like its
  siblings. The second-copy rule now has three enforcement stories in one day,
  all from our own gate.

gin at the pin: 108 findings, all `duplicate` (real per-endpoint test-scaffold
clones), zero `unused` — matching the oracle's zero. Eight v1 fixtures replay
byte-pinned; `go-work-phantom-dep`, `internal-only-unit` and `private-type-leak`
hold at zero pending their v1 categories (undeclared, internal-only,
private-type-leak), which do not exist in v2 yet.

## 2026-08-30 — M4.d: the visibility ladder waits for its consumer, measured

The EXPERIMENTS entry came due when the Rust adapter landed, and the measurement
decides: DEFER. Demand is real and large — 8,026 oracle findings depend on ladder
knowledge (internal-only 7,983 across every language, private-type-leak 43), the
biggest category v2 does not report — but its consumer is the internal-only
analysis, which does not exist, and a capability without a named core consumer
does not merge. Building the rungs now would be exactly the speculative vocabulary
the contract's growth rules exist to prevent.

What M4 measured about the SHAPE strengthens the recorded hypothesis: three
languages shipped on binary `Reach` alone, and none of the milestone's
false-positive hunts needed an intermediate rung — they needed scope shape, twice,
and both landed as their own constructs: Go's package scope as
`ReferenceScope::Directory`, Rust's module-tree privacy as
bindings-keep-whatever-the-reach. When internal-only arrives, the ladder arrives
with it, designed rung+scope-shape from the start, and `Declaration.exported_as`
folds into the same type in the same deliberate contract change (the shape debt
stays recorded in EXPERIMENTS until then). ripgrep alone would convert 59 oracle
findings, gin 1 — the corpus numbers to beat are already pinned.

## 2026-08-30 — M4.e: built entries map to their source, existence-gated

The experiment COMPARISON.md carried since M3 ran and won: a `package.json` entry
naming built output that is not in the tree resolves to the same path with its
first segment under `src/`, through the ordinary candidate machinery (the
`.js`-names-the-source swap included), only when the literal entry resolves
nowhere and the mapped file exists. vite's test-only fell 122 → 16 (oracle 27)
and unused 699 as production color finally reached `packages/*/src`; lodash and
all 56 conformance fixtures held byte-identical, because an existing built tree
always wins untouched. The alternative recorded at M4.a — emitting entry-less
`PackageEntry`s for dangling entries — is superseded for js by this mapping
(the mapped entry is strictly more informative than no entry) and stays in
EXPERIMENTS only if a subpath-linking gap ever shows up in a measurement.

## 2026-08-30 — The multi-file unit is one concept: `unit_mates`

Owner call, from the M4 audit: the package-as-unit fact was spelled three ways —
`Resolution::Files` for imports, `ReferenceScope::Directory` for pooling, and a
synthetic `"."` import every Go file carried for reachability — and the third was
evidence lying about the source (a record with an empty span no code wrote). The
fix is the deeper concept: `LanguageAdapter::unit_mates(path, cx)` — the files a
file's names see WITHOUT an import — asked at assembly beside `resolve`, a pure
function of path and file set (which is what lets a content-only patch trust the
persisted values). The engine turns it into reachability edges and `unused` pools
references over the reverse visibility, so a mate's use keeps a declaration no
import ever names, private or not.

Retired by it, one commit after arriving: `ReferenceScope` (the narrower spelling
of the same fact) and the synthetic edge (Go's evidence is faithful to the source
again; its `semantics_version` bumps to 2 for the changed emission, and
`GRAPH_SEMANTICS_VERSION` to 4 for the new assembled field). Go's answer also
fixes a small infidelity the synthetic edge had: test files now see their test
siblings, as internal test packages really do — while the asymmetry stands (the
package never consumes its tests, so production color cannot leak through them).
Core still never says "module" or "package": what a unit IS stays the adapter's
knowledge; core only walks the visibility it declares.

Equivalence, proven not assumed: the full suite passed with ZERO fixture
regenerations — all 55 conformance reports byte-identical — and the corpus
findings held byte-for-byte on every repo. The one diff anywhere is gin's graph
stats in SUMMARY.md (edges 1711 → 230, unresolved 4 → 0): the synthetic edges
inflating an edge count and four phantom "." specifiers polluting the unresolved
counter, both gone — the change deleted noise and nothing else.

## 2026-08-30 — internal-only (and the ladder with it) lands in M6

Owner call at the M5 kickoff: the internal-only analysis — the largest oracle
category (7,983 findings) and the named consumer the visibility ladder waits for —
belongs to M6, beside the remaining languages and parity polish, not to M5's
plugin/ABI/frontend territory. M5 proceeds as planned: native plugins, the WASM
ABI with its compat matrix, and the two facade frontends.

## 2026-08-30 — M5.a: two frontends, and the facade rule becomes a gate

The plan's own note drove the order (`kndo-serve` early is the living test that
the facade suffices — v1's lone consumer drifted three times), so M5 opens with
both frontends at once, and with the rule they prove made executable:

- **The `kndo` CLI is real**: `check` (default) and `baseline`, `--json`,
  `--no-cache`, `--threads`, `--fail-on error|warning|info|never`, exit codes
  0 pass / 1 findings / 2 refused (clap's usage-error 2 folds into the refusal
  class, documented). All logic lives in the crate's library with a one-call
  `main` — a structure our own gates force: a bin-only crate is unreachable to
  tests through imports, and the dogfood's untested heuristic would say so.
  clap arrives with v1's measured feature trim. `RunOutcome::exit_code` meets
  its consumer at last.
- **`kndo-serve` exists at skeleton size**: MCP over stdio (newline-delimited
  JSON-RPC, hand-rolled on serde_json — the surface this skeleton speaks fits in
  one match), one tool (`check`) returning the same Report envelope the CLI
  prints. Refusals are tool-level errors (`isError`), never protocol failures —
  the conversation stays alive.
- **`frontends_import_only_the_facade`** is the tenth named gate: each frontend's
  `[dependencies]` contains exactly one kndo crate — `kndo`. Dev-dependencies may
  use the testkit; the product graph may not reach deeper. A new frontend joins
  the gate's list, never escapes it.
- The release loop's install check now runs `kndo --version` — bare `kndo`
  became a real analysis of the current directory, which is the product, not the
  smoke test. The full package → checksum → extract → run loop was exercised
  locally before landing, as the release surface demands.

Both frontends pass the dogfood from birth (zero findings over their own code)
and the corpus held byte-identical — frontends are presentation, and the
measurement agrees.

## 2026-08-30 — M5.b: native plugins, v1's containment model carried whole

The plugin trait lands in core (`kndo_core::plugin`), the composition in the
facade (`default_plugins()`, mirror of `default_adapters()`), and v1's containment
model carries without dilution: plugin findings are namespaced
(`plugin:<coordinate>/<rule>`) and advisory — the gate never counts them —
contributions are ALWAYS in the report (roots applied, findings, every dropped
assertion described, budget cuts), and a plugin can only lie about graph facts,
never forge engine state. Measured consequences of the shape:

- **`mutates_graph()` has no default**, and it is load-bearing twice over: any
  ACTIVE graph-mutating plugin bypasses the persisted graph cache entirely (the
  surgical patch never re-invokes plugin hooks, so it could never safely reuse a
  graph one influenced), and a root smuggled through the shared sink by a
  non-mutating plugin drops with a described line instead of applying. Both are
  tested end to end (`plugin_containment.rs`), not documented and hoped.
- **Activation is decided before the graph exists** — `FileExists` globs over
  discovery, `ManifestDependency` over names the claiming adapters report through
  the new `LanguageAdapter::manifest_dependencies` capability (default: none; js,
  rust and go implement it over the ONE manifest pipeline `roots`/`packages`
  already ride) — because the cache decision hangs on the active set. The
  dependency closure runs to fixpoint, so `dependencies: ["kndo:express"]`
  reaches a plugin whose own rules can never match; `AnyRule([])` is that
  posture's deliberate spelling.
- **Coverage moved out of the engine into the plugin system** with zero behavior
  change: `kndo-coverage` is a no-I/O parsing crate (the same code will compile
  to WASM as the reference external ingester in M5.d), and `kndo:coverage-lcov`
  is the first built-in — `Activation::Always`, `mutates_graph() == false`
  (v1's lesson in one line: an always-on ingester answering `true` would turn
  the graph cache off product-wide), reading its conventional paths through the
  well-known channel that exists precisely because run output is gitignored.
  First-Some-wins in registration order decides between competing ingesters.
- **Anchors now reach declarations**: plugin symbol roots land in
  `GraphFile.anchored` as `RootTarget::Declaration`, obtained through the
  contract's new read-side `FileEvidence::declarations_with_ids()` — ids stay
  unforgeable (only real declarations of the same evidence yield one). `unused`
  reads evidence roots and anchors through one chain; the whole-file/declaration
  distinction stayed byte-equivalent for every existing graph (anchors were all
  whole-file until now).
- **Two new named gates** (twelve total): `builtin_plugin_proofs` — every
  built-in proves its effect baseline-then-plugin and the proof list is closed
  over `default_plugins()`, so a coordinate shipped without one fails the suite —
  and `plugin_dependency_implication` — the dependency-activation path no shipped
  plugin uses is exactly the one a gate must hold in place.

The envelope grew `plugins` (SCHEMA `kndo-v2/m3` → `kndo-v2/m5`); all 56
conformance fixtures and all eight corpus reports regenerated with the identical
mechanical diff — the schema line plus the `kndo:coverage-lcov` contribution —
and every finding held byte-for-byte, coverage-lcov's Certain verdict included:
re-homing the ingester changed nothing it measures.

## 2026-08-30 — M5.c: the WASM ABI — one vocabulary, three worlds, born complete

`kndo:vocab@1.0.0` is ONE WIT package: a single `types` interface mirroring the
contract field-for-field (byte spans included), and the three worlds — `adapter`,
`plugin`, `coverage-ingester` — beside it in the same package. v1 deliberately
duplicated the vocabulary between its adapter and plugin packages so each could
version alone, and paid with per-world conversion modules synchronized by prose;
here one version stream is the POINT, and evolution is additive interfaces and
sibling worlds, never a grown record. Where the contract's ids are unforgeable,
the wire spells indices — and the host REPLAYS all wire evidence through a real
`EvidenceSink` under the loaded spec's declared streams, so clamps, drops and the
pairing rule hit a WASM adapter exactly as a native one.

- **The adapter world is born complete** — extraction, `resolve`, manifest roots,
  packages, dependency names, unit mates — closing v1's second-class-citizen cut.
  Resolution's "host callbacks" became two ENUMERATION imports (`known-files`,
  `package-entries`); the guest SDK rebuilds a real `ResolveContext` from them
  once per instance, so an external adapter implements the same
  `LanguageAdapter`, sink and context queries as a native one and exports it with
  one macro (`export_adapter!`). Rules like innermost-package matching keep one
  owner. The contract grew the enumerations (`ResolveContext::known_files`/
  `packages`) and owned-parts constructors (`AdapterSpec::assemble`,
  `PluginSpec::assemble`) for the bridge side.
- **The plugin world inherits containment instead of re-implementing it**: the
  bridge writes through the engine's own `PluginSink`, and content crosses as a
  snapshot prefetched THROUGH the engine's `ContentView`
  (`readable_paths()` + budgeted `read`) — so a WASM plugin's budget is charged
  by declaration rather than demand, and the cut still lands on the
  contribution. `mutates-graph` is a MANDATORY export: the WIT spelling of the
  native method having no default, and the compliance suite watches the cache
  bypass happen from outside (`graph.bin` absent, evidence cache present). A
  trapped guest contributes nothing — silently, today: a host-attributed dropped
  line is a ledgered gap, not yet a channel.
- **The ingester world is unidirectional** — the host locates the report by the
  spec's well-known paths, pushes bytes, gets RECORDS back, and maps them with
  `kndo_coverage::assemble`; `parse_lcov_records`/`assemble` split out of
  `parse_lcov` with unit-proven equivalence. The reference ingester IS
  `kndo-coverage` compiled to wasm32 — the "same crate compiles twice" promise as
  a binary, and the native/WASM prose-sync pair structurally impossible.
- **The host is one crate** (`kndo-host-wasm`, wasmtime `=27.0.0` isolated):
  fuel 50M per call and a 256 MiB memory ceiling carry from v1 with their
  reasons; epoch interruption stays off (wall-clock cutoffs would break the
  byte-identity gates). One deliberate deviation from v1's proven design: ONE
  INSTANCE PER CALL, not per round — no guest state between calls, no
  shared-instance locks under parallel extraction, instantiation amortized by
  the engine's JIT cache. Cost: resolution re-fetches the enumerations per call;
  measured acceptable at reference scale, revisit with a corpus number if a real
  WASM language lands.
- **Conversion lives once per side**: guest in `kndo-sdk`, host in
  `kndo-host-wasm` — where the generator insists on per-world Rust types, the
  conversions are one macro body instantiated per world, one source text. The
  SDK's asymmetry is stated, not hidden: adapters get the REAL native trait
  (`kndo-contract` compiles to wasm32); plugin and ingester authors write wire
  records, because their native trait lives in `kndo-core`, which cannot cross —
  dragging the engine into guests would be the wrong trade.
- **The compat matrix runs from the ABI's first day** — gate thirteen,
  `abi_compat_matrix`: the three reference components PINNED under `abi/compat/`
  (never rebuilt by the gate; `cargo xtask pin-abi` re-pins in the same commit
  as any pre-freeze WIT change, making every break a reviewable diff) are driven
  through real sessions to real verdicts against the HEAD host. The compliance
  suite builds the same guests FRESH (cargo + `wit-component`, the third-party
  path) and proves the whole surface: a WASM language whose unit mates and
  manifest dependency names behave natively, byte-determinism through the
  evidence cache, containment across the boundary, records into a Certain
  `untested` verdict. CI's test job gains `targets: wasm32-unknown-unknown` —
  the exact "can't find crate for `core`" lesson, applied before it fired.

The dogfood earned its keep once more: it flagged `PluginSpec::assemble` as a
structural clone of `AdapterSpec::assemble` — true, and irreducible (two spec
types each owe the wire an owned-parts constructor; different crates, no source
to share) — which became v2's first reasoned suppression, a line-scoped
`kndo:allow duplicate` with the why beside it. Everything else held to the byte:
all 56 fixtures, all 8 corpus reports, the schema — the ABI added a tier, not a
behavior.

## 2026-08-30 — M5.d: the build shell — `.kndo/plugins/` and the authoring guide

The `wasm` feature on the facade is the build shell: `open()` also loads external
components from `<root>/.kndo/plugins/*.wasm`, and both shipped frontends carry
the feature, so the release binary is the shell by construction — an embedder
that wants no wasmtime in its tree simply doesn't enable it, and nothing else
changes. The M5 exit criterion runs as a test in the CLI's own suite: the pinned
kmini adapter and probe plugin, copied into `.kndo/plugins/` of a temp project,
drive `kndo check --json` to a report whose adapter table says `kmini`, whose
findings carry the WASM language's dead code and the plugin's namespaced note,
and whose contribution shows the root that kept a file alive.

Load policy, decided not defaulted:

- **Presence is the opt-in; the file name is irrelevant, the world it targets is
  not.** Worlds are tried in a fixed order (adapter, plugin, coverage-ingester);
  the first that instantiates wins, and a reserved-`kndo:` coordinate stops the
  ladder — refused on identity, not shape.
- **External components are SECOND in every ordering**: an external adapter
  cannot take a built-in language's claims, and the built-in coverage ingester
  keeps first-answer precedence. Deterministic: the directory is walked sorted.
- **Activation is uniform** — a deliberate divergence from v1, which ran
  project-local components unconditionally. One activation semantics for every
  plugin, external or built-in; `Always` is the spelling for "just run", and a
  dropped-in component with `any-rule([])` stays dependency-reachable only,
  exactly as the posture means.
- **A component that fails to load never vanishes silently.** The session grew a
  composition-diagnostics channel (`Session::with_composition_diagnostics`);
  a broken or unrecognizable file becomes a Warn diagnostic on every report of
  that session, asserted end-to-end through the CLI's text mode.

`abi/README.md` is the authoring guide, written from the worked examples: the
adapter path (the REAL `LanguageAdapter` plus `export_adapter!` — same code
natively and here), the wire-record path for plugins and ingesters with the
containment promises stated as behavior, budgets and their reasons, the build
loop, and the proof discipline (`builtin_plugin_proofs` as the bar external
authors should hold themselves to).

## 2026-08-30 — One door: the unified extension mechanism (design approved; M6.a)

The owner's grill approved the redesign of the whole extension surface: ONE
species — "extension" — replaces the adapter/plugin/ingester taxonomy. One
`ExtensionSpec` declares everything a component does; one `Extension` trait in
`kndo-contract` (10 defaulted hooks — fewer than the 12 methods across the two
traits it replaces) is implemented by built-ins, embedders, and WASM guests
alike; one WIT world replaces the three. The engine routes by what the spec
declares — never by instantiate-and-see. The diagnosis that forced it: the
sniffing ladder, the SDK asymmetry (plugins wrote wire records while adapters
got the real trait), native counting two species while the wire counted three,
and the framework use case (language-aware reads + roots) straddling the
taxonomy. The deciding teleology, owner's words: **built-ins are extensions
that ship in the box** — the end state has them distributable and replaceable
like any other, so identity, trait, and packaging must be uniform now, while
the ABI is pre-freeze and zero external components exist.

Decisions, each with its rejected alternative:

- **D1 — namespaced coordinates for ALL**: built-ins rename to `kndo:js-ts`,
  `kndo:rust`, `kndo:go` (`kndo:coverage-lcov` already is). Measured before
  deciding: finding identity is category + subject + discriminator — the
  adapter id is NOT an input — so no finding changes id; baselines,
  suppressions and the v1-oracle comparison survive intact. The regeneration
  is confined to `run.adapters[].id` values in envelopes (~62 files, one
  string per row) plus the envelope const bump with its regenerated schema.
  `is_reserved_coordinate` stays the pure `kndo:` prefix. Rejected: keeping
  bare legacy names — it fossilized the old taxonomy in a public contract and
  made the future "built-in as installable component" a major version.
- **R1** — `run.adapters` → `run.extensions` (`AdapterRun` → `ExtensionRun`).
- **R2** — finding categories `plugin:<coordinate>/<rule>` →
  `ext:<coordinate>/<rule>`. Categories are finding identity, so this was
  now-or-never; verified that no harvested fixture or corpus report carries a
  plugin finding — the cost is test strings only.
- **D2 — claims gate evidence-gathering; activation gates judgment.**
  Activation evaluates ONCE, post-extraction (ManifestDependency consumes
  names manifest extraction produces), and gates conduct
  (`contribute_roots`, `report_findings`) and ingestion; extraction,
  resolution and unit mates are gated by claims alone. Two nuances are part
  of the decision: (1) one file, one extractor — the M1 first-claim-wins rule
  becomes an explicit law of the model; a framework extension claims its OWN
  formats and reaches language-owned files through `requested_file_access`
  at conduct time; (2) two paths into the graph split by the fact/judgment
  line — manifest roots are transcription (ungated, cacheable, per-file),
  conduct roots are inference (activation + mutates_graph). Rejected:
  two-stage activation (FileExists gating extraction at discovery) — two
  effective semantics; the accepted residual is that a claiming extension
  extracts (and caches) on projects where it never activates, with a
  discovery-time knob deferred until a named consumer exists.
- **D3 — the conduct methods sit behind a typestate key.** The builder has
  two stages: identity+extraction first; `.rule`, `.dependencies`,
  `.requested_file_access`, `.reads_reports` EXIST only past
  `.conduct(activation, MutatesGraph::Yes|No)` (enum, not bool). Forgetting
  the gates does not panic — it does not compile. `mutates_graph` becomes
  spec data (where the wire wants it: a mandatory field of the
  `extension-spec` record, alongside `activation` — hand-rolled guests are
  forced by shape; `always`/`false` are the documented-inert neutrals for
  extraction-only components). Today's builder default of `AnyRule([])`
  dies: the dependency-only posture is written by hand. The
  "`mutates_graph` has no default" gate retires INTO the type system
  (precedent: the ReferenceScope retirement) — the bad case is no longer
  writable; the containment behavior (roots from a `false` component refused
  with a described drop) stays tested in compliance. Rejected: panic in
  `build()` with a gate proving it — the original form of this decision,
  overturned in the grill; two stages is one method boundary, and "make
  invalid states unrepresentable" demanded the stronger form.
- **D4 — pure ingestion**: `ingest(report_path, bytes) →
  Option<Vec<CoverageRecord>>`, the shape the WIT world already proved. The
  engine walks the new `reads_reports` spec field (splitting
  `requested_file_access`'s double duty — one field, one meaning) through
  the well-known channel, pushes bytes, assembles records→coverage uniformly;
  first answer wins in registration order (built-in lcov first — precedence
  unchanged). `WellKnown` leaves the trait surface and becomes
  engine-internal; no hook touches the filesystem.
- **D5 — coverage records are contract vocabulary**: the record types move to
  `kndo-contract` (the WIT already keeps them in `kndo:vocab/types`);
  `kndo-coverage` remains a parser crate depending on the contract. This
  breaks the cycle that would keep the trait from naming `ingest`'s return.
- **D6 — conduct views are contract interfaces**: `GraphAccess` (paths +
  contains — the surface the wire proved sufficient), `ContentView`,
  `ConductSink`, `PluginTarget`, severity/activation/rule types move to the
  contract; `Graph`, the round, target resolution and all containment stay in
  core, which implements the interfaces. Rejected: moving `Graph` itself.
- **D7 — one WIT world**: all exports (minus `mutates-graph`, now a record
  field), all imports. The SDK macro stubs unimplemented exports AND keeps
  the raw bindings private, exposing only phase-correct wrappers
  (`ResolveContext` in extraction hooks, `GraphAccess` in conduct hooks) —
  so phase discipline is structural for native code (hook signatures carry
  only what the phase provides) and for SDK guests, and enforced by
  host-side phase scoping for hand-rolled guests: an extraction-time call to
  a conduct import traps with a named violation, proven by a deliberately
  misbehaving compliance guest. Rejected: inspecting a component's exports
  to infer capabilities — sniffing with better manners.

§8 as settled in the grill: the phase-discipline price shrank to "the .wit no
longer communicates phases by shape" once the three enforcement tiers were laid
out; the regeneration precedent is contained by being THE one, written here;
the reservation wart died with D1; the wide-trait watch item keeps the
standing law (a new hook enters with a spec capability + named consumer +
conformance case). The half-measure — unifying only the wire — stays
rejected: it keeps the SDK asymmetry and both spec twins, and buys the look
of the benefit without the benefit.

**Migration in two batches.** Batch 1, the mechanism under the OLD names:
contract types and trait; engine over `Extension`; built-ins rewritten;
`MockExtension` folding the three local mocks; one world + SDK + host bridge +
single-path loader; guests migrated plus a fourth two-cluster reference guest
(the framework case) and the misbehaving guest; gates adapted
(`extension_dependency_implication` rename, single-load, phase-scoping).
Acceptance for every step of batch 1: PURE byte-identity — all 56 fixtures,
all 8 corpus reports, the schema, untouched. Batch 2, one final commit: the
identity toll — D1 + R1 + R2 + envelope const bump + regenerated schema —
whose diff is exactly the enumerated list and nothing else. This entry marks
that commit as the repository's ONE deliberate regeneration; the
never-regenerate bar stands for everything after it.

## 2026-08-30 — M6.a executed: one door, and the single deliberate regeneration

The approved One-door design is implemented. Batch 1 (four commits) rebuilt the
whole extension surface under the OLD names with PURE byte-identity held at
every step — all 56 conformance fixtures, all 8 corpus reports and the schema
unchanged while the taxonomy left the engine, the WIT collapsed to one world,
the SDK began handing every author the real trait, and the loader lost its
sniffing ladder. Batch 2 is THE identity toll commit, and this entry marks it
as the repository's ONE deliberate regeneration; the never-regenerate bar
stands for everything after it. The toll's diff, audited line-by-line with a
uniq count over every regenerated file, is exactly the enumerated list:

- `"schema": "kndo-v2/m5"` → `"kndo-v2/m6"` (64 envelopes: 56 fixtures + 8
  corpus reports), with the schema regenerated from the types.
- `"adapters"` → `"extensions"` in the run info (R1; `AdapterRun` →
  `ExtensionRun` in code).
- Built-in ids `js-ts`/`rust`/`go` → `kndo:js-ts`/`kndo:rust`/`kndo:go` (D1);
  `kndo:coverage-lcov` already namespaced. `is_reserved_coordinate` stays the
  pure `kndo:` prefix — the reservation list died unborn.
- Finding categories `plugin:<coordinate>/<rule>` → `ext:<coordinate>/<rule>`
  (R2; `Category::plugin` → `Category::extension`); no harvested fixture or
  corpus report carried one, so the change touched test literals only.
- NOT in the diff, as the design promised: not one finding changed id —
  identity is category + subject + discriminator, and the extension id was
  never an input. vite still reports 895, ripgrep 143, gin 108, lodash 21.

Two facts discovered during execution, recorded where they belong:

- The SDK's raw bindings are `#[doc(hidden)] pub`, not private: the component
  model's `export!` macro must expand in the guest crate, so full privacy is
  unavailable — the documented surface is phase-correct by construction, and
  the host's phase scoping covers whoever digs past it (proven by the
  `rude-probe` compliance guest, whose extraction-time `graph-paths` call
  traps with a named violation that surfaces as a diagnostic).
- The rust adapter emits no references from `use` paths, so a private module
  used only through imports reads as unused to the dogfood. The host's
  bindings module is `pub` + `doc(hidden)` (matching the SDK) rather than
  suppressed; teaching the adapter use-tree references is an adapter
  semantics change with its own fixtures and its own corpus measurement —
  deferred with that named shape, not silently.

## 2026-08-30 — Coverage is an extension; core is mechanism (owner direction)

Coverage the FEATURE never belonged to the engine — core exposes the tools for
an ingester to exist and judges what any of them delivers. The cut that makes
the dependency arrows say so:

- `Coverage`, `FileCoverage`, `assemble` and `line_starts` moved into
  `kndo-core`'s own `coverage` module: the format-blind mapping half (records →
  line tables → span answers) is engine semantics, and `line_starts` was
  already shared with suppression. Core's dependency on the coverage crate is
  GONE — the engine now depends on the contract alone, and no format name
  appears anywhere in it.
- `kndo-coverage` is now THE lcov extension and only that: the parser (format
  knowledge) plus the built-in `LcovPlugin` behind the same `Extension` trait,
  absorbed from `kndo-plugin-coverage`, which is deleted (it had shrunk to a
  35-line shell after the unification). One crate, contract-only, compiled
  natively as the built-in and to wasm32 by the reference guest — the "one
  parser, never drift by prose" promise intact and now covering the spec too.
- `parse_lcov` (the one-step convenience) died with its last callers — the
  engine-driven split IS the only path now, so the old split-equals-one-step
  equivalence test dissolved into each half's own tests.
- The facade re-export of `Coverage`/`FileCoverage` is dropped: no frontend
  ever consumed it, and findings — not raw coverage — are the frontier.

The boundary this deliberately does NOT move: the `untested` JUDGMENT stays a
first-party core analysis (gate-eligible, abstains without evidence).
Extensions contribute evidence; core judges — that is the containment model,
and moving the judgment out would demote `untested` to an advisory `ext:`
category. Verified as pure code motion: full suite green, zero drift in
fixtures, corpus reports and schema.

## 2026-08-31 — M6 order by census, and kndo:java's never-declare postures

The owner green-lit continuing M6; the oracle census fixed the order the
recorded M6 decision left open: internal-only's demand is 88% guava (7,014 of
7,983), so the remaining languages land before the analysis that needs them —
M6.b (java, then kotlin/swift), M6.c internal-only + linear ladder, M6.d
measured parity, M6.e close-out.

kndo:java (M6.b.1) ships with two never-declare postures, both measured:

- Enum constants are never declared. `values()`/`valueOf`/reflection reach
  every constant with no source line naming it — the grammar cannot prove one
  dead. Vindication came from the corpus itself: 84% of v1's guava `unused`
  oracle rows are enum members, 5,194 of them in a benchmark whose first line
  is guava's own `@SuppressWarnings("unused") // Nested enums used reflectively
  in setUp.` The one measurement killed 5,485 false positives.
- Constructors are never declared: `new Widget()` references the type, no
  source line calls `<init>` — a declared constructor is a manufactured dead
  symbol.

Resolution is the compiler-checked convention run backwards — path suffix over
the discovered set with a nearest-module preference (longest shared prefix)
for sibling modules declaring the same package — and the unit is the directory
plus the standard layout's two mirrors: `src/test/java` sees its
`src/main/java` package one-way; `src/main/java<NN>` multi-release variants
and their base are one unit symmetrically. Third-party packages stay
Unresolved, deliberately (no package→coordinate mapping exists without a
classpath; guessing floods `undeclared`).

## 2026-08-31 — kndo:kotlin lands; Python joins the plan as the first fresh-baseline language

M6.b.2 ships on the same playbook with Kotlin's own rules: public-by-default
reach (the opposite of Java — `internal` folds to Exported until the linear
ladder can say "module scope"), `override`/`operator` as dispatch roots,
promoted `val`/`var` constructor parameters as members, companion members on
the enclosing class, and the never-declare postures carried over (secondary
constructors; enum entries — the guava-vindicated rule). The JVM manifest
scanners moved to `kndo-toolkit::jvm_manifest` on the second-copy rule.
Measured on Exposed: 809 claimed, 774 findings; the 646-vs-91 duplicate
surplus is real lockstep duplication between its JDBC and R2DBC test suites,
and the 19-vs-277 unused gap is Kotlin's public default meeting library-mode
surface — the worst case of the recorded granularity gap, named fix M6.c.

Owner decision: PYTHON enters the plan as M6.b.4 — the first language with no
v1 quarry, no harvested fixtures and no oracle. Its acceptance bar is
therefore different by necessity and recorded here up front: corpus
measurement with sampled, explained findings and fixtures authored fresh
(same-or-better-than-nothing is not a bar; the bar is every sampled finding
defensible). Corpus candidate flask (BSD-3), license reviewed and pinned at
kickoff. Swift (M6.b.3) stays ahead of it in order — vapor and Alamofire
already sit unmeasured in the corpus.

## 2026-08-31 — The pre-Swift audit round: four auditors, one enumerated sweep

Owner-ordered audit (architecture, ergonomics, plan alignment, "no warts") before
M6.b.3. Everything below landed in one sweep; measurements are the corpus re-run in
the same commit.

**Correctness.** (1) `WellKnown::read` refuses absolute and `..` paths — a loaded
component's `reads_reports` is wire data and could climb out of the project root.
(2) Discovery closes over the TREE: the walker's global-gitignore, `.git/info/exclude`
and parent-directory sources are off (they made two checkouts of one tree discover
different sets; two fixture projects immediately gained a file the repo's own
`.gitignore` had been hiding — the committed `files_discovered` bumps are that leak
sealed). (3) A multi-line `kndo:allow` comment anchors "next line" at its LAST line.
(4) Repeated lcov `SF:` blocks accumulate (the wire path's semantics; shards no longer
overwrite). (5) A files-scoped abstention no longer disables stale-allow detection for
its category run-wide — only a whole-run abstention is the flicker case. (6) The
native `EvidenceSink` degrades out-of-range ids with a diagnostic exactly like the
wire boundary, instead of release-panicking. (7) Finding identity: the discriminator
is now real where subjects underdetermine it — stale allows carry a per-file ordinal,
conduct findings carry their message — two stale pragmas in one file no longer share
an id (baselining one silently baselined the other). (8) Baselines are versioned
envelopes (`kndo-baseline/1`); an unreadable one is a loud diagnostic, a v1 baseline
at the shared path is quietly another product's memory.

**The knob discipline, applied to ourselves.** Metrics mechanics moved to the toolkit
once (`MetricsSpec` — two grammar functions per adapter; `WINNOW_K`/`WINNOW_WINDOW`
named once), the walk unified on the WHOLE declaration node, and the branch rule
unified: a default/else/`_` arm is not a new predicate, null-coalescing operators are
not control forks (TS drops `??`, Go drops `default_case`, Java counts arrow
`switch_rule` but not default arms, Kotlin counts conditioned `when_entry` only, Rust
skips the `_` match arm). Kotlin's classifier was rewritten against the PINNED grammar
— `navigation_suffix`/`type_identifier`/five literal kinds do not exist in kotlin-ng
1.1.0; calls and literals now classify (the census's structural-gate lesson: verify
kind strings against the grammar you pin). Kotlin resolution tries the exact package
dir before the peeled parent (a wildcard import no longer grabs the parent package)
and gains Java's peel-to-file for nested types; source sets mirror BOTH spellings both
ways (joint compilation is one namespace). Generated-file posture unified across all
five (shared needles + per-language comment openers in the toolkit; TS/Rust root
generated files Tooling — their fixture said v1 did this and pinned the opposite).
Test roots: standard-layout dirs stay Certain; filename-only convention is Probable
and keeps the library-mode Production root. Every adapter bumped its version;
`kndo:coverage-lcov` too.

**One surface, then the toll.** The retired `AdapterSpec`/`LanguageAdapter` pair is
deleted (M6.a's unification, finished). `is_reserved_coordinate` lives in the
contract; the WASM host depends on contract + engine-as-dev-dep only. Conduct results
are typed records (`ContributedRoot`/`ContributedFinding`) and carry the extension's
own `confidence` (WIT record field + pins rebuilt via `cargo xtask pin-abi` — the
enumerated ABI toll of this round). A trapped conduct call lands as a described line
on its contribution (`ConductSink::note`) — a vanished call and a clean empty round no
longer look alike. `manifest-dependencies` runs under its own phase: reaching for
`known-files` there traps as a named violation instead of reading an empty snapshot
(extract's no-project-data rule is now stated correctly in the WIT and on the SDK's
`resolve_context`, with rude-probe compliance cases for both). The SDK derives its
wire streams from the ONE declared set (`EvidenceStreams::iter`), so a future stream
cannot be silently stripped. `Category::FIRST_PARTY` is the one list `parse` reads;
rule names reject `/` at declaration and at load (category identity is injective).
`Severity::as_str` and `Subject::render` are contract surface — the CLI's dependency
findings keep their dependency NAME. The dead `PluginSink` alias is gone.

**Doc truth restored** (the auditors' drift list): README speaks the living tree (not
the M-1 seed), the real gate names, and the full M6 order (swift M6.b.3 → M6.c scope
shape → M6.d/M6.e → python M6.b.4); COMPARISON's gin section explains `unit_mates`
(the mechanism that produces its committed numbers); EXPERIMENTS' ladder entry points
at `unit_mates` and now carries the worked region design (pending owner decision as
M6.c); the corpus SUMMARY template stops attributing abstentions to a pre-M2 gap.
Superseded prose recorded rather than silently outgrown: no `CacheEnvelope` ever
existed (bincode behind `KNE1`/`KNG1` magic with the fingerprint folded covers the
rkyv-version concern it named); `RunOutcome` shipped without `FailBudget`, and
`PluginContext`/`DiagnosticCode` became `GraphAccess`/`ContentView` and plain
diagnostics; the `unit_mates` entry's "55 conformance reports" was a miscount of the
56 on disk.

**Deferred to the owner, deliberately:** renaming the conduct cluster's `Plugin*`
family and `ExtensionSpec::extensions` (file suffixes) — identity-scale renames that
belong to a toll commit like M6.a tanda 2; the scope-shape contract change (M6.c);
`ingest`'s trap-honesty channel (EXPERIMENTS has both debts).

## 2026-08-31 — The naming toll: three passengers, one commit, before M6.c

Owner decision. Identity-scale renames ride together (the M6.a tanda-2 pattern),
deliberately BEFORE M6.c so the visibility capability is born with its twin name
instead of renamed after. The mapping, old → new:

- **The conduct cluster stops speaking v1's "plugin".** `PluginSeverity` →
  `ConductSeverity`, `PluginTarget` → `ConductTarget`, `PluginContribution` →
  `Contribution` (beside its children `ContributedRoot`/`ContributedFinding`),
  `Category::is_plugin` → `is_extension`, core module `plugin.rs` → `conduct.rs`,
  test `plugin_containment.rs` → `conduct_containment.rs`, gate
  `builtin_plugin_proofs` → `builtin_conduct_proofs` (registry + generated
  workflow). WIT: `plugin-severity`/`plugin-target` → `conduct-severity`/
  `conduct-target`. NOT renamed, deliberately: the report envelope's `plugins`
  JSON key and the `ext:` category prefix — both are user-facing contract
  (schema `kndo-v2/m6`, finding identity); the schema's `$defs` name follows the
  type and the regenerated schema is this commit's deliberate diff. Guest
  directory names (`probe-plugin`) keep their identity as fixtures.
- **`ExtensionSpec::extensions` → `suffixes`** (builder, getter, WIT field,
  wire structs, `declare_extensions` → `declare_suffixes`): the list holds file
  suffixes, and "extension" already means the species. The serde field feeds
  only cache keys — renaming invalidates caches once, changes no envelope.
- **`unit_mates` → `sees`.** The relation is directional (a test sees main,
  never the reverse; the engine's reverse map was already called `seen_by`) and
  "unit" borrowed Go's compilation story where the real fact is a shared name
  scope — in Java, "compilation unit" formally means ONE file, the worst
  possible reader collision. `sees(path)` = the files whose names `path` sees
  with no import naming them. M6.c's region capability arrives as `seen_from`,
  completing the pair.

ABI toll paid: pins rebuilt via `cargo xtask pin-abi` in this commit. Everything
byte-stable that should be: conformance fixtures, corpus reports and the report
envelope are unchanged; the schema diff is the `$defs` rename plus the
documented-key note.

## 2026-08-31 — Cache writes are atomic (temp-then-rename)

Both caches wrote with `std::fs::write` straight to the final path; two sessions
on one root (parallel gate tests, a user's second terminal) could hand each
other a half-written entry. The magic-prefix/bincode checks degrade most torn
reads to a miss, but "mostly degrades" is not a contract — writes now go
temp-then-rename in the destination directory, so a reader sees old bytes or
new bytes, never a mix. Failures stay silent by design: a cache that cannot
write is a cache that misses.

## 2026-08-31 — M6.c first half: the region, not the rung

The visibility mechanism the EXPERIMENTS ladder entry designed, landed:
`Reach::Scoped { scope }` carries the adapter's own token; `Extension::seen_from`
answers the region behind it (path- and manifest-computable, the `sees`
stability class; `None` = Exported treatment, keep-alive); the engine stores
regions per (file, token) on the graph (GRAPH_SEMANTICS 5) and judges by the
SET — a Scoped declaration pools its region, is never part of the surface an
entry or namespace importer hands out from OUTSIDE the region, and a member
with a bounded region does not ride its owner's hand-out (an `internal` method
of a public class is uncallable outside the module). Private members never ride
at all — vite re-proved that restoration byte-identically. The alias question
the EXPERIMENTS entry carried is answered the other way: `exported_as` does NOT
fold into the variants — reach level and module-system alias are orthogonal
axes (Kotlin `internal` + `@JvmName` coexist) — the inert Private+alias
combination dies at the sink instead, the one constructor.

Migrations measured exactly as designed: go lowercase and java package-private
became `Scoped("package")` with regions equal to their old pooling sets —
fixtures and gin byte-identical, the no-op proof. The unlocks: kotlin
`internal` → `Scoped("module")` (Exposed unused 18→38, each a module-unnamed
internal; the `internal-scope` fixture pins the FQN-no-import case that only
the region pool can keep) and rust `pub(crate)` → `Scoped("crate")` via the
package map (`crate-scoped` fixture; ripgrep unchanged — its crate-scoped
surface is genuinely used). guava unused 766→1,017 from the member-surface
rule; the fresh accusations are statically-true package-private members in
reflection-driven scaffolding (caliper, NullPointerTester), the reflective
escape v1 accepted too. `pub(super)`/`pub(in …)` stay Exported, recorded in
the rust adapter: an unanswerable bound must keep alive, and module-tree
regions are not yet enumerable from paths.

Contract fingerprint moved (regenerated in this commit), WIT `reach` is a
variant with `scoped(string)` plus a `seen-from` export, pins rebuilt. The
second half — `internal-only` over the same regions — is next.

## 2026-08-31 — M6.c second half: internal-only over the regions

The census's category, built on the same mechanism: a `Scoped` declaration with
real uses in its own file and no confident use beyond it — no binding importer,
no reference elsewhere in its REGION (the only files that can legally resolve
the name; matches outside it are v1's "weaker matches", carried by
`Possible`/`Info` exactly as v1 carried them). `unused` outranks it: a
zero-use declaration is dead, not demotable — the tiering v1 could not make,
and most of the numeric gap to the oracle (guava 3,305 vs 7,014, Exposed 31 vs
163, ripgrep 3; every slice explained in COMPARISON).

The ignorance rule demanded one new fact: whether a narrower rung EXISTS is
language knowledge, so `ExtensionSpec` grows `narrowable_scopes` (spec data,
floor 2 — default empty keeps the analysis silent; java: package, kotlin:
module, rust: crate, go: deliberately none — the same evidence is advice in
one language and noise in the other; gin reports zero by design). Analyses
read it through `RunContext::narrowables`, never by adapter identity.

The dogfood fired on its first run: `pub(crate) StoreData` in the WASM host,
used only in its own file — narrowed to private in this commit. Three
harvested fixtures gained internal-only findings (rust macro-use-mod, java
nested members, kotlin ctor defaults), each verified by hand; the wire grew
the spec field, pins rebuilt. The category leaves `Category::parse`'s
reserved shelf and is judged.

## 2026-08-31 — Owner directive: the oracle is a quarry, not a target

"Do not drag v1's vices along for the metric's sake. v2 must be immaculate even
when the numbers come out different, because v1's numbers carry its vices, its
wrongness and its bad measurements." Now law in CLAUDE.md ("The oracle is a
quarry, not a target"): no v2 behavior, threshold, confidence or wording is ever
justified by "v1 did it"; every judgment derives from v2's own evidence, and the
COMPARISON explains differences by naming v1's defect or v2's decision.

Applied immediately to the two places the M6.c entries above had leaned on v1:
`internal-only`'s confidence was `Possible` *because v1 hedged at Possible* —
wrong reason; v2's regions are enumerated, absence there is a strong fact, and
the analysis now carries `Probable` derived from its own residuals (reflection,
name-pool collisions). And guava's +251 unused had been half-excused as "the
reflective escape v1 accepted too" — the honest ground is v2's own: statically
true, framework dispatch is conduct-plugin territory (EXPERIMENTS records the
`kndo:caliper`-class candidate), and nothing is suppressed to match anyone.
The M6.c entries above stand as history; where their rationale cites v1, this
entry supersedes the rationale, not the code.

## 2026-08-31 — M6.b.3: kndo:swift, the census closes

The sixth built-in, first measured under the quarry-not-target law and first
language whose DEFAULT visibility is the region mechanism's home case
(`internal` → `Scoped("module")`, `narrowable(["module"])`). The postures, each
derived: flat targets (a SwiftPM module has no sub-packages; a file directly
under `Sources|Tests/` takes that directory as its target — the content-free
spelling of a `path:` override, Alamofire's layout); no test mirror in `sees`
(tests are their own module; `@testable import` is the edge, and the module
REGION still spans every `Tests/` tree since any of them may hold that import);
toolchain test-runner dispatch roots at `Certain` (XCTest dispatches `test*` by
name, swift-testing by `@Test` — the runner is the toolchain, the same
enforcement tier as the layout itself; this is what melted vapor's first-run
570 unused to 77, all but 12 of them test methods); witness keeps at
`Possible` for conforming types' non-private methods (external protocols'
requirements are not statically enumerable); never declared: initializers,
deinit, enum cases (`.case` dot-shorthand resolves by type, not name — the
cases' uses still pool as Read references); `Package.swift` and its
`@swift-*` variants are Tooling manifests, dependencies read by SwiftPM's own
labeled arguments, parsed with the extraction grammar.

Every node kind verified against tree-sitter-swift 0.7.3's node-types.json
before writing (the census's own lesson), and the smoke dump caught three
shapes the types file alone hides: `let` nests inside `value_binding_pattern`,
same-file `extension Foo` members attach through a two-pass type-id map, and
`case a, b` binds every identifier seat.

Measurements, decomposed in COMPARISON: vapor 216 (unused 77 — Development
examples plus dead test scaffolding, sampled; internal-only 85 over enumerated
regions; duplicate 51, real Client delegation twins), Alamofire 609 (unused
154 — sampled accused ⇔ zero grep uses: ~130 dead per-case test helpers v1's
fuzzy pooling kept alive, plus storyboard-instantiated Example types reported
on static ground, the UIKit-reflection plugin candidate joining caliper's in
EXPERIMENTS). 167 tests, 13 gates.

## 2026-08-31 — M6.b.4: kndo:python, the first fresh-baseline language

**Decision.** Python lands as the seventh built-in with no oracle, no quarry
and no harvested fixtures — v1 never spoke it — so every posture is derived
from the language and the acceptance bar is total: flask (BSD-3, pinned at
d318b68) measured and EVERY finding reviewed against the tree's own ground
truth, not a sample. Fixtures are authored new, six of them, each pinning one
accusation an underscore-private leftover earns.

The derived postures: visibility is the underscore convention and the honest
mapping is binary (leading underscore → Private, dunder names are protocol
spelling, everything else → Exported); `narrowable` stays EMPTY — no
enforceable rung sits between underscore and importable, so `internal-only`
is silent by construction ("add an underscore" is advice about a convention,
not a boundary the language checks); the module IS the file, so `sees`
answers nothing (the default's degenerate case) and packages re-export
through ordinary `__init__.py` edges; dispatch the source never names roots
as `Possible` for decorated defs (`@d def f` IS `f = d(f)`) and dunder
methods (the runtime protocol calls them), `Certain` for the `__main__`
guard and for `test_*` functions in discovery-named test files
(`test_*.py`/`*_test.py`/`conftest.py` — the runner dispatches by name);
never declared: `__init__`/`__new__`/`__del__` and function-local defs.

**The measurement fixed the adapter twice** — the point of a fresh baseline.
First run said 29 findings; six were false and both causes were wrong models
of the language, not tuning: (1) imports are legal ANYWHERE — the top-level
walk missed flask's function-scoped lazy imports (all four imports of
`debughelpers.py` sit inside function bodies, one of them inside a test), so
one whole-tree walk now collects them, `TYPE_CHECKING` and
`try/except ImportError` blocks included; (2) in `from X import a`, `a` may
be the SUBMODULE `X/a.py` (importlib's lookup order, the tutorial's
`from . import auth`), so every from-import binding now emits a namespace
probe of its dotted path — inert when no file matches, the language's own
semantics when one does. No spec-version bump for either: the adapter had
never been committed, so version 1 lands complete.

**Measurement.** flask: 83 claimed, 23 findings — unused 3 (all grep-verified
true positives; `app.py:_make_timedelta` is dead beside its living
`sansio/app.py` namesake, the accusation name-pooling could never make),
duplicate 11 (the real five-way `template_*` decorator family, one
byte-identical pair, four test clones), untested 9 (all true: sphinx config,
a test-less celery example, CLI-string-loaded test apps — a string is not an
import — and mypy-driven `type_check/` fixtures). Decomposed in COMPARISON.
181 tests, 13 gates, clippy clean.

The ancestor-package chain (`import a.b.c` executes `a/__init__.py` and
`a/b/__init__.py`) is NOT built: no measured case in flask needed it —
recorded in EXPERIMENTS as an open candidate instead.

## 2026-08-31 — M6.d: measured parity closed, and the reread caught a vice

**Decision.** The parity milestone closes as a measurement, not a prose pass: the
full v2-vs-oracle table (nine repos × every category) was tabulated from the live
reports and the pinned oracle, and every cell is now decomposed — in a milestone
section where one existed, in COMPARISON's new close-out section where none did
(vite `internal-only` 0 vs 147 is structural: TS has no `Scoped` rung, and v1's
147 are export-narrowing advice, a different analysis now pinned in EXPERIMENTS
with its demand; the five `test-only` zeros are the one library-roots mechanism
named at guava, standing for all five; lodash's untested/test-only remainders and
ripgrep's unused 3-vs-11 close under the decompositions already written). The
unbuilt categories are a ledger, not an omission: 497 oracle findings across
undeclared/unresolved/version-skew (the dependency-hygiene family, one
EXPERIMENTS candidate with per-language dependency models as its cost),
private-type-leak (consumer-rule censused on `RefKind::TypeUse`), cyclic
(zero-FP definition first), and deep-import (half killed by v1's own
measurement, half deferred on external-manifest visibility).

**The catch.** Rereading ripgrep's three `internal-only` against ground truth
showed two false: `set_errored`/`ignore_messages` are referenced only inside
`macro_rules!` bodies in their own file, and a macro template's names resolve at
every EXPANSION site — narrowing them breaks every `err_message!` caller, so
"the narrower rung would suffice" was wrong. The `macro-use-mod` fixture
(harvested from the very hunt that produced ripgrep's pattern) had the false
finding PINNED in its expectation since internal-only landed. The posture, not a
patch: free declarations named inside a `macro_rules!` body root `Possible` —
dispatch the source never names, the expansion site does. The fixture now pins
the contrast in both directions (macro-named silent, ordinary own-file-only
still fires), a new `macro-template-names` fixture pins it end-to-end (26 rust
fixtures), and ripgrep measures 144: internal-only 1, the survivor
(`RegexCaptures`) grep-verified true. One existing expectation regenerated,
deliberately, for exactly this documented reason.

Also fixed by the reread: Exposed's headline miscalled `test-only` an unbuilt
category (it is built; its zero is the library-roots mechanism) and now carries
the tracked 936→987 arithmetic. 182 tests, 13 gates, clippy clean.

## 2026-08-31 — M6.e: the milestone closes

**Decision.** M6 closes complete against the census order (M6.a unified door →
M6.b java/kotlin/swift + python by the owner's "Python antes" → M6.c regions +
internal-only → M6.d measured parity → this close-out); the public release was
never in the census order and stays with the owner's root-swap window. The
close-out verified the mechanical syncs rather than asserting them: `cargo
xtask gen-ci` produces zero drift against the committed workflow, and
`corpus/clone.sh` reads `corpus.toml` as its one source, so flask joined the
CI corpus job the moment it was pinned — no workflow edit existed to forget.
README's M6 section speaks the delivered state; the owner report's plan
section now carries the full M6 close in its Estado callout (republished).

**Where M6 leaves the tree.** Seven built-in languages behind one `Extension`
door; `Reach` with its middle rung and region-enumerated judgment; the
quarry-not-target law in CLAUDE.md; 182 tests and 13 named gates green;
corpus at nine repos — v2 12,928 findings against the eight-repo oracle's
24,459, every cell decomposed in COMPARISON — and the standing ledgers
(unbuilt categories with dispositions, the export-narrowing and
dependency-family demands) pinned in EXPERIMENTS with their numbers.

## 2026-08-31 — The surface law, and the render toll that opens it

**Owner directive.** v2 must never ship less value than v1: every v1 capability is
either present, carried in EXPERIMENTS' new v1-surface ledger with a disposition, or
dead with its vice named here. And nothing lands as a transliteration — each item is
rebuilt from v2's contracts, copied only if v1's version would survive v2's laws
untouched (none has yet). The law joins CLAUDE.md's quarry section as its floor.

**The first gap, measured.** The render census: v1 ships four formats (human, json,
agent, sarif) plus flag > `KNDO_FORMAT` > tty selection; v2 had two (text, `--json`).
Shipped, v2-native — each divergence from v1 carries its own reason:

- `Report::to_agent` (agent format 1): token-frugal text for LLM windows, core-side as
  a PURE projection of the report — no clock, no env — so the envelope's byte-identity
  gates cover it for free. v1's per-run numbering died: the stable `FindingId` is the
  reference handle, which no `#5` can be across runs. Version-free header (the format
  version and envelope schema, never the crate version), which is what lets the new
  `agent_format_matches_its_committed_golden` gate pin it byte-for-byte (gate #14,
  everything-specimen, KNDO_CONFORMANCE=overwrite to regenerate deliberately).
- `Report::to_sarif`: SARIF 2.1.0 with the contract's own vocabulary — category→rule,
  severity→level (info speaks SARIF's `note`), FindingId→partialFingerprints (result
  matching is what fingerprints exist for, and v2 ids are span-free by design), every
  subject's path→artifactLocation (v2 subjects always anchor — v1's optional-location
  branch has nothing to guard). Spans are bytes in this contract, so regions are
  SARIF's binary form (byteOffset/byteLength) — no invented lines; the line-numbers
  gap is the ledger's one open CONTRACT decision, not a render patch.
- CLI `--format human|json|agent|sarif` on `check` only (`baseline` has no report — the
  shape makes the flag unrepresentable there); `--json` died unreplaced by any shim.
  The binary reads tty + `KNDO_FORMAT` once and hands them in as `Host` data — the
  library stays deterministic and every path testable. Piped default is now json
  (`kndo | jq` is the designed pipeline; the terminal gets human). Malformed
  `KNDO_FORMAT` warns and falls to the terminal default — ambient config is never a
  refusal.
- One-source completions the toll forced: `Confidence::as_str` joins `Severity`'s
  pattern in the contract (tie-test pins both spellings to serde's); the facade now
  exports the envelope's full Rust vocabulary (`RunInfo`, `ExtensionRun`,
  `ReportDiagnostic`, `DiagnosticLevel`, `REPORT_SCHEMA` — renamed from `SCHEMA` for
  its life at the crate root).

**Deliberate regenerations.** `gen-ci` (the new gate's step), `gen-schema` — the diff
is one doc-string (schemars embeds rustdoc; the const value never moved). 193 tests,
14 gates, clippy clean.

## 2026-08-31 — Health: a derived ratio, not a penalty score

**Owner decision.** Health is a key piece and was never in any milestone — the
v1-surface ledger was its only mention. Built now, greenfield.

**The model.** `Report.health` carries three counted facts: `implicated` (distinct
symbol-or-file subjects with at least one first-party, warning-or-worse finding — two
findings on one function are one problem unit), `subjects` (every declaration plus
every claimed file — the graph's own universe), and the per-category tally of counting
findings. The score IS the ratio, `100 × (1 − implicated/subjects)`, computed at
render to one decimal in `Health::score_text` — the envelope stays integer-only and
byte-stable with no float formatting in it.

**Every rule is a derivation, not a knob:**

- *Extension findings never count* — plugin findings are advisory by the two-tier
  containment decision already on this log.
- *Info never counts* — it is the advisory severity tier; health takes no severity
  opinion of its own, so a category's health-weight is decided where its severity is
  decided (its analysis), and promoting one to warning changes health with no
  health-side edit.
- *Distinct subjects* — health measures how much OF THE PROJECT is implicated, not
  how many complaints exist.
- *The baseline is transparent to health* — acknowledged debt is still debt;
  baselining everything must not read as getting healthier (the CLI test pins this).
- *Suppression clears health* — an in-code `kndo:allow` is a human verdict
  overriding the analysis.
- *Absent, not 100, when `unused` never judged* — no reachability, no implicated
  set; `Snapshot.judged` (the categories that actually ran) gates it and the
  abstention channel says why.
- *No `previous`, no delta, no clock* — v1's `health.previous` was the one
  run-varying field that ever forced a determinism carve-out; v2's health is a pure
  function of the current tree, covered by the byte-identity gates like the rest of
  the envelope.
- *No penalty weights, no letter bands* — v1's −25/−10 bucket constants were
  underivable, and grade letters imply model meaning that does not exist. If letters
  are ever wanted they are frontend presentation over documented thresholds, never
  model.

**The measurement.** Nine repos, one run: vite 86.1 (699/5,044) → lodash 90.4 →
Alamofire 95.4 → vapor 97.6 → guava 98.6 (1,017 implicated in a 70,175 universe —
size cannot hide damage, damage cannot hide size) → Exposed and flask 99.7 → ripgrep
99.9 → gin 100.0 (its 108 findings are all Info-tier duplicates). Decomposed in
COMPARISON's health section.

**Freight.** Envelope schema regenerated (+Health defs); 79 conformance fixtures
regenerated — 505 insertions, zero deletions, verified health-only; agent golden
regenerated (the specimen's health comes from the real `Health::measure`, so the
golden pins genuine model output: its Info and dependency-subject findings implicate
nothing). Still open in the ledger: a `health` verb as sugar, and `--by-package`
(waits on package aggregation). 196 tests, 14 gates, clippy clean.

## 2026-08-31 — Lines in findings: resolved per run, never persisted, never identity

The ledger's one open contract decision, executed. `Finding.lines` is a 1-based,
inclusive `LineSpan` the ENGINE derives — in one pass, for every spanned subject —
by resolving the byte span against the file's actual newlines. The load-bearing
choices:

- **Nothing persisted learns about lines.** The pipeline already holds every file's
  content in memory (hashing requires reading), so the line index is recomputed per
  run as a pure function of content: no graph schema bump, no cache format change,
  no adapter involvement. Deleting the feature would delete one pass and one field.
- **Identity never includes lines** — moving code must not change a finding; the
  field is serialized display data, absent (old baselines included) without error.
- **One spelling.** `Finding::location()` renders `path:line — symbol` in the
  contract, and human and agent both call it; SARIF regions gain
  `startLine`/`endLine` beside the byte-true `byteOffset`/`byteLength` — code
  scanning anchors at the exact line now. Columns stay absent on purpose: SARIF
  counts them in UTF-16 units, and a slightly-wrong column is worse than none.
- Lines are exact in any encoding — a newline is one byte — which is why lines
  ship and columns do not.

Freight: schema regenerated; 24 conformance fixtures gained `lines` (116
insertions, zero deletions, verified lines-only); agent golden regenerated with
hand-lined specimen findings so the grammar is pinned; corpus reports regenerated.
196 tests, 14 gates, clippy clean, smoke-tested through the real binary
(`pkg/core.py:7 — _fresh_leftover`; SARIF `startLine: 7, endLine: 8`).

## 2026-08-31 — Diff modes: two full analyses over two pinned trees, composed

`kndo check --staged` (index vs HEAD) and `--diff <ref>` (worktree vs
merge-base(ref, HEAD)). The design is one sentence: **a diff run is the composition
of two full runs** — and everything falls out of it:

- **The engine never learned git.** Frontends own the git edge: the CLI resolves
  the base (HEAD / merge-base; `git write-tree` for the index — plumbing,
  worktree-untouched) and materializes it with `git archive | tar` into a scratch
  dir, stateless. Scratch trees run cache-off so nothing is written into them; the
  worktree's untracked `.kndo/plugins/` is copied over so both sides run the same
  composition — otherwise the diff reports the composition, not the change.
- **One split, everywhere.** `Snapshot::against(base, mode)` feeds the base tree's
  findings through the baseline mechanism — `new`/`fixed`/`carried` is the same
  operation whether the comparison set came from the baseline file or a tree. The
  span-free, line-free `FindingId` is what makes it correct: moving code cannot
  fabricate a new/fixed pair.
- **A tree-vs-tree split never consults the baseline file** — a baselined finding a
  change reintroduces reads as new debt, and the gate holds it.
- **`base_health` is the honest version of v1's arrow.** Both trees are pinned by
  the invocation, so `health 97.6 → 97.8` is a pure function of them —
  deterministic, unlike v1's cross-run `health.previous` (the field that forced the
  one determinism carve-out). Absent in full mode.
- **Envelope**: `run.mode` (`full`/`staged`/`diff`) is stated in every report.
  Agent format bumped to 2: the `result:` line reshaped (leading `mode`, and
  `carried` as the one label for the comparison set's still-present findings) and
  the health arrow — a grammar change is a version change, so agents parse on a
  stated contract.
- The gate stays what it was: new findings at or above the floor fail, in every
  mode — v1's per-mode default (`none` on full) solved the legacy-repo problem the
  baseline solves in v2.

Freight: schema regenerated (`mode`, `base_health`); fixtures regenerated
(`"mode": "full"`, pure addition); agent golden regenerated at format 2; corpus
reports regenerated. Three end-to-end CLI tests over real git repos (staged
new/fixed/carried + arrow + envelope; diff-vs-ref carried-not-gated; outside-git
degrades to a plain exit-2 failure). Smoke: the staged run reports
`staged: 1 new · 1 fixed · 1 carried`, `health 71.4 → 71.4`, exit 1.

## 2026-08-31 — Category selection is judgment scope, and the health verb

`--only`/`--skip` land as `Config.categories: Categories` — the engine does not run
what was not selected, which is the only honest shape: a display-only filter would
leave the `judged` set lying (a suppression for a hidden category would read as
stale), pad health with unjudged categories, and let a "clean" narrowed report
imply more than it measured. Instead the unselected analysis never runs, `judged`
shrinks, health follows judgment (skip `unused` and health is absent, not 100),
plugin findings pass the same category test, and the envelope's `run.selection`
records the narrowing verbatim (`{"only": ["unused"]}`) — what a narrowed run does
not list, it did not judge. Unknown names are refused invocations at the frontier
(exit 2, listing the first-party set) — told apart from a category that judged
nothing. Both sides of a diff-mode run share the selection, or the diff would
report the narrowing, not the change. v1's `--strict` did not come along: it
promoted `undeclared`, a category v2 has not built — the ledger's dependency
family carries it.

`kndo health` ships as sugar over the same full analysis: the health block alone,
the line on a terminal, the JSON object piped, always exit 0 — health is
measurement, not a gate.

Freight: schema regenerated (`run.selection` + Categories defs; fixtures and
goldens byte-unmoved — All serializes as absence). 202 tests, 14 gates, clippy
clean.

## 2026-08-31 — kndo.toml and init: config born with v1's lessons as law

The CLI's persisted invocation defaults, deliberately CLI-side: `[check]`
`fail-on`, `format`, `only`, `skip` — every key has a living consumer, and nothing
else exists (a commented-out key is still a promise; the `init` template and the
parse struct are held to ONE list by a test that parses the template both
commented and uncommented). Two behaviors are the point:

- **A typo refuses the run.** `deny_unknown_fields`: `fail-onn` is an exit-2
  error naming the key, never silence — the failure mode v1's LIVE_TABLES check
  existed to prevent, now structural.
- **Precedence has one spelling.** flag > `KNDO_FORMAT` (format only — the
  environment has no opinion on gates or selection) > `kndo.toml` > built-in
  default, merged in `effective()` and nowhere else (v1's EffectiveConfig lesson).
  `--fail-on` became `Option` so "user said warning" and "default warning" stopped
  being the same value. A malformed environment value warns and falls through; a
  malformed kndo.toml refuses — the file is the project's own claim.

`kndo init` writes the commented template (refuses to overwrite — the file is
someone's work), and `--hook` installs `.git/hooks/pre-commit` running
`kndo check --staged` — the diff modes' natural home. 207 tests, 14 gates, clippy
clean; envelope, fixtures and goldens byte-unmoved (config is invocation, not
output).

## 2026-08-31 — doctor: introspection through two new facade doors

`kndo doctor` prints what kndo would see at a root, without running an analysis:
the REAL composition in registration order (built-ins and `.kndo/plugins`
components alike, load failures included — an opted-in component never vanishes
silently), the config as parsed (a broken `kndo.toml` is doctor's diagnosis —
`config: BROKEN — …` — never its crash), and cache/baseline as filesystem facts
only (doctor never learns the baseline's format; byte size is honest without a
second reader). The two doors it needed went through the facade as PRs to core,
per the frontier law: `Session::extensions()` (the specs, the extensions' own
claims) and `Session::composition_diagnostics()`. Spellings come from the one
source each: clap's `to_possible_value` for flag values, `REPORT_SCHEMA` for the
envelope. 208 tests, 14 gates, clippy clean.

## 2026-08-31 — The query contract, designed; phase 1 shipped (the shared index)

**The thesis (owner-confirmed, v1's own):** the graph exists so an agent does not
burn tokens reconstructing it — navigation is selling the graph back at minimal
token cost. v1's orientation was right (the verbs, the selector addresses, explicit
elision, affordances-with-data); its design is quarried, not copied.

**The design, in one diagram:**

```
            ┌────────────── one contract (core) ──────────────┐
            │  query::Request ── Snapshot::query ── Response   │
            │        │                 │                       │
            │  selectors = the      navigate::Index            │
            │  Subject vocabulary   (keep rules, ONE spelling) │
            └───────┬──────────────────┬───────────────────────┘
   CLI verbs ───────┘                  └─────── unused judgment
   (find/describe/uses/used-by/          (keepers(limit 1) — emptiness
    trace/impact/explain)                 is the accusation)
   serve MCP tools (same Request/Response types, schema generated)
```

**The decisions, each with its reason:**

- **The index the agent navigates IS the index the judge used.** v1 kept a parallel
  navigation index beside the analyses — the drift class the one-source law forbids:
  `trace` must never explain a world the findings did not come from.
  `navigate::keepers(graph, index, decl, limit)` is the one spelling of the keep
  rules; `unused` asks it with `limit 1` (a boolean in witness's clothing), `used-by`
  asks for the capped list. Certified by construction: 79 conformance fixtures and
  the nine corpus reports byte-identical after the extraction, 14 gates green.
- **Selectors are the Subject vocabulary.** `path`, `path#name`, `path#Owner.member` —
  the finding address space and the query address space are one space; `describe`
  of what a finding points at needs zero translation. `dep:`/`pkg:` selectors wait
  for the dependency family (no such graph nodes yet); `roots:production|test|tooling`
  stays a trace endpoint.
- **The verb set, judged one by one:** `find` (search → selectors), `describe` (one
  node in full), `uses`/`used-by` (the neighbor pair — used-by IS the deletion
  question, answered by keepers), `trace` (why alive: root → node path, every path
  with its weakest confidence), `impact` (reverse closure; `--if-deleted` simulates
  the removal and reports typed reachability flips, never fabricated findings),
  `explain` (finding id → its subject described + the generic graph evidence;
  per-category enrichment lands per analysis later). All seven earn their place;
  v1's batched `kndo query` JSONL does not ship yet — serve is the batch amortizer
  (a persistent session over one graph), and JSONL returns only if measurement
  shows CLI-only agents need it.
- **One envelope, versioned, pure.** `kndo-query/1`; results align 1:1 with
  selectors and one bad selector never fails its siblings; every listing capped
  with EXPLICIT elision (a model must never guess whether it saw everything);
  node refs carry selector/kind/reach-color/lines, edge refs carry kind/confidence/
  site. No wall-clock, no cache-state inside the envelope — v1 carried
  `duration_ms`/`cache: warm` in query responses, the run-varying vice the report
  envelope already banned.
- **Reach colors are a core projection.** production / test-only / tooling-only /
  unreachable derived ONCE from the three floods (the same Reachability the
  analyses use); v1's `visibility: 0` ladder index is dead — v2 says
  `private | scoped(token) | exported`.
- **Agent text form** joins agent format 2 as new lines (selectors as handles —
  numbering stays dead; `more:`-style explicit elision; `next:` affordances so the
  model needn't memorize the CLI).
- **serve grows from skeleton to proof:** each verb one MCP tool, input schema
  generated from the same Request types (the report-schema machinery), Session
  held across calls — the amortized-graph transport for agents.

**Phasing:** Q1 (this commit) the shared index + unused on top of it. Q2 envelope +
find/describe/uses/used-by + CLI + agent render + schema + goldens gate. Q3
trace/impact(--if-deleted)/explain. Q4 serve tools + docs + ledger close.

## 2026-08-31 — Query Q2 shipped: the first four verbs on the one contract

`kndo find/describe/uses/used-by`, exactly as designed: `Snapshot::query(Request)
-> Response` is the one door; the CLI verbs and (next) the serve tools both build
the same `Request`. The envelope (`kndo-query/1`) is pure and versioned; its
request/response schemas are generated and gate-checked beside the report's; the
agent grammar (selectors as handles, `kept-by:`/`by-color:`/`elided:` always
explicit, `next:` affordances) is byte-pinned by a multi-verb golden. The new gate
`query_contract_is_generated_and_pinned` also holds THE certificate: for every
symbol `unused` accuses on the fixture, `used-by` returns an empty `kept_by` —
judge and navigator provably one. Two contract touches along the way, both
one-source moves: `SymbolKind::as_str` (the adapter's own word carried) and
`ReachColor` (the single projection of the three floods). 210 tests, 15 gates,
clippy clean; smoke on the demo shows the loop: the dead symbol answers
`kept-by: nothing` with its finding id beside it.

## 2026-08-31 — Query Q3 shipped: trace, impact --if-deleted, explain

The judgment verbs land on the same door, and each answer is graph evidence, never
narrative:

- **`trace` is a liveness proof.** The shortest root→node path over forward edges
  (imports + sees), from the production root set first, falling back test → tooling
  when the requested set was not pinned — reporting WHICH set anchored the path
  (`roots: production`) rather than silently mixing them. Each hop names the edge
  that led into it: `import` carries the resolution's recorded confidence, `sees`
  is structural and carries none — the chain's weakest link is visible, not
  averaged. For a symbol target the final hop is the in-file keeper, in the same
  keeper vocabulary `used-by` speaks (one `navigate::keepers` spelling). `path:
  null` is itself the answer — and the CLI exits 1 for it, so
  `kndo trace x && rm x` cannot delete something reachable.
- **`impact` is the reverse closure, and `--if-deleted` simulates, never fabricates.**
  Affected files nearest-first with by-color totals and the root kinds whose reach
  passes through the set. The simulation reports typed reachability flips: for a
  file subject, a re-flood with the file masked (newly-unreachable /
  newly-test-only); for a symbol subject, the declarations whose EVERY reference
  site lives inside the deleted span — a precise orphan subset, not a re-judgment,
  because findings that don't exist until the edit does must not be minted by a
  query.
- **`explain` closes finding-id → subject → why.** The brief (id, category,
  severity, confidence, message, location) plus the FULL describe of the subject —
  for `unused` the empty keeper preview IS the explanation; category-specific
  evidence deepens per analysis as they grow. An id from nowhere is a not-found,
  never a crash.

The gate's golden script grew trace (live + orphan) and impact (--if-deleted)
blocks, and the certificate gained explain's live half: the unused finding's id
explains to a subject whose keeper preview is empty, and an unknown id is its own
not-found. One dogfood catch during the build: `Verb::as_str` as a 7-arm match
was a structural clone of an existing describe ladder — reshaped to a
discriminant-indexed array tied to serde's spelling by a test, because the tool
flagged its own author. 212 tests, 15 gates, clippy clean.

## 2026-08-31 — Query Q4 shipped: serve is the amortizer, and one contract has two doors

`kndo-serve` grows from skeleton to its designed role: one MCP tool per query
verb plus `check`, all eight speaking the agent grammar — the token-thrifty
render is the reason this server exists, so serve returns it everywhere and the
JSON envelopes stay the CLI's piped door (one optimal render per door; a format
knob on serve waits for a measured need).

- **The held session is the amortization, stated and proven.** Query tools
  answer from the last analysis of their root, analyzing once on first use;
  `check` is the refresh and its description says so. A test edits the tree
  mid-conversation and asserts the held truth survives until `check`, then
  flips — the contract is behavioral, not a comment. Core-side, the navigation
  index moved into the snapshot (`OnceLock`): built on the first query, held for
  the snapshot's lifetime — a pure function of the graph, so serve pays one
  build per analysis while the single-query CLI pays exactly what it always did.
- **The advertised schema is the parsed schema.** Each verb tool's `options`
  is `query::options_schema()` — schemars over the same `Options` the door
  deserializes (`$defs` hoisted so refs resolve) — through a new `schema`
  feature on the facade, because serve is a frontend and reaches nothing
  deeper than `kndo::<Name>`. A typo in `options` refuses with serde's own
  message, the same deny-unknown-fields law as the CLI flags and kndo.toml.
- **Tool names are the contract's spelling.** `Verb::as_str` names the tools
  (`used-by`, not a second snake_case alias) — the CLI verb, the serde value,
  and the MCP tool are one word.

Dogfood caught the build once more: serve's test fixture project was a
structural clone of the CLI's — the second copy, promoted to the testkit
(`js_demo_project`, the three liveness stories every frontend test needs) with
both frontends delegating, exactly the floor the promotion rule names for test
machinery.

With this the navigation ledger row closes: all seven verbs on one contract,
both doors live, `frontends_import_only_the_facade` still the tenth gate's
proof that two frontends need nothing the facade doesn't export. 214 tests,
15 gates, clippy clean.


## 2026-08-31 — Presentation: --quiet, --verbose, --color, and the filter disposition

The last broadly-applicable v1-surface rows, rebuilt as the human render's own
options — they shape presentation and nothing else, so on a non-human format
(json/agent/sarif are byte-pinned contracts) the CLI says so on stderr and
changes nothing, and `--quiet --verbose` together is a refused invocation.

- **`--quiet` is the one-line contract**: the verdict line alone (findings
  count, or the staged/diff summary) — the exit code already carries the gate,
  which is exactly what a hook or script wants.
- **`--verbose` is the observability channel**: a `phases:` line rendered from
  the timings that deliberately live BESIDE the byte-identical report, never
  inside it. v1's other verbose effect — revealing `possible`-confidence
  findings — has no successor because v2 hides no confidence tier in the first
  place; there is nothing to reveal.
- **`--color auto|always|never`** in the universal spelling, resolved in the one
  merge site: flag > `NO_COLOR` (present and non-empty, no-color.org) > tty.
  The palette is semantic and small: severity words (error red, warning
  yellow, info cyan), the clean line green, and on a diff-mode health arrow
  the CURRENT side colored by DIRECTION — a fact derived from two measured
  ratios, compared exactly by cross-multiplication. A single score is never
  colored: judging 76 as "bad" would smuggle v1's letter bands back in through
  the palette. None of the three is a kndo.toml key — quiet/verbose are
  per-invocation moods and NO_COLOR is the persistent color preference; a
  config key waits for demand.
- **The query verbs' reach filter respells as `--reach`** (`kndo find x --reach
  production`), freeing `--color` for its universal meaning and reading better
  for what it filters. The contract field stays `color` — the envelope and
  schemas are untouched; only the CLI flag moved, before anything shipped.
- **SIGPIPE**: kndo's output is designed to be piped, so the CLI restores the
  default disposition at startup and dies silently with signal 13 under
  `| head`/`| grep -q` like every other Unix filter — v1's reasoning, adopted
  as v2's own judgment. The suite proves it the honest way: a fixture whose
  JSON output is ASSERTED to overflow a pipe buffer, then killed-by-SIGPIPE
  asserted with no panic on stderr. serve keeps error-propagation instead —
  a protocol conversation is not a filter, and it ends cleanly when its
  transport closes.

217 tests, 15 gates, clippy clean; smoke on the demo shows the staged arrow
`health 83.3 → 100.0` with the improved side green.

## 2026-08-31 — Directed trace ships as `--to`; v1's path enumeration dies

v1's `trace` had a second, positional form — `trace <from> <to>`, with `--all
--max-paths K` enumerating simple-path alternatives. The surface question
("how does A reach B?" — THE coupling question before a refactor) survives;
the shape and the enumeration do not:

- **`--to <selector>`, not a positional pair.** v2's inputs are 1:1 with
  results by contract; a positional pair would silently re-type the second
  input. With `--to` the target is one option for the whole request, and
  every input answers with ITS shortest path to that target — batch semantics
  preserved (`kndo trace a b c --to lib.js` is three answers).
- **The answer is the same `TracePath`**, with `roots` absent — the directed
  form's origin is the input itself, so the field that names which root set
  anchored a liveness path has honestly nothing to say. For a symbol target
  the final hop is its in-file keeper, the same vocabulary `used-by` and the
  liveness form speak.
- **`--all --max-paths` dies with its vice named**: path enumeration shipped
  without a consumer or a measurement — speculative surface. One shortest
  path is the deterministic answer; alternatives return only with a measured
  need.
- Two render fixes rode along, caught by the golden regen: the trace header
  spelled its root set via Rust's `Debug` (`from Production`) instead of the
  contract spelling — `RootSet::as_str` now exists, discriminant-indexed and
  serde-tied like `Verb`'s — and the no-path line respelled form-neutral
  ("not reachable that way").

serve gets the form for free (`options.to` flows through the same Request;
the trace tool's description names it). 217 tests, 15 gates, clippy clean.

## 2026-08-31 — The dependency family, measured first: two ship, two defer

The oracle's largest unbuilt block — 543 dependency-subject findings
(undeclared 293, deps-unused 198, version-skew 38, deps-test-only 14) — went
through the corpus experiment before any analysis was written. The
decomposition (recorded in `corpus-findings/COMPARISON.md`, per finding) turned
the demand on its head: the bulk was the oracle's own vices, and the family's
honest core is two small, high-precision analyses plus a set of adapter fixes
worth more than either.

**`unresolved` ships** — a relative specifier pointing at no file, `error`,
the import's own confidence, on the new `Subject::Import { path, specifier,
span }` (identity = path + specifier, spans carried for lines only, the Symbol
rule). Its precision floor, each rule measured: only the contract's `./`-style
Relative spelling (an adapter routing other grammars through the variant —
rust's `crate::` paths — is judged by its own resolver, killing 54 ripgrep
non-findings); only `Certain` imports (an adapter's derived probe is
speculation — python's submodule probes now honestly emit `Possible`, fixing
their mislabel); a target that EXISTS in the discovered tree is not missing
(`Graph.discovered` is new — assets and manifests live outside the analyzed
world; scope, not breakage); and the missing target's parent directory must
hold a discovered file (a `../dist/…` reach into build output is deliberate).
Corpus: vite 10 (three deliberate fixtures, four unmodeled vite resolution
features, three symlinks — discovery does not follow symlinks, a boundary now
written down), every other repo 0. The category's value is the regression it
will catch, and its noise floor is provably zero here.

**`version-skew` ships** — same name, diverging comparable requirements across
manifests, `info`/`certain` on `Subject::Dependency`, deliberately not denting
health. Peer scope exempt (a wide peer range beside a narrow dev pin is
correct practice v1 flagged); `version_req: None` never compares — and the
adapter that knows the ecosystem decides what is comparable (npm protocols,
cargo path/git/workspace-inherited specs). Corpus: ripgrep 10 (all
semver-compatible — the `workspace.dependencies` nudge), vite 3, everything
else 0 (the Exposed oracle's 4 were doc-snippet poms; JVM declarations ship
name-only until BOM/catalog modeling exists).

**`undeclared` defers with its number**: 2–3 honest true positives on this
corpus against a 124-finding fixture cliff and a 195-case
ancestor-declaration class the per-leaf model must first learn — the corpus is
a pathological instrument for this rule (vite is a repository OF resolution
edge cases). **Dependency `unused`/`test-only` defers**: config-driven tooling
is invisible to structural evidence (vite root: `typescript`, `lint-staged`,
`execa`…), and gin's go.mod `test-only` names a vice — Go has no dev section,
so the advice is unactionable by construction.

The carrier: `Extension::manifest_dependencies` now returns
`Vec<DependencyDeclaration { name, scope: Option<DependencyScope>,
version_req: Option<SmolStr> }>` — one manifest pipeline still, activation
reads `.name` from the same stream. js-ts and rust implement it fully; the
other five ship `name_only` (typed absence: skew stays silent rather than
wrong). The WASM ABI is untouched — the world still speaks names, the host
shim wraps them, and a versioned world can carry rich declarations later.
`Graph` grows `manifest_declarations` + `discovered`;
`GRAPH_SEMANTICS_VERSION` 5→6 (one knob). The contract fingerprint did not
move: subjects and manifest data are not evidence-reachable, and the gate
proved it.

Riding along, worth more than the analyses: the js-ts resolver strips
query/fragment suffixes (42 real vite edges had been dying as unresolved
behind `?worker`-style suffixes), the compiled-to-source swap learned
`.js→.d.ts`, `.mjs→.mts`, `.cjs→.cts`, and the adapter now CLAIMS `mts`/`cts`
sources at all (real files in the wild; vite ships them) — adapter version
3→4. vite's corpus totals moved 1077→913 findings, mostly unused
false-positives dying as their keep-alive edges came back.

218 tests, 15 gates, clippy clean; one conformance fixture change (the
npm-workspace fixture now also pins a version-skew and an unresolved finding),
called out here as the deliberate contract change it is.

## 2026-08-31 — Cyclic ships: hazard is the language's own word

Import cycles land as SCCs ≥ 2 over resolved import edges — never over
`sees`, whose regions are mutual by construction — ONE warning per cycle,
anchored at the lexicographically-first participant, the shortest loop through
the anchor spelled in the message as the evidence chain, and confidence the
weakest import on that loop (the honesty rule `trace` already applies to
paths).

Whether a cycle deserves a finding is a fact about the LANGUAGE, so it lives
where language facts live: `ExtensionSpec::import_cycles: CycleTolerance`,
default `Tolerated` (silence — the default-compatibility rule), consumed by
the one analysis, exercised by the harvested `cyclic` conformance fixture.
Two behavioral states, not v1's three: `Idiomatic` and `Impossible` behaved
identically (silence), and two states that behave the same are one state —
each adapter's declaration carries its own why in a comment instead. js-ts and
python declare `Hazard` (ESM/CJS TDZ and partially-initialized modules;
Python's circular ImportError); go, rust, the JVM pair and swift stay
tolerated, which retires v1's JVM file-cycle findings as the vice they were
(javac resolves reference cycles in multiple passes — flagging routine legal
structure dressed information up as a defect). A mixed cycle fires iff any
participant's language calls it a hazard. Self-import edges (Python's
`from . import x` inside `__init__.py` resolves to the package's own file)
are excluded before SCC computation — not a cycle between modules, and they
must not shadow a real loop's rendering.

Corpus: vite 31 (the 88-file `packages/vite/src/node` tangle is the headline;
its deliberate cycle fixtures are code like any other), flask 3 (the
package's own 20-file knot, `__init__.py → app.py` the shortest loop — a
category v1 never measured on Python), everything else 0. The oracle's two
package-level findings wait for the package graph (EXPERIMENTS, package
aggregation). Wire components cannot declare Hazard yet — the world speaks no
cycle vocabulary; absence defaults to silence like every other undeclared
capability, and a versioned world can carry it later.

219 tests, 15 gates, clippy clean; the harvested cyclic fixture and the
npm-workspace fixture now pin cycle findings byte-exactly (deliberate,
called out here).

## 2026-08-31 — private-type-leak ships: only the signature is a promise

An exported callable whose SIGNATURE references a private type — the other
direction of the visibility pair (`internal-only` finds visibility above use;
this finds it below). The evidence is new and minimal:
`Declaration.signature_span: Option<Span>` (set via `EvidenceSink::signature`;
the fingerprint moved and says so) — the adapter marks the promise region
itself, absence means silence, and a private type used inside a BODY is
ordinary encapsulation, never a finding. js-ts emits it for free function
declarations (everything before the body — name, parameters, return
annotation; classes and interfaces deliberately not: their "signature" would
be their whole body, drowning the analysis in member references).

The precision floor, each rule bought with a measured oracle vice:
**Exported-vs-Private only** — any `Scoped` reach on either side is silence
(v1 folded rust's `pub(crate)` to exported and accused crate-wide methods of
leaking crate-wide types: ripgrep, four findings about nothing; Java
package-private is `Scoped` here and equally silent). **The effective surface
is the whole owner chain.** **Same-file unique resolution** — a name that
could mean two declarations accuses neither. **A file carrying its own Test
root makes no public promise** — guava's nine oracle findings were
test-support constructors.

Corpus: vite 30 — the oracle's own thirty, the real shape (exported free
functions naming file-local types consumers can call but never name) — and
zero everywhere else, zero noise. Scoped-vs-scoped comparison (a region
strictly wider than another region) is a real future judgment and waits for
region semantics with a consumer, recorded in EXPERIMENTS beside the ladder
work. 219 tests, 15 gates, clippy clean; the harvested private-type-leak
fixture pins the canonical case (signature fires, body use does not), and
the fingerprint bump is this entry's announcement.

## 2026-08-31 — deep-import defers with its number; the oracle ledger closes

The last unbuilt oracle category decomposes to nothing: all six findings are
the monorepo's own test or tooling packages deep-importing vite's internals —
`@vitejs/unit-ssr`'s 99 sites are vite's unit-test suite doing exactly its
job. The consumer-role gate an honest v2 build would need makes the corpus
demand zero, and this rule's zero (unlike `unresolved`'s) guards nothing the
corpus can show. Deferred in EXPERIMENTS with the conditions that would
reopen it.

With this, every oracle category has a verdict: nine analyses ship (unused,
duplicate, internal-only, test-only, untested, stale, unresolved,
version-skew, cyclic, private-type-leak — ten, counting stale's
suppression-side), three defer with numbers (undeclared, dependency
unused/test-only, deep-import), and the deferrals' reopening conditions are
recorded. The remaining open candidates (hollow-test, speculative-abstraction,
churn×complexity, crap-with-coverage) have no recorded demand and wait for
their own zero-FP definitions plus a measurement.

## 2026-08-31 — Package aggregation: ownership as directory truth, and both oracle package-cycles retired as vices

The graph learns packages: `Graph.packages` (name, dir, anchoring manifest —
keyed by name AND dir, because parallel trees legitimately duplicate a name:
guava's `android/` mirror is two real `guava` packages, told apart by their
manifests) plus `Graph::package_of` — nearest-boundary ownership by longest
dir prefix, engine-side, language-free. JVM joins the package world: a
`settings.gradle(.kts)` names its included modules (colon optional — both
spellings are the same declaration), a `pom.xml` describes ITSELF (artifactId,
group-qualified only when the pom states one, matching the bare spelling the
dependency scan also emits), and gradle build files stay silent — a module's
name is positional, held by the settings file. `GRAPH_SEMANTICS_VERSION` 6→7.

**Health partitions by package.** `Health.by_package` — same universe, same
counting rule, split by ownership; the empty name is the unpackaged remainder,
and the buckets always reconcile to the whole. The envelope carries it always;
`kndo health --by-package` renders the table on a terminal. guava's split is
the showcase: both trees visible (`guava` 42/15,467 beside android's
61/15,256; `guava-tests` 332/14,998), Exposed resolves into its 38 modules.

**Package-level cyclic ships — and finds the oracle's two findings were
vices.** The edge is a DECLARED dependency on a workspace sibling at any
scope but Dev: a dev/test-scoped mutual never blocks a publish (npm installs
without devDependencies; gradle publishes without testImplementation). The
gradle scan now reads each line's configuration word (`testImplementation` →
Dev, `implementation`/`api`/… → Prod, unknown → honestly unstated) and maven's
`<scope>test</scope>` → Dev — declarations, not guesses. Against the real
pins: guava's pair does not exist (guava-tests → guava-testlib is
one-directional; v1 manufactured the loop), and Exposed's closes only through
`testImplementation(project(":exposed-tests"))` — test-scoped, publish-safe,
exactly the exemption. Corpus package-cycles: zero, each absence explained;
the mechanism is pinned by an engine test instead (a prod mutual pair fires
with both names in the loop; the same shape dev-side is silence).

221 tests, 15 gates, clippy clean; every conformance fixture regenerated for
the one new envelope field (`health.by_package`), the diff audited to contain
nothing else.

## 2026-08-31 — Scoped-vs-Scoped visibility comparison: measured to zero, deferred

The region-subset half of `private-type-leak` — fire when the signature's audience is
a STRICT superset of the named type's region (Exported-vs-Scoped, and Scoped against a
narrower Scoped) — was instrumented before being built, and the number closes it.

**Instrument.** A throwaway binary over the engine's own evidence graph, replicating
the shipped floor (all-Exported owner chain, same-file unique resolution, test-root
exemption) plus what the floor cannot yet have: a same-directory type index as the
region approximation (java/go packages are directories), a textual promise cut (a
function's signature ends at its body's `{`, a value's at its initializer's `=`, a
type's header at its body's `{`), and whole-span containment where java and go emit no
signature spans. Swift served as the control group: swiftc rejects real
public-exposes-internal signatures outright, so every Swift "signature hit" measures
the instrument's own noise floor.

**Upper bound: 1,190 candidates. True defects a maintainer would patch: zero.**
Per repo: guava 1,048, Alamofire 69, vapor 35, Exposed 30, gin 7, ripgrep 1. The
signature-position slice (154) decomposes without remainder:

- guava type headers (88): `public class X extends PackagePrivateSkeleton` — the
  deliberate skeleton/bridge idiom (`Abstract*`, `*Bridge`, `TypeCapture`, GWT
  specializations), uniform across the whole slice. v1's "package-private types in
  package-visible signatures" vice reincarnated as accusations against architecture.
- guava method signatures (11): four `protected` overrides forced by a
  package-private parent's contract (`protected` folds to Exported — a rung the
  ladder deliberately does not split), three GWT `src-super` shadow-tree interop
  shims (never a javac consumer surface), four qualified-name-segment
  mis-resolutions (the `function` collision recorded in EXPERIMENTS).
- Exposed type headers (15): public objects conforming to `internal interface
  OpBoolean` — an empty marker interface, deliberate DSL architecture, and it
  compiles in the shipped library as written.
- Swift signatures (40): all noise, as the control group predicts — 39 name-fuzzy
  mis-resolutions (stdlib `Result` 21, generic parameters `Failure`/`Value` 18)
  where same-directory name matching bound a generic parameter or a standard-library
  name to an unrelated internal declaration, plus one promise-cut artifact (a
  computed property's getter body sits before any `=`). Name matching is not
  resolution; the vice is now measured at 100% of what it touched.
- The BODY buckets (1,036): the factory pattern — a public signature declares the
  interface, the body constructs the package-private implementation — plus each
  class span re-counting its methods' bodies. This is the exemplary pattern in both
  Java and Go, and it dies the moment a real signature span exists.
- gin struct fields (4): three unexported fields (per-field visibility is invisible
  to the instrument), and the one pattern-true instance in the whole corpus —
  `Context.Errors errorMsgs`, an exported field of a package-scoped type, shipped
  deliberately for a decade in one of Go's most-used libraries. Not a finding
  anyone would act on; and Go's adapter deliberately declares neither methods nor
  fields, so even seeing it requires supply that does not exist.

**Verdict: deferred, 0/1,190.** The toolchains already own the valuable slices —
kotlinc and swiftc reject signature exposure, rustc lints `private_interfaces` — and
the one javac-silent ecosystem measured zero because guava's review culture already
enforces what the analysis would check. No contract work, no signature emission in
java/go, no region-subset judgment. Reopening condition: a consumer with a real case
(most plausibly a Java codebase leaking package-private types through public
returns/params, or per-field visibility demand in Go) re-runs this instrument's audit
before any build. Two adapter facts the instrument surfaced — java's qualified-name
segments entering the reference stream (keep-alive inflation, `unused` false
negatives) and generic supertypes bypassing `RefKind::Extend` — are recorded as their
own candidate in EXPERIMENTS with a fix sketch and their measurement.

## 2026-09-01 — kndo:java: qualified-type segments are spelling, not references

The Scoped-comparison instrument's collateral discovery, fixed and measured.
tree-sitter-java types every segment of a qualified type as `type_identifier`, so
`java.util.function.Function` also emitted references named `java`, `util`,
`function` — and pooled keep-alive matching let a declaration sharing a segment's
name live on unrelated qualified mentions (guava's `Tables.java` package-private
`function` fields, seven instrument rows). The walker already refused exactly this
for import and package paths; the same judgment now covers type positions: inside
`scoped_type_identifier`, only the named type itself and an uppercase-initial
qualifier (`Map` in `Map.Entry` — an outer class, which the grammar cannot
distinguish from a package) are references. The JLS case convention decides, and
its failure mode only ever KEEPS a reference — an uppercase package segment stays a
harmless extra use, while no real use can be dropped. Alongside: supertype names
classify as `RefKind::Extend` through `generic_type`/qualified wrappers
(`extends Base<T>`), never crossing `type_arguments`; core consumes `Extend`
nowhere today, so that half is evidence honesty for future consumers.

Measured on the corpus: 266 references dropped across guava (890,419 → 890,153),
**finding delta zero at the pin** — every collision-named declaration also carries
real expression uses, so the inflation was latent. Shipped anyway, deliberately:
the channel's failure mode is a DEAD declaration silently kept alive by an
unrelated spelling — invisible exactly until it matters, which is the one leak
direction a dead-code tool must not have. Two extraction tests failed before the
fix and pin both behaviors now; adapter version 2→3; conformance fixtures
byte-identical (reports carry findings, not reference kinds); every non-java
corpus report unchanged. 223 tests, 15 gates, clippy clean.

## 2026-09-01 — Export-narrowing: internal-only's Exported rung, js-ts first

The pinned residue (EXPERIMENTS, the ladder's two-rung slice: vite's 147) shipped
as a second rung of the SAME analysis: `internal-only` now judges `Exported`
declarations wherever the claiming adapter declared narrowing expressible — a new
spec capability, `ExportNarrowing` (`None` default; js-ts alone declares
`Expressible`, because dropping `export` is the narrowing tsc itself then
enforces; the wire world defaults to `None` like every undeclared capability).
One claim, two mechanisms: the Scoped rung disqualifies by uses across its
ENUMERATED region; the Exported rung has no region, so its disqualifier is total —

- own-file use required (else `unused` owns it), and the floor is the file's
  top-level surface (owner-None; owned members wait for their own demand);
- whole-file-rooted files exempt: an entry's exports are the outside world's
  surface, a test's are its runner's;
- any whole-surface importer (namespace, side-effect, reexport-all, glob)
  exempts the file;
- a binding import of the name, or a same-named reference in ANY claimed file,
  disqualifies — REACHABLE OR NOT. The live subgraph is the wrong universe for
  narrowing advice: an unreachable file still compiles against the export it
  spells. Three audited false positives bought this rule (vite's `__tests_dts__`
  type-tests — claimed, rooted by nothing, imported by nothing, still naming the
  symbols), and the engine test pins it adversarially: gating the binding set by
  reachability makes the test fail by accusing `spelled`.

Instrument before build, then the engine equal to the instrument: 51 candidates
on vite under the final floor, and the shipped analysis reports the same 51,
path-for-path and symbol-for-symbol. Eight hand-audited across shapes (functions,
types, regex constants): the name does not exist outside its declaring file
anywhere in the checkout, unclaimed file types included. lodash: 0 (a built
single-file library).

Against the oracle's 147: a strict subset — 51 shared, zero v2-only. The 96
oracle-only decompose into named vices: 18 `playground/` findings (deliberate e2e
fixture apps; `dead-accept.js#value` is dead on purpose), the packages/vite bulk
is type-only re-exports v1 could not see (`BuildOptions` sits in `index.ts`'s
`export type {...}` — v1 accused documented public API), and the rest is v2's
name-pool conservatism.

Corpus: vite 974 → 1,025 findings, health unchanged at 85.2 (`internal-only` is
Info and does not implicate); every other repo byte-identical. One dogfood catch
worth recording: the first draft grew `source_adapter_spec` a seventh positional
parameter and kndo-on-kndo immediately reported two adapters' `new` as
duplicates — the boilerplate echo of the "adjacent same-typed parameters" law.
Resolved by reshape, never by allowlist: the shared fn keeps six parameters and
js-ts, the one adapter declaring more, writes its own builder chain — the fn's
own documented escape hatch. Conformance grew the `export-narrowing` fixture
(the 23rd js-ts fixture: the finding, the entry exemption, the namespace
exemption, and the unreachable-binding protection in one project). 224 tests, 15
gates, clippy clean. Recorded asymmetry, deliberately untouched: the Scoped rung
still pools reachable region files only — changing a shipped analysis's universe
needs its own corpus proof first.

## 2026-09-01 — The report names each language's judgment capabilities

Follow-through on the owner's question ("shouldn't an adapter subscribe to the
findings that apply to it?"): the architecture already gates every
language-sensitive analysis on a declared FACT rather than a subscription —
`CycleTolerance`, `narrowable_scopes`, `ExportNarrowing`, evidence-stream
pairing, `signature_span` — because facts carry their reason in the type,
compose across languages inside one finding (a mixed-language cycle fires if
ANY participant declares the hazard), and keep zero-measured distinct from
not-judged (the distinction `dogfood_zero_means_measured` polices). A
subscription table would be identity-branching — the wrong-floor signal — with
two new failure modes the fact model makes unrepresentable: silencing real
findings by identity, and enabling categories without the evidence to judge.

What WAS missing is legibility: the answer to "why does kndo (not) report X
for this language" lived only in contract doc comments. Now every
`run.extensions` row carries the declared trio — `narrowable_scopes`,
`export_narrowing`, `import_cycles` — so the report answers it itself: a Go
row reads `"import_cycles": "tolerated"`, which IS why no cycle findings exist
there. Envelope and schema only; renders unchanged (JSON and `serve` carry it;
a dedicated human surface waits for demand). Every conformance fixture
regenerated, the diff audited to contain only the new row fields. 224 tests,
15 gates, clippy clean.

## 2026-09-01 — The refused list, re-reviewed under the owner's challenge

The owner asked whether the refused categories were refused too easily — "unused
dependencies you could delete from the manifest, you don't see that as useful?" —
and the honest answer is that two verdicts over-generalized their number.

- **`deps-unused` / `deps-test-only`: reopened.** The deferral said "config-driven
  tooling is invisible to structural evidence" — true for DEV dependencies, where
  eslint, prettier and vitest live and run from scripts, never from imports. It does
  not hold for PRODUCTION dependencies, whose whole contract is "the shipped code
  imports me": a prod dependency no claimed file imports is a real finding (the class
  depcheck, knip, cargo-machete, deptry and `go mod tidy` exist for), and the scope
  split the manifest itself declares (`DependencyScope`, shipped with the family) is
  the floor. The remaining exemptions are declared facts, not guesses: peer and
  optional scopes, workspace siblings resolved through `packages`, and implicit uses
  an adapter can SEE (JSX present ⇒ the JSX runtime package is used). Python's
  dist-name ≠ import-name gap abstains unless the derivation is unambiguous. Measured
  before built: the 198 + 14 oracle findings are the demand, the prod-scope
  instrument decides. `deps-test-only` (a prod dependency only test-rooted files
  import) rides the same machinery.
- **`crap`: reopened as an instrument problem, not a value verdict.** Demand read 0
  because the corpus carries no coverage — an artifact of the measuring setup. Both
  inputs exist in v2 (metrics winnowing, lcov ingestion); the experiment is to
  CAPTURE coverage from a real producer for a corpus repo (flask via pytest-cov,
  vite via vitest) and count. "Complex and untested" is a typed accusation with two
  measured inputs; the threshold is the design question the number answers.
- **`undeclared`: stays deferred, with its path.** 2–3 true positives on this corpus
  of well-kept libraries; its value lives in hoisted monorepo apps. It shares the
  bare-specifier → package machinery `deps-unused` builds, plus ancestor-manifest
  resolution, adapter-declared builtin lists, and alias exclusion. Reopens after
  M7.a, on that shared floor.
- **New candidate: JVM package (directory) cycles.** File-level JVM cycles were
  retired correctly (multi-pass compilation makes them routine), but cycles between
  PACKAGES are the jdepend/ArchUnit metric — a different unit, never measured. Row
  in EXPERIMENTS.
- **Verdicts that stand:** `--strict` (its function is `[check] fail-on`), the JSONL
  query batch (`serve` is the amortizer), `trace --all` (no consumer), `deep-import`
  (6/6 oracle consumers are test/tooling; reopens on a corpus case), v1's
  file-level JVM cycles and manufactured package-cycles, and the 96 `internal-only`
  extras — each refused with its number, none a lost capability.

The consolidated plan for what remains — these reopenings and the swap-blocking
surfaces the 2026-09-01 audit recorded — is README's M7 section.

## 2026-09-01 — dependency subjects ship; the health universe grows; identity is declared data

**Decision.** `unused` and `test-only` judge production-scope dependency
declarations (`Subject::Dependency`), on one shared floor: the manifest's adapter
declares a `DependencyIdentity` (how a specifier names a declaration — `PathPrefix`
for npm and Go, `CrateRoot` for Cargo, `Underivable` by default) and its
`dependency_importers` (unclaimed suffixes that could carry its imports); a manifest
is judged when identity is derivable, no such unclaimed file sits in its package, an
owned file is reached, and not every owned file is a test — each failure a
`manifests`-scoped abstention with its reason. A declaration is in use when any file
the adapter claims imports it (tree-wide: hoisting), when a literal mentions it
(`ImportShape::Mention`, new — it keeps a declaration and draws no reachability
edge), or when the manifest names it outside its declaration (`used_by_manifest`).
`unused` is `Warning`/`Probable` (a runtime can inject a use no import shows);
`test-only` is `Info`/`Probable` and never fires under `DependencyScoping::Unscoped`.

**Measurement.** Corpus: 119 declarations judged (ripgrep 61, vite 43, gin 15), one
finding (ripgrep `crates/index/Cargo.toml`: `fst`), zero false; the pre-build
instrument's floor exactly. vite abstains on 43 manifests for unclaimed
`.vue`/`.astro`/`.html`/`.css` importers, 3 all-test packages, 4 unreached
packages; JVM/Swift/Python manifests abstain as underivable (12/49/2/5). Health's
universe grows by the judged declarations: ripgrep 2,557 → 2,618, vite 5,052 →
5,095, gin 1,304 → 1,319; every other corpus cell byte-identical.

**Why declared data, not a callback.** The first cut asked the adapter
`imports_dependency(specifier, dependency) -> Option<bool>`, `None` meaning
"cannot derive". A manifest whose files carried no package-shaped import never
asked, stayed "judgeable", and accused every JVM dependency in two fixtures — a
callback never invoked cannot abstain. `DependencyIdentity` is spec data: an adapter
cannot claim to derive identity without saying how, `Underivable` is the default,
and the run's `extensions` rows show it. The same reasoning retired a core list of
"inert" suffixes: which unclaimed files can import is the ecosystem's fact, so each
adapter declares `dependency_importers` (js-ts: the single-file-component, page,
stylesheet and template families) and Cargo/Go declare none.

**Two defects the gates caught.** The dogfood accused `thiserror` in two of kndo's
own crates — named only in `#[derive(thiserror::Error)]`; crate paths inside
attributes are now imports (rust adapter 4). The corpus re-measure showed flask
losing a real cycle: the tranche-A rule "a `Possible` package import never resolves"
also matched Python's `Possible` absolute imports; `ImportShape::Mention` replaces
it, and the contract fingerprint moved.

**Contract change, audited.** 80 conformance reports regenerated: 69 differ only by
the two new `run.extensions` rows (`dependency_scoping`, `dependency_identity`);
the eleven with substance are go/go-work-multi-module (+1 judged), the four
`*-dependency-skip` fixtures and java/multi-release-variants (v1's "skipped"
diagnostic is now a typed `specifier-identity-underivable` abstention),
rust/workspace-deps (`serde` declared in `app`, never used — a true positive on a
harvested fixture), ts/deep-import (+1), ts/dependency-hygiene (`chai` `test-only`,
the fixture's own intent), ts/esm-dead-code (`left-pad` `unused`),
ts/npm-workspace-monorepo (+3 judged), plus the new ts/dependency-usage.
`GRAPH_SEMANTICS_VERSION` 9 (`users` tree-wide, identity from the spec).

## 2026-09-01 — `undeclared` ships on the shared floor; identity spelled per ecosystem

**Decision.** `undeclared` judges a reached file's `Certain` package-shaped imports
against the manifest chain (nearest first, ancestors for hoisting), on the same
floor as the dependency subjects; exempt: a self-reference resolved in the tree, a
platform module (`DependencyBuiltins` — Node's list plus specifier schemes, Go's
undotted first segment, Rust's `std`/`core`/`alloc`/`proc_macro`/`test`), a name the
file declares, a declaration in the chain (`@types/` through the identity), a
mention in any manifest (`Extension::manifest_mentions`, which also replaces the
`used_by_manifest` flag: one scan, two consumers) or in any literal in the tree.
`Warning`/`Probable`; never a health subject. `DependencyIdentity::PathPrefix`
split into `PackageName` (npm: scope/name, `package_of` spells what a finding
reports) and `ModulePath` (Go: the path is reported whole — the module boundary is
the declaration's to say). A `require`/`import()` inside a function, branch or
guard is `Probable` (js-ts 6): the optional-dependency idiom accuses nothing, and
`unresolved`'s Certain-only rule keeps it silent there too. Rust qualified paths
headed by a type, a primitive, a tool attribute or a `use`-bound local are not
imports (rust adapter 5). Manifests without declarations hold a
`ManifestDeclarations` entry — a package.json with no `dependencies` is still what
its files' imports answer to. `scripts` tokens with a path root what they name.

**Measurement.** Corpus: one `undeclared` finding (vite's
`playground/nested-deps/test-package-b`, importing from a committed
`node_modules` — true by definition), zero false; ripgrep 905 → 0 candidates
through the adapter fix alone; ablation with the importer doubt off surfaces
lodash's `@playwright/test`, the first measurement's true positive. Side effects:
lodash 21 → 16 (build scripts rooted), vite 1,024 → 1,010 (typecheck-script roots
reach `module-runner/`). COMPARISON has the table.

**Contract change, audited.** 51 conformance reports regenerated: 29 differ only
by the identity value's new spelling; go/go-work-phantom-dep (`example.com/a`),
rust/workspace-deps (`rand_chacha`), ts/npm-workspace-monorepo (`@demo/a`,
`left-pad` — the fixture's own comment names the phantom) and ts/dependency-usage
(`phantom-dep`) gain the harvested `undeclared` findings; ts/cli-only-dependency
abstains `nothing-reaches-owned-files` (no sources); sixteen JVM/Swift/Python
fixtures whose manifest declares nothing now abstain as underivable under
`unused` and `undeclared`. Fingerprint re-pinned (`used_by_manifest` gone,
`DependencyBuiltins` in the spec).

## 2026-09-02 — `crap` ships on real coverage; the threshold is the metric's own

**Decision.** `crap` judges every declaration carrying metrics outside test files,
`CRAP = cc² × (1 − cov)³ + cc` with `cov` the covered fraction of the function's
instrumented body lines from ingested coverage; a function at or above the
threshold is an `Info`/`Probable` finding on the symbol. The threshold defaults to
30 — the metric's own definition — and `[analysis.crap] threshold` in `kndo.toml`
overrides it (`Config::crap_threshold`, resolved in the CLI's one precedence site).
A function no test executed is `untested`'s subject and never `crap`'s: one verdict
per fact. Without a coverage report the analysis abstains for the whole run
(`no-coverage-ingested`); files a report never instrumented are unmeasured
(`no-coverage-record`, `files` scope). The dogfood gate accepts exactly that
whole-run abstention, with its reason: a coverage report is run input, and this
repository ships none.

**Measurement.** Real producers on scratch copies of two corpus repositories:
pytest-cov on flask (482 tests) and vitest with `@vitest/coverage-v8` on vite's
unit suite (932 tests). flask: 305 scored, 3 at 30, 1 partially covered. vite:
1,004 scored, 129 at 30 (70 partially covered), 82 at 50, 51 at 100. Every corpus
report otherwise byte-identical, plus the one abstention.

**Why `Info`.** The score ranks change risk; it names no defect. Health measures the
tree, and a coverage report is not the tree — a `Warning` here would move health
between runs with and without a report, which the health law forbids.

**What reality caught.** The engine panicked on the first real report (a one-line
function inverts the coverage line range) and dropped every function record from
coverage.py's lcov 2.x `FN:<line>,<end>,<name>` shape. Both fixed with tests; the
python fixture `crap-partial-coverage` carries a report captured from pytest-cov,
never hand-written. 81 conformance reports regenerated: 80 differ only by the
`crap` whole-run abstention; ts/crap (v1's harvested fixture) also reports one
uninstrumented file, and the new fixture pins a `crap` finding beside an
`untested` one.

## 2026-09-02 — Framework conduct: two Apple plugins, one scripts rule, six verdicts

**Decision.** v1's nine built-in plugins each went through the reflection-dispatch
rule — demand measured on the corpus first; only roots that land on real findings
earn a plugin. Two land as built-ins in `kndo-apple`, proven baseline-then-plugin
in `builtin_conduct_proofs` on Xcode's artifacts verbatim. `kndo:interface-builder`
roots every `customClass` a storyboard or xib instantiates and every outlet or
action it connects — members it can point at a real declaration of that class
for, nothing else — under every runtime the editor writes for: the document
format is one, `customClass` is instantiated and connections bind by name under
`iOS.CocoaTouch`, `watchKit` and `MacOSX.Cocoa` alike, so the coordinate names
the editor rather than one framework (v1's `kndo:uikit` read iOS documents only
and would leave Alamofire's watchKit `HostingController` dead). `kndo:info-plist`
roots the classes a bundle's class-naming keys name. Both resolve a bare name to
the declarations closest to the artifact and both are `Probable`: the artifact's
claim is certain, the match from a name to a declaration is by name alone with
no module to check, and a wrong match keeps alive, never accuses. Root
confidence has no consumer beyond the report, so the choice is documentary.

express is not a plugin: a package script handing a runtime a file (`node
server`, `tsx src/worker.ts`) makes that file an entry, an adapter fact — js-ts
version 7. serde, rkyv and wasmtime are dead by construction: v1's roots kept
trait-impl members alive, and in v2 a trait impl's members are never declarations,
so there is nothing to root (zero demand on ripgrep and on kndo itself). nextjs
is deferred at zero demand — no corpus repository is a Next.js app, and a plugin
without an instrument would be a guess. thymeleaf and libsass are deferred to
M7.d: their subjects are html and css files no adapter claims yet, and
spring-petclinic joins the corpus when they do.

**Contract.** `GraphAccess::declarations()` — every declaration in the graph as
path, name, kind and owner name — is the one door from a name read outside the
code to a `ConductTarget::Symbol`; the engine's graph view answers it, the ABI
carries it as the `graph-declarations` import (fetched on first use), and the
pinned guests are rebuilt in this commit. The symbol-kind wire table now exists
once, as `symbol_kind_conversions!` in the contract invoked by host and SDK: the
dogfood gate flagged the second copy the moment each side needed both directions.

**Rust adapter, version 6.** A `use` headed by a local another `use` in the same
file binds (`use a::b as c; use c::d;`) names that path, not a crate called `c`,
whatever the order of the two declarations — also the dogfood gate's finding
(`wire` and `awire` accused as undeclared on the host and SDK manifests).

**Measurement.** Alamofire 609 → 599: three `unused` classes, four `internal-only`
members of the rooted owners, three file `untested` verdicts on files that now
carry an anchored Production root (declared wiring, the rule that exempts a
manifest's entry). vite 1,011 → 1,000: eleven `unused` files handed to a runtime
by a script. Every other report byte-identical; every conformance fixture
unmoved; the proof fixture pinned at eight findings without the plugins and two
with. Contributions: interface-builder 4 roots, info-plist 1, nothing dropped.

## 2026-09-02 — The web adapters: `kndo:html`, `kndo:css`; json declined

**Decision.** Two non-source adapters land, each narrow by measurement.
`kndo:html`: a document roots itself (Production, Certain; Test under the web
tree's test paths, Probable), and its edges are a `script[src]`, a `link[href]`
whose `rel` loads a stylesheet or preloads a module or script, and the import
statements of an inline `<script type="module">` — read by their statement forms
alone, comments blanked, string literals skipped, script and style bodies raw
text. Nothing in a page names what an inline import took, so its shape is `Glob`
(the whole imported surface stays alive; symbol-level refinement waits for its
own measurement) and a dynamic `import()` is `Probable`. An attribute URL is
document-relative by definition, so a bare `main.js` is spelled `./main.js`.
Resolution is JavaScript's — a bundler serves the page — and is delegated to the
js-ts adapter, except the root-relative shape, answered by the nearest ancestor
directory holding the path (vite serves each app from its own directory).
`kndo:css`: css and scss claimed, the `@import`/`@use`/`@forward` graph, comments
for suppression, a generated sheet rooted as tooling output, and no symbols —
every one of v1's 137 stylesheet findings on vite was file-level. A bare
specifier is a package by declaration and a sibling by resolution: vite's sheets
name packages sixteen times over, and resolution tries Sass's sibling spellings
(suffix, partial, index) before leaving a name external. The scss grammar wraps
Sass's `as`/`with`/`show` clauses in error nodes; the specifier is read from the
statement's subtree with the `with (…)` map pruned.

**Engine.** `dependency_importers` is two-sided: a file with a declared suffix
that another extension claims has its package-shaped specifiers read by the
declaring ecosystem's usage judgment (a stylesheet's `@import "tailwindcss"`, an
inline script's `import "vue"`); unclaimed, it makes the judgment abstain as
before. Without this, claiming css would have turned every abstaining manifest
into false `unused` dependencies. `GRAPH_SEMANTICS_VERSION` 10. A loader's query
or fragment is never part of a package name — `package_of` and `names` strip it;
vite's `import 'normalize.css?inline'` had produced an `undeclared`/`unused`
pair on one manifest. Four path helpers move to the toolkit (`parent_dir`,
`join_relative`, `nearest_rooted_match`, `web_test_path`), the js-ts and Apple
crates their first copies.

**Measurement.** vite 999 → 834: 336 `unused` files a page reaches leave; 68
`unused` stylesheets, 60 `untested` page-reached app files, 13 symbol
refinements, 21 dependency verdicts and 7 page `unresolved` arrive — COMPARISON
decomposes every group. lodash 16 → 19, Alamofire 599 → 591, Exposed 987 → 986,
flask 26 → 28 (two sheets Jinja names through `url_for`: a `kndo:flask` plugin's
fact), guava and vapor unchanged. Claimed files grow with every doc site's pages
(Exposed 809 → 5,150; 2.3 s). No pinned fixture moved; the html fixture pins 5
findings, the four css fixtures are v1's harvested ones renamed by what v2
judges. Proved before shipping: nine `unresolved` from `<script src>` text
inside lodash's `document.write` strings, a mid-character panic on a non-ASCII
page, the `?inline` pair.

**json, declined.** 386 json files on vite, 22 imported from JS; v1's json
findings there were 15 `unused` tsconfigs and 20 unreferenced data files — a
config file's consumer is a tool, never an import, and the accusation is the
vice. v1's whole-file `duplicate` on html and css (28 on vite, identical
playground scaffolds) is not inherited: a document declares nothing to
fingerprint.

## 2026-09-02 — Coverage ingesters: cobertura, jacoco, go-cover, on the report's own spelling

**Decision.** Three ingesters join lcov in the coverage crate, one parser per
format and format knowledge only: `kndo:coverage-cobertura` (`coverage.xml`,
`cobertura.xml`, `coverage/cobertura-coverage.xml`; `<class filename>` line
hits, no declaration lines in the format), `kndo:coverage-jacoco`
(`target/site/jacoco/jacoco.xml`, `build/reports/jacoco/test/jacocoTestReport.xml`,
`jacoco.xml`; `<line nr ci>` hits and `<method line>` METHOD counters as
function records — the primary evidence), `kndo:coverage-go` (`coverage.out`,
`cover.out`, `coverage.txt`; every line of a block takes the block's count,
blocks sharing a line accumulate, no function records). All `Always` on and
`MutatesGraph::No`, first answer wins in registration order, lcov first. A
DOCTYPE is allowed on every XML read: real tools write one, and a parser that
refused it would ingest nothing in the field while a DOCTYPE-less fixture
passed.

**Engine.** Mapping records onto the project stays the engine's, uniformly: a
reported path is the project file itself when the project has it, else the ONE
project file that ends with it or that it ends with at a `/` boundary — a Go
profile keys by import path, JaCoCo by package and source name, coverage.py by
the name under a source root it records apart. Two candidates leave the record
unmapped: crediting the wrong file would be a guess. Two report entries naming
one file accumulate, the same rule as repeated lcov sections.

**Measurement.** Every fixture's report is its producer's: `go test
-coverprofile` in the go fixture, pytest-cov's `--cov-report=xml` in the python
one, jacoco-maven-plugin 0.8.12 in a Maven project committed whole. Proofs in
`builtin_conduct_proofs`: the never-run function is `untested` and Certain only
with the ingester, the graph's file-level Probable leaves where coverage now
speaks, nothing else moves. On the corpus copies: gin 108 → 115 with its
profile (seven `ginS` wrappers no test executes); flask's cobertura and lcov
from one run judge the identical 46 findings. 89 conformance reports and every
corpus report regenerated, audited to differ only by the three new always-on
rows.

## 2026-09-02 — GitHub Actions launchers are roots; a launcher is not a manifest; a named dot-directory enters discovery

**Trigger.** The dogfood: `action/render.mjs`, v2's own Action, reported
`unused` the moment a JS root existed anywhere in the repository — it is
launched by a composite action's `run: node "$GITHUB_ACTION_PATH/render.mjs"`,
a launcher no adapter read. Measure first: across the eight corpus
repositories, exactly one launches project files from a workflow — vite's
`publish.yml` and `prepare-release.yml` hand `scripts/detect-release.ts`,
`scripts/extract-changelog.ts` and `scripts/prepare-release.ts` to `node`, all
three reported `unused`, and `scripts/releaseUtils.ts` with them (two of the
three import it). python, go, cargo, swift launches from workflows: none.

**Mechanism, three parts.** (1) The js-ts adapter's launched-token rule
(version 8) reads a second launcher: a workflow's or composite action's `run:`
steps, and a JavaScript action's `main`/`pre`/`post` entries. The walk over
the YAML is language-blind, so it lives once in the toolkit
(`kndo_toolkit::github_actions`), on a real YAML parser (`yaml-rust2`) — block
scalars, quoting and `defaults` are exactly where a line scan would lie. Each
step is answered with the directory it runs in (`working-directory` at the
step, the job, or the workflow's `defaults`) and its command with GitHub's path
variables expanded relative to that directory: a composite action's steps run
in the CALLER's workspace, so an action's own file is spelled through
`$GITHUB_ACTION_PATH`, and the toolkit turns that into the action's directory.
(2) `ExtensionSpec::launchers`, a declaration apart from `manifests` on the
contract and the wire record alike: a launcher reaches `roots` and nothing
else. The first cut declared the workflows as manifests, and the engine did
what a manifest earns — an entry in `manifest_declarations`, owning files by
directory — so `.github/workflows/` became a package owning nothing, on which
every dependency judgment abstained (three categories, `nothing reaches owned
files`), and an action's directory would have had its imports judged against
empty declarations. The type makes that unrepresentable; the pinned guests
were rebuilt, and the hand-rolled one states the field, as the record's shape
forces it to. (3) Discovery's `HiddenOptIn`: dot-entries stay outside the
walk — tool state, caches, the VCS; v1 drew the same line — except a
dot-directory an extension's manifest or launcher glob names literally
(`.github` from `**/.github/workflows/*.yml`), which enters whole. No hosting
convention is spelled in the engine: the extension that reads one declares
it, and that declaration is the opt-in.

**Measurement.** Corpus: vite 835 → 831, exactly the four files above; every
other repository's findings unchanged, its `files_discovered` grown by its
`.github` files (gin 118 → 127, flask 217 → 226). Hidden source files
anywhere else in the corpus: 8, all vite (`docs/.vitepress`, two fixture
dot-directories) — no case for walking hidden entries in general. The
dogfood's third finding of the batch was the duplicate analysis accusing the
launcher pass of cloning the manifest pass; it was right, and the two
collapsed into one iterator parameterized by which glob list it reads.
Conformance: a 25th js-ts fixture (`workflow-launched-scripts`: a root step, a
`working-directory` step, a composite action through `$GITHUB_ACTION_PATH`, a
helper reached only through them, one orphan that stays accused); nothing
else regenerated.

## 2026-09-02 — Windows returns to the CI matrix; the release surface is rendered from one table

**Windows.** The 2026-08-30 deferral named its exit: the CSS grammar question
resolved. M7.d resolved it by shipping the grammar, and the one obstacle was
always a single line of `tree-sitter-scss`'s build script — a C flag MSVC
rejects. The grammar is vendored under `vendor/` verbatim but for that line
(`flag_if_supported`; `vendor/README.md` records the one deviation) and
reached through `[patch.crates-io]`. The vendored tree sits in `.ignore`:
third-party source is not the code under judgment, and its manifest is not
ours to align — the dogfood reported its `>=0.21.0` against the workspace's
`0.25` as version skew the moment discovery saw it. Windows is back in both
matrices, the test suite and the package-install loop; the MSVC run is the
first thing CI shows when it returns (M0).

**The release surface.** One producer: `kndo_gates::release` holds the target
table (four — x86_64 and aarch64 Linux musl through `cross`, x86_64 and
aarch64 macOS), the binary name, and the artifact's name and layout
(`kndo-<tag>-<triple>.tar.gz`, one directory holding `kndo`). `xtask package`
builds and archives from it, `xtask verify-artifact` checksums, unpacks and
runs the result, `render_release()` writes `v2-release.yml` from it —
dispatched with a tag until the swap gives it the tag trigger — and the tap
job substitutes each triple's checksum into the Homebrew template. The four
consumers (`install.sh`, `action/action.yml`, the Homebrew template, the
install page once the docs site exists) are read against that table by
`release_channels.rs`, never against each other. Nothing unverified until a
tag: on every push CI packages the musl target and proves it static, installs
through `install.sh` over a local HTTP server from the artifact just built,
and renders the notes with git-cliff; the workflow installs the pinned
toolchain (`rust-toolchain.toml` rendered into it, held by the same test).
`publish-crates` is not carried: every crate is `publish = false` until the
ABI freezes.

**Measurement.** The loop ran here end to end before the workflow existed:
package (musl) → verify-artifact → `ldd` answering "statically linked" →
`install.sh` over `python3 -m http.server` → `kndo --version` from the
installed binary. Six channel tests; `generated_ci_is_current` holds both
workflows. Windows itself is unmeasured from this container — CI's return is
its measurement.

## 2026-09-02 — `xtask bench` on a recorded baseline; gen-stdlib is dead

**Bench.** The harness rebuilt for v2: generated, deterministic fixtures at 1k,
5k and 50k files (import chains from the manifest's entry, one dead file per
decade, branchy exports, a clone pair per 500), five scenarios — cold full,
warm no-op, warm 1-file, warm 100-file, `--staged` — measured end to end on the
release binary, minimum of N. The baseline recorded on this container
(`xtask/perf-baseline.json`): 1k 71.5 / 21.6 / 22.6 / 36.3 / 177.8 ms; 5k
365 / 106 / 96 / 120 / 821 ms; 50k 4,431 / 1,116 / 1,184 / 1,150 / 9,832 ms.
The gate is both >10% and >10 ms over baseline, and it is not a CI job: the
baseline is one machine's, and v1's reasoning is adopted as v2's own judgment
— a gate that compares numbers never comparable fails for reasons no change
explains (CONTRIBUTING). What the table already says: a warm run at 50k is
discovery, hashing and the cache read (1.1 s); `--staged` is two full analyses
over two `git archive` trees and costs more than a cold run at every size
(9.8 s against 4.4 s at 50k) — recorded as the first perf candidate, the base
tree being a pure function of a commit and so a natural cache key of its own.

**gen-stdlib.** v1 generated stdlib datasets for bare-specifier
classification — node's `builtinModules` (68 entries at v22.22.2) and Go's
258 packages — with a task to regenerate them. v2 classifies by rule and
carries no dataset. js-ts: `NODE_BUILTINS` is 42 names and 9 scheme prefixes
under `names()`'s prefix rule; of the 26 entries v1's list has beyond it, 12
are subpaths the rule already names (`fs/promises` is `fs`) and 14 are the
underscore internals (`_http_*`, `_stream_*`, `_tls_*`), imported nowhere in
the corpus (0 occurrences across eight repositories). Go:
`DependencyBuiltins::UndottedFirstSegment` is the language's own definition of
its standard library, and gin reports 0 `undeclared`. No corpus finding traces
to a builtin misclassification: dead, no port. Its vice: a dataset that must
be regenerated against a moving runtime to stay true, standing in for a rule
that does not move.

## 2026-09-02 — The docs site: fourteen pages from the shipped surface, held by two tests

**What.** `docs/` (mdBook): introduction, install, getting started, the
command line, findings, health and coverage, configuration, suppressions and
the baseline, CI, navigation, agents, languages, extensions, FAQ. Written
from v2's behavior, never from v1's pages: every example is a real render of
the release binary over a conformance fixture, every table is read off the
spec builders and the CLI's own help, and a capability the docs describe is
one the tree ships. v1's per-plugin pages have no successor — the two Apple
plugins and the four ingesters are rows on the extensions page; a page per
plugin was prose about code that `builtin_conduct_proofs` now pins.

**Held by.** The install page is the fourth release-channel consumer:
`release_channels.rs` reads it against the target table (every archive name,
the installer URL and its three variables, the tap), the same way it reads
`install.sh`, the Action and the Homebrew template. Every relative Markdown
link in the tree resolves, by the `every_relative_markdown_link_resolves`
gate — links only, code spans and fences blanked first, because a path in
backticks is quoted rather than claimed. CI builds the book on every push
(`docs` job, mdBook pinned by release tarball); deployment to Pages lands
with the root swap, when the site's URLs become true.

**Measurement.** Fourteen pages, one build, zero broken links on the first
gate run over the whole tree (README, DECISIONS, EXPERIMENTS, CONTRIBUTING,
the corpus notes, the docs). `deep-import` stays a reserved category with no
analysis behind it — documented as such on the findings page, deferred with
its number in EXPERIMENTS.

## 2026-09-02 — The root swap: v2 is the repository

**What.** The v2 workspace moved from `v2/` to the repository root by rename, so
history follows every file; v1's tree — its crates, xtask, action, docs,
examples, spikes, design notes, manifests, workflows and agent skill — left the
working tree and stays in history. The quarry is closed: every capability v1
shipped is present, carried in EXPERIMENTS' ledger with a disposition, or dead
with its vice named. LICENSE, NOTICE (its URL corrected to the repository that
exists, and added to the published-URL test), SECURITY.md and
CODE_OF_CONDUCT.md carry over unchanged in substance.

**The coupling, measured before the move.** Two constants and two templates
were all of it: the gate registry's workflow paths (`ci.yml`, `release.yml` —
the `v2` names retire) and the repository-root resolution in `kndo-gates`, two
levels up from the crate instead of three; the workflow templates lose their
`v2` working directory, path filters and cache workspaces. `.ignore` keeps its
four exclusions at the root — fixture corpora, the frozen spike, the oracle and
corpus measurements, the vendored grammar — and drops the quarry lines; the
harness, CONTRIBUTING and the README say the new paths, and the README opens
as the product's front page ahead of the engineering log.

**What the swap turns on.** The release workflow's `v*` tag trigger (the
dispatch with a tag stays as the escape hatch; `inputs.tag || github.ref_name`
is the one spelling of the tag) and the docs site's deployment to Pages from
`main`. Neither is the first run of anything: every job of the release has run
on every push since the surface was rendered, and the book has been built on
every push since it existed.

**Measurement.** In the swap commit: 658 files deleted, 430 renamed, 121
replaced in place (paths v1 and v2 both had — `crates/kndo-core/src/lib.rs`,
`install.sh`, `Cargo.toml`). At the new root, before the commit: every gate,
the full suite, clippy and fmt green; the dogfood zero over the root tree with
the rewritten `.ignore`; the book built; both workflows regenerated from the
registry and byte-identical to the committed files.

## 2026-09-02 — The diff modes share the project's cache; where the rest of `--staged` goes

**Measured first.** The bench baseline said `--staged` costs more than a cold
run at every size (9.8 s against 4.4 s at 50k files). Decomposed on the 50k
fixture with the engine's own phase timings — the container's disk was
unusable for wall-clock work that day (the same `git archive | tar -x` took
1.9 s, then 19 s; a staged run 16 s, then 58 s), so every wall-clock number
below comes from the fixture copied to tmpfs and the two binaries alternated
round for round. A staged run was two materializations (`git archive | tar
-x`, 0.8 s each in RAM) and two COLD analyses, each ≈ 3.9 s: discover 0.42,
extract 2.4 (tree-sitter over 50k files), assemble 0.55, analyze 0.55. The
scratch trees ran cache-off by design — "nothing is written into them" — and
so paid extraction twice for content the project's cache already held.

**Change.** `Config::use_cache` became `Config::cache: CacheLocation` — `Off`,
`InTree`, `At(dir)` — and the diff modes hand both pinned trees
`At(<project>/.kndo/cache)`. Sound by construction: every entry is
content-addressed and keyed by everything that could change it (fingerprint,
adapter specs, graph semantics), so a pinned tree's unchanged files hit
exactly where the worktree's do, and still nothing is written into the
scratch trees. `--no-cache` turns it off for both sides. The gate
`a_shared_cache_is_read_and_warmed_across_trees` pins the contract: a copy of
a tree over the original's cache adds no evidence entry, patches the graph
(extract folds to zero), writes nothing into itself, and reports the bytes an
uncached run reports; a changed file adds exactly its own entry.

**Result.** Staged at 50k in RAM, minimum of four alternated rounds: 14.6 s →
8.9 s; the staged report byte-identical between the two binaries. Per side,
warm: discover 0.36–0.44, extract 0 (folded into the patch), assemble
0.36–0.42, analyze 0.54 — about 1.4 s against 3.9 s cold, and a ten- or
hundred-file patch costs the same assemble as none (0.38–0.40 s against 0.37).
What the phases do not time — process start, the 47 MB persisted graph's
store, the two scratch directories' creation and deletion, git plumbing, the
composition and the render — is the remainder. The committed bench baseline
predates this change and is re-recorded only on a quiet reference machine.

**What the remaining 8.9 s is, with its projections — the owner's call.**
(a) The base side is a pure function of a commit: persisting its snapshot
keyed by tree id and cache key would skip a whole side (≈ 2.9 s) on every
staged run that follows a previous one against the same HEAD — the
pre-commit loop's common case. (b) Materialization writes 50k files per side
(0.8 s each in RAM, the whole disk-bound term elsewhere); reading `git
archive` into memory instead needs a second door into a session (a
pre-discovered tree) and an equivalence gate for the ignore semantics the
filesystem walk owns. (c) Each side loads and stores the 47 MB persisted graph
(`graph.bin`; the evidence cache is 50,011 entries, 196 MB, untouched on a
patched run); a borrowed cache that reads and warms evidence but leaves the
persisted graph to the worktree's own runs would drop two stores per staged
run; the ceiling of that saving is the assemble phase of a zero-change warm
run — load, verify, store, nothing to re-extract — 0.33–0.38 s per side in
RAM, the smallest of the three.

## 2026-09-02 — The diff modes' base side is pinned by its tree

**What.** The base side of `--staged` (HEAD) and `--diff` (the merge-base) is a
pure function of a git tree and of the analysis identity, so its result is
persisted under `.kndo/cache/pinned/<key>.json` and read back by the next run
against the same tree — no materialization, no analysis. What is persisted is
exactly what the comparison consumes (`Snapshot::against`): the side's
findings and its measured health, never a graph. The engine never resolves a
tree; the CLI hands it git's tree id (`rev-parse <base>^{tree}`, so an amend
that only rewords shares the pin), and `Session::pinned`/`Session::pin` are
the two doors, closed when the cache is off.

**The identity.** Findings are a function of the analysis code as much as of
the tree, and no knob names that code — the contract fingerprint names the
contract's shape, the semantics version the assembly, and neither moves when a
judgment changes. Rather than a third knob that a forgotten bump would leave
stale in every developer's cache, the key folds in the executing binary
itself: blake3 over `current_exe()`, once per process (32 MB, a few
milliseconds with rayon), unreadable ⇒ nothing pinned or read. The rest of
the key is the graph cache key (fingerprint, semantics, every adapter spec),
the judged categories and the `crap` line, and the tree id. The file is JSON
under a schema string, not bincode: `Health` skips an empty partition when
serializing, which bincode cannot round-trip. The directory is capped at 32
entries, the oldest by modification time evicted — housekeeping that touches
nothing any run reports.

**Measurement.** 50k fixture in RAM, the pin removed before every miss, three
rounds: miss 7.7–8.6 s, hit 3.3–3.5 s; the staged report byte-identical
across miss, hit and `--no-cache`. The pin for that fixture is 24 MB: it holds
94,998 findings (45,000 dead files and 49,998 structural clones — the
generator's shape, not a project's) at about 280 bytes each; vite's 831
findings would pin under a quarter of a megabyte. What a hit still pays is the
index side alone: its materialization (0.8 s in RAM), its warm analysis
(discover 0.32, assemble 0.47, analyze 0.50) and the process, composition and
render around them. Two gates hold the contract — the engine's (a pinned side
composes the byte-identical comparison, is found only under its own identity,
never through a cache that is off) and the CLI's (a second `--staged` reads
the pin back and writes no second one; the bytes match with the cache off).

## 2026-09-02 — A fully staged worktree stands in for the index; in-memory trees declined

**What.** `--staged` materialized the index tree on every run, whatever the
worktree held. When the worktree already is the index as discovery sees it —
no tracked file differs from the index in content or presence (`git diff
--quiet`), and no untracked file is visible under the tree's own `.gitignore`
files (`git ls-files --others --exclude-per-directory=.gitignore`) — the CLI
judges the worktree in place, with its own cache, and materializes nothing.
The check is conservative by construction: `.git/info/exclude` and the global
excludes are machine state discovery never consults, so a file only they hide
still counts; a file only `.ignore` hides, or a hidden entry the walk would
skip, counts too — each costs a materialization, never a wrong tree; `.kndo/`
is kndo's own and never analyzed. Everything else takes the road it always
took.

**Measurement.** 50k fixture in RAM, everything staged, base side pinned,
the two binaries alternated for three rounds: materialized index 3.1 s (a
first round at 7.0 s, warming), in place 2.1–2.2 s — against a plain warm run
of 1.8 s, the difference being the base pin's read and the composition.
Byte-identical: in place against materialized, and the fallback road (an
untracked file in sight) against both. The git edge's unit test walks the
decision — clean, unstaged edit, staged, untracked, gitignored untracked,
`.kndo/`, unstaged deletion — and the CLI test holds the bytes across the two
roads. The day's arc for `--staged` at 50k in RAM: 14.6 s → 8.9 (the shared
cache) → 3.3 (the pinned base, on a hit) → 2.1 (in place).

**In-memory trees, declined with the number.** After the shared cache, the
pinned base and this shortcut, materialization survives on two roads only —
the base side once per HEAD (a miss) and the index side under partial staging
or an untracked file in sight — at 0.8 s per side in RAM. Reading `git
archive` into memory instead would need a second door into a session (a
pre-discovered tree) and a second implementation of the ignore semantics the
filesystem walk owns — `.gitignore`, `.ignore`, the hidden opt-in — held
equivalent by a gate over every fixture and this repository. A second
implementation of the one algorithm the determinism law wants once, for
0.8 s on the uncommon roads: dead. The measurement that reopens it is a
`--staged` run on a disk slow enough that the fallback road's `tar -x`
dominates again — the container's disk did that once today (19 s for one
materialization), which is why the shortcut removes the common road's
materialization entirely rather than making it cheaper.

## 2026-09-05 — M8: evidence declared, structure in the engine (the root-cause redesign, approved)

**What.** The nine-adapter audit (2026-09-04/05; seven auditors, each adapter
crossed against its grammar's node inventory and its language's primary
sources, the top findings reproduced with the release binary) left the verdict
matrix with WRONG and GAP in every column. The owner asked for the definitive,
root-cause design and approved it on 2026-09-05. Its thesis: an adapter reports
what its file and its manifest SAY — the namespace clause, markers (attributes,
annotations, decorators), relations (extends, conforms, implements, overrides),
qualified references (`pkg.Name`, `super::x::f`), the timing of every import
(load, lazy, erased), mounts (`mod x;`), embedded regions (`<script>`), and
manifest evidence (units with roots, excludes, entries, friends and publication;
packages; dependencies with normalized requirements; path aliases; ignores) —
and never a conclusion computed from a path convention. The engine owns the
structure in three deep modules: `Project` (units, friendship, publication,
aliases, ignores, file roles), `Scopes` (the forest project → unit → namespace →
file → owner, with structured `Reach`, effective reach capped by the owner,
pools by subtree plus friends, and the language's ladder), and `Dispatch`
(declarative rules marker/relation/name/witness → root, witness, exemption,
generated). Framework knowledge is data: JUnit, TestNG, Spring, Lombok, XCTest,
swift-testing, SwiftUI/UIKit, rstest and kin, pytest/Django/Flask, Storybook
ride rule packs — conduct extensions that declare rules and an activation
(`ManifestDependency`, `FileExists`, the new `FileImports`) and nothing else.

**Five root causes, each with the element that removes it.** (1) Visibility
without structure — `Reach::Scoped{token}` plus a per-file region the adapter
computed from paths — becomes the scope forest and a structured `Reach`
(`Owner`, `File`, `Namespace{up}`, `Unit{up}`, `Directory{up}`, `Named`,
`Inherited`, `Exported`); `sees` and `seen_from` die. (2) Project knowledge
re-derived by convention (SwiftPM `path:`, Maven/Gradle source sets, tsconfig,
`sys.path`, the go tool's ignores, Gradle catalogs, `publish = false`,
`private: true`) becomes one hook, `extract_manifest`, writing
`ManifestEvidence` through a `ManifestSink`; `roots`, `packages`,
`manifest_dependencies` and `manifest_mentions` die. (3) Dispatch by convention
in code (closed text tables of rooting attributes in every adapter) becomes
markers and relations as evidence plus dispatch rules; the language's defaults
ride its spec, frameworks ride packs. (4) The engine not seeing through imports
(a namespace or glob import kept a whole surface; `EntrySurface` kept private
members; every production file was a whole-file root; cycles counted edges
that never run) becomes qualified references, glob pooled by name, the
published surface per unit, import timing and rewritten keepers where
`Owner`/`File`-reaching members never ride. (5) No gate for what the grammar
offers or what a fixture claims becomes a generated grammar-inventory ledger
per adapter, expectations as pins (`expectations.toml`), a loud-change gate,
vendored grammars with patch files, and an ERROR-tolerant item walk.

**Removed layers.** `Extension::sees`, `Extension::seen_from`,
`Reach::Scoped{token}` (after the adapters migrate), `narrowable_scopes` and
`export_narrowing` (folded into the ladder), the four manifest hooks, the
whole-file library root spelled in nine adapters (the published surface is a
unit's), html's second `TypeScriptAdapter` and hand-written JavaScript scanner
(embedded regions), the line scanners of pom, Gradle, pyproject and go.mod
(structural parsers), and the toolkit's second copies (`parent_dir`, the loader
suffix strip, generated needles, comment markers, the file-role block).

**Execution rule.** The capability law stands: a contract growth lands with its
default, its named consumer in core and its conformance case, in one merge. So
the milestones are vertical slices, not a contract-first cutover: M8.a adds
expectations and the loud-change gate, import timing with `cyclic` on load-time
edges, and markers/relations with `Dispatch` (Rust first, the dogfood); M8.b
lands `ManifestEvidence`, `Project`, `Scopes` and the keepers; M8.c migrates the
adapters one by one (rust, go, java+kotlin, swift, python, js-ts, html+css),
each deleting its convention code and adding the audit's fixtures; M8.d the
structural manifests; M8.e the rule packs; M8.f the grammars (vendoring with
patches, the ledger gate, the Kotlin bake-off on Exposed); M8.g the close-out,
whose exit criterion is the audit re-run over the new tree with OK in every
cell. Old hooks stay as defaulted bridges until the last adapter migrates, so
the tree is green between slices. The contract fingerprint, the graph semantics
version and each adapter's version move once each, in the slice that earns it.

**Measurements the design rests on** (audit, verified with the release binary):
Swift's test containers alone are 161 of the corpus's 218 `unused` (Alamofire
113 of 141, vapor 48 of 77); 37 of Alamofire's 41 `untested` follow from
`import Alamofire` never resolving under `path: "Source"`; guava: 588 of 1,751
main-tree `internal-only` advise narrowing members its same-package tests use,
and 914 private methods plus 535 private nested classes are invisible under the
private-member keep; Exposed: 61 of 802 files (7.6%) parse with errors under
kotlin-ng 1.1.0 and keep about a fifth of their declarations; flask's cycle
component drops from 20 files to 9 over load-time edges only; gin: 72 grouped
`var` names invisible, a phantom declaration named `,` on every multi-name
`const`. Eleven fixtures carried comments contradicting their pins with no
DECISIONS entry (js-ts 4, go 3, swift 2, java/kotlin 2).

**Naming.** The manifest payload is `ManifestEvidence`, the name this file gave
it on 2026-08-29 (evidence, never facts); the hook is `extract_manifest`, the
mirror of `extract`. A fixture's claims are **expectations**, not claims — that
word already names file claiming by suffix. The glossary lands as `CONTEXT.md`
with this entry.

**Three owner decisions, taken 2026-09-05.**

1. `#[allow(dead_code)]` and its kin (`@SuppressWarnings("unused")`,
   `@Suppress("unused")`, `eslint-disable … no-unused-vars`) are honored. At
   item level as `Effect::Exempt`, visible as a keeper (`kept-by: exempt
   allow(dead_code)`): the author already declared the intent, and accusing
   what the compiler silences on request is noise by definition. A blanket
   allow at file or crate level is honored too and reported as a run
   diagnostic naming the count it hides. `kndo:allow` stays the channel common
   to every language and the only one that suppresses categories other than
   `unused`.
2. Python's `_x` reaches the distribution's root package (`Unit{0}`): PEP 8's
   "internal use" — `mod._x()` from a sibling module is legal and common, so it
   is never accused; an `_x` unused across the package is. No narrowing advice:
   the ladder is empty because no keyword exists. The alternative (`Exported`
   with a warning) would make no `_x` accusable inside a published unit — the
   one class of dead code Python reports today.
3. Go methods are declared, always, as members of the receiver's base type;
   accusation follows reach. Measured on gin (98 files) before deciding: 406
   methods (368 exported, 38 not); 146 match a method name of a project
   interface and 85 a common standard-library interface (`String`, `Error`,
   `ServeHTTP`, `MarshalJSON`, …); only 9 are never called as `.Name(`
   anywhere, and the 6 of those matching no interface at all are
   `CreateDecoder`/`CreateEncoder`/`IsEmpty` in `binding/json_test.go` — the
   structural dispatch through EXTERNAL interfaces that retired the class in
   M4.c. The rule the number yields: an exported method satisfies interfaces
   kndo cannot see (other packages, the standard library), so it is declared
   and carries the language's `Possible` root ("structural dispatch") — it
   enters the graph, `describe`, `used-by`, `trace`, the metrics and
   `duplicate` (which was blind to Go method-body clones, a recorded
   under-report) and is never accused; an unexported method can only satisfy
   interfaces of its own package, all visible, so it is judged with a witness
   against them and accused when nothing names it. Expected on gin: zero new
   accusations, 406 declarations with an owner. The same shape Swift and Java
   already have: the whole graph tells the story, reach decides the accusation.

## 2026-09-05 — M8.a: expectations are pins, contract changes are loud, imports carry their moment

**Expectations.** Every conformance fixture now carries `expectations.toml`
beside its byte pin: `dead` (must be reported), `alive` (must not be accused
by `unused`/`internal-only`, or by the named category) and `known_gap` (the
claim the tree cannot honor yet, with the fix that will). The
`fixture_expectations_hold` gate checks them against the run, and a known gap
fails the day the tree closes it, so the ledger cannot rot. The eleven
fixtures whose comments contradicted their pins are now gaps with their audit
ids — js-ts `cjs-dead-object-export`, `cjs-interop`, `dynamic-import-scan`,
`namespace-members`; go `internal-only-unit`, `go-work-phantom-dep`; swift
`dispatch-and-extension`, `visibility-ladder-and-internal-default`; java
`coverage-jacoco`, `visibility-ladder-and-nested-members`; kotlin
`dead-code-same-package` — plus the apple `apple-bundles` PreviewProvider pair
the audit's Swift row 20 named. Subjects use the query contract's spelling, so
`kndo describe` and an expectation name a thing identically; a spelling nothing
declares is refused. The `contract_changes_are_loud` gate closes the other
half: a commit range that changes a pinned report, the contract fingerprint or
`GRAPH_SEMANTICS_VERSION` must append to DECISIONS.md and name what moved (the
fixture, or `<crate> fixtures`, or `every conformance fixture`); CI diffs the
pull request's base..head with a full checkout.

**Timing.** `Import` gains `timing: Timing` — `Load`, `Lazy`, `Erased` — a fact
the adapter reads off the syntax: a static import, a top-level `require`, a
`use` or `mod` load; a dynamic `import()`, a guarded or function-scoped
`require`, a Python import inside a function run later; `import type` and a
`TYPE_CHECKING` block never. The sink grows `import_at`; `import` stays and
means load-time, so an adapter that never learned the field reports what it
always did. `ImportShape::TypeOnly` is gone — a type-only import is its
bindings at `Erased`, the same evidence with only its moment different; the
wire keeps `type-only` until the ABI's next version carries the field, and
the host and SDK translate. The consumer is `cyclic`: strongly connected
components over load-time edges only, because an initialization hazard needs
initialization; reachability keeps every timing (a type used is a type kept).
Conformance: `type-only-cycle` (js-ts) and `type-checking-cycle` (python), each
pinning the erased/lazy pair silent and its value-import control accused; the
kmock language speaks `lazy-import`/`erased-import` for the engine test.

**Measurement.** vite cyclic 31 → 28 (the type-only `module-runner` loop
gone, the 88-file `node/` tangle down to its 48-file value core plus one
2-file pair it had swallowed, three dynamic-import loops in test fixtures and
the playground gone); flask 3 → 3 with the 20-file knot down to 9 load-time
files, `json/__init__.py ↔ provider.py` surfacing on its own, the celery
example's function-scoped loop gone, and the `__init__.py → app.py` loop
`Certain` instead of `Possible`. Every other repo byte-identical; every
existing fixture byte-identical (the fingerprint moved for the field; the
reports did not). The contract fingerprint is regenerated once for the slice.

## 2026-09-05 — M8.a: markers are evidence, dispatch rules are their meaning

**The shape.** `FileEvidence` gains `markers` — an attribute, annotation,
decorator or pragma on a declaration (by id) or on the whole file, with its
path and top-level arguments as written — behind the declared
`EvidenceStream::Markers` and the sink's `marker`. What a marker MEANS is no
longer an adapter's branch: the spec declares `dispatch` rules — a trigger
(a marker path pattern, `*` matching any run, optionally with an argument
pattern) and an effect (a root of a color, or an exemption from `unused`)
with a confidence — and the engine's `dispatch` module derives roots and
exemptions from every file's markers under its claiming extension's rules.
Dispatched roots live on the graph beside the manifest anchors (never in the
evidence cache, so a rule change re-dispatches without re-extracting), and
`GraphFile::roots()` is the one iteration every color judgment reads:
`GRAPH_SEMANTICS_VERSION` 10 → 11. `used-by` shows a dispatched root as
`dispatch:<color>` and an exemption as `exempt`; an exemption is listed first
and alone it keeps. Exemptions are lexically scoped, as lint attributes are —
the marked declaration and everything declared within its extent — and a
file-level one covers the file's every declaration and lands in the report's
diagnostics, so the silence is visible. The contract fingerprint is
regenerated for the field; the report schema is regenerated because the
stream enum grew.

**The first language.** kndo:rust (version 7) reports every attribute as a
marker: the path as written, arguments split at top-level commas with
whitespace collapsed, `#[unsafe(no_mangle)]` unwrapped to the attribute
inside, a `cfg` predicate flattened to its atoms (`all`/`any` transparent,
`not` as a `!` prefix), inner attributes on the file or the inline module they
sit in, an `impl` block's attributes riding every member. Its rules replace
the adapter's root table: `test`, `*::test`, `bench`, `*::bench` and
`cfg(test)` root Test; `*::main`, `no_mangle`, `export_name`,
`global_allocator`, `panic_handler`, `alloc_error_handler`, `used`,
`proc_macro`, `proc_macro_derive`, `proc_macro_attribute`, `start` root
Production; `allow`/`expect` of `dead_code`, `unused` or `warnings` exempt —
the owner's 2026-09-05 decision, honored as a visible exemption rather than a
second suppression channel. Every existing rust fixture stays byte-identical:
the roots the table used to state are the roots the rules now derive. Two
fixtures land: `attribute-dispatch` (rust) pins a linkage root through the
unsafe spelling, two exemptions, a dead control and the file-level blanket
with its diagnostic; `crate-level-allow` (rust) holds the one gap this slice
knows as a `known_gap` with teeth — rustc scopes a crate root's inner
attribute over the whole crate, and today the marker covers only the file
that carries it; M8.b's units will dispatch a unit root's file-level markers
over the unit, and the gap fails the day that lands.

**Measurement.** ripgrep 155 → 154: `unused` 4 → 3, the const
`SHERLOCK_CRLF` in `crates/printer/src/standard.rs` exempt by its own
`#[allow(dead_code)]`. Of the three that remain, two —
`Handle.read_write` and `Handle.read_write_mut` in `crates/index/src/index.rs`
— sit under the crate root's `#![allow(warnings)]` in `crates/index/src/lib.rs`:
exactly the crate-level gap above, measured. No new diagnostics: that crate
root declares nothing itself, so no blanket note fires. Every other repo
byte-identical; every existing conformance fixture in every corpus
byte-identical.

**Deferred, named.** Relations and witnesses (the other half of the design's
dispatch triggers) wait for the JVM adapter migration in M8.c, where their
first consumer lives; the wire carries neither markers nor timing until the
ABI's next version, one toll at M8.a's close — the SDK omits the stream from
a guest's declaration so host-side pairing stays truthful. The report's
`extensions` rows do not list dispatch rules: a count says nothing, the rules
themselves belong to an extension listing verb, M8.g's docs decide where. The
dogfood gained one self-retiring allow: the engine tests' shared helpers in
`crates/kndo/tests/common/mod.rs` are a module of a test target, and the
`test-only` analysis reads a file only tests reach as production code until
M8.b's file roles land — the allow turns `stale` that day and the dogfood
gate says so.

## 2026-09-05 — M8.a closes: the ABI toll carries timing, markers and dispatch rules

The wire mirrors the contract again, field for field. `%import` gains
`timing` and the `type-only` shape is gone (a type-only import is its bindings
at `erased`, the same evidence with only its moment different — the host's and
the SDK's translations retire with it); `file-evidence` gains `markers`;
`evidence-stream` gains `markers`; `extension-spec` gains `import-cycles` and
`dispatch` — the two facts a component needs for its markers and its timing to
have a consumer (without `hazard`, a lazy edge crossing the wire could never be
told from a load-time one by any judgment). The host replays markers through
the real sink like every other write, indices validated into ids or dropped
with a diagnostic, and turns wire rules into the spec's `DispatchRule`s, so a
component's markers are dispatched by the same engine module as a built-in's.
In-place growth of the records, as the pre-freeze policy allows; the four
reference pins under `abi/compat/` are rebuilt in the same commit via
`cargo xtask pin-abi`. The toll found a vice on its way: the host replayed
wire evidence through a validating sink and then TRANSFERRED it into the
engine's sink field by field — and that hand-written transfer had already
dropped every import's timing in silence, invisible only because no
component declared `hazard`. The transfer is gone: the wire replays straight
into the engine's own sink, primed with the same declared streams, so there
is one enumeration of the evidence and it lives in the contract. The `kmini` reference guest (version 2) speaks
`@path` and `@!` marker lines, `lazy use`/`type use`, declares `Markers`,
`hazard` and two rules (`@test` roots Test, `@keep` exempts); the compliance
suite drives all of it fresh from source, and the compat matrix drives it from
the pinned bytes — a keep, a dispatched Test root, a lazy loop that is no
hazard beside a load-time loop that is, and `used-by` answering `exempt` and
`dispatch:test` for evidence that crossed the wire. Still not on the wire, each
defaulting to silence as before: export narrowing, dependency scoping,
identity, importers and builtins — M8.b's contract reshapes reach and retires
narrowing, so their toll waits for that shape. No pinned report moves: the
change is the wire's alone.

## 2026-09-06 — M8.b.1: a manifest states the project, and the engine owns the structure

**The hook.** `Extension::extract_manifest` is the mirror of `extract`: one
manifest in, `ManifestEvidence` out — units (name, kind, source roots,
excludes, entries), packages, dependencies, mentions, and the files the
manifest runs — written through a `ManifestSink` that validates at the call
site. It replaces four hooks that each re-parsed the same file (`roots`,
`packages`, `manifest_dependencies`, `manifest_mentions`) and, with them, the
convention tables nine adapters carried to guess what the manifest already
said. Until every adapter has moved, the engine reads BOTH and merges: an
adapter populates one side or the other, never both, so the union is disjoint
and the bridge retires by deleting four lines beside the hooks it feeds — no
flag, no capability bit, nothing to get wrong in between.

**The structure.** `kndo-core`'s new `project` module owns what the manifests
said. `Project::unit_of` answers which unit compiles a file — the deepest
source root that contains it, excludes honored, the earlier unit breaking a
tie — and that answer rides the graph as `GraphFile::unit`. A unit's entries
anchor roots of the color its KIND implies (`UnitKind::color`: library and
executable are production, test and bench are test, example and tooling are
tooling), so the nine per-adapter root tables have one home and one rule. A
unit entry is the manifest's own statement and anchors `Certain`; an adapter
that merely guesses an entry reports it as a manifest root carrying a
confidence of its own. Manifests are now read ONCE per run: packages,
dependencies, mentions, units and roots all come from the same value, where
five walks over the manifest set used to each re-ask an adapter.

**Measurement.** Every corpus report byte-identical, all nine repositories,
and every conformance fixture likewise — no adapter emits a unit yet, which is
exactly what M8.c is for. The one delta the corpus DID surface was a defect
this slice introduced and the measurement caught: rewriting `Graph::package_of`
onto the promoted `is_under` flipped its tie-break from first-wins to
last-wins, and Exposed's `documentation-website/.../pom.xml` — a directory two
packages name, `com.example:exposed-modules-maven` from the pom and
`exposed-modules-maven` from the settings file — changed which one owned its
files. The rule is now explicit in the code and pinned by a test.
`GRAPH_SEMANTICS_VERSION` 11 → 12: the same evidence assembles into a graph
that carries a project.

**Promoted.** Directory containment had three hand-rolled spellings (a
package's ownership, a manifest's reach, and the new unit roots); it is one
now, `ProjectPath::is_under` / `vocab::is_under`, with the separator rule that
keeps `src/apple` from owning `src/apples/x.rs` stated once and tested once.

**Conformance.** The kmock language grew a `kmock.pkg` manifest that declares
units (`unit core library roots=src entries=src/lib.kmock`) and files a
manifest runs, and `crates/kndo/tests/units.rs` pins the three consequences: a
test unit's entry colors what only it reaches `test-only` rather than dead, a
file belongs to the unit whose source root is deepest with an exclude removing
it from that unit rather than handing it upward, and a unit entry anchors
`Certain` where a run file carries the adapter's own confidence.

**Deferred, named.** `friends` and `publication` on a unit, path aliases and
ignores are all in the design and none has a consumer until the scope forest
and the keepers land — they arrive in the slice that reads them, per the
capability law, rather than sitting in the contract as unread fields.

## 2026-09-06 — kndo:kotlin (3): a parameter's default value is a use

The binder rule excluded every identifier directly under a `class_parameter`
or `parameter`, which is right for the name being bound and wrong for
everything after the `=`: `iterations: Int = DEFAULT_ITERATIONS` reported
`Int` (it sits inside a `user_type`) and dropped `DEFAULT_ITERATIONS`. Only
the FIRST identifier child is the binder now; the rest is the default
expression. Exposed gains 148 references and not one finding changes, because
the constants this hid were kept anyway by a surface keeper that hands out
private members — which is the defect the next slice removes, and which would
have turned all seventeen of Exposed's crypt-hasher constants into false
accusations the day it did. Every conformance fixture byte-identical.

## 2026-09-06 — kndo:java (4): annotations are markers, and the source's own suppression is honored

Java's annotations now ride the markers stream — the name as written
(`Override`, `org.junit.Test`) and its arguments split at the top-level commas,
so a brace initializer (`{"unused", "rawtypes"}`) is ONE argument, as the
grammar has it. Their meaning moved to the spec: `@Override` roots Production
(`Probable` — an override nobody calls is still dead one supertype up), and
`@SuppressWarnings` naming `"unused"` exempts, the owner's 2026-09-05 decision
reaching its second language. The exemption is lexically scoped like the lint
it mirrors: a class-level suppression covers the class's members, which is
exactly how javac reads it.

**Measurement.** guava 9,925 → 9,837: 88 `unused` findings gone, every one in
a file whose `@SuppressWarnings` names `"unused"` — guava's reflection-tested
helpers, whose own comment reads "many methods tested reflectively". Nothing
else moved on any repository, and every conformance fixture is byte-identical:
the `@Override` root is the same root, derived by rule instead of stated by
extraction. No new diagnostics, because a class-level suppression is a
declaration marker with a lexical extent, not a blanket over a file.

The argument splitting the Rust adapter wrote in M8.a is promoted to the
toolkit (`split_arguments`, `normalize_whitespace`) on its second use, per the
second-copy rule: depth-zero commas and opaque string literals are grammar
knowledge no adapter owns.

## 2026-09-06 — M8.b.2: a surface hands out what the language exports, and nothing else

**The rule.** A member whose OWN reach is `Private` no longer rides any surface
keeper — not a whole-surface (namespace, glob, side-effect) importer, not an
entry point's exported API, not its owner's binding. Its name cannot be spelled
outside the file that declares it, so neither surface can hand it out; the
`OwnerBinding` keeper already knew this and the other two did not. What still
keeps such a member is unchanged and generous: dispatch is not lexical, so ANY
reachable reference to its name counts. Cause (4) of the M8 design, the half
that needed no adapter migration.

**Measurement, and what it cost to earn.** The first run of this rule produced
47 new `unused` findings, and the sample said 27 of them were false — so the
rule was reverted and its two blockers fixed first, each measured on its own:

| step | Exposed | guava | vapor |
|---|---|---|---|
| the rule, first attempt | +17, all false | +28, 10 false | +2, both true |
| kotlin: a parameter's default value is a use | −13 of those | — | — |
| java: `@SuppressWarnings("unused")` exempts | — | −88 existing, −11 of those | — |
| kotlin: `$name` in a string template is a use | −4 of those, −1 existing | — | — |
| the rule, shipped | +0 | +17 | +2 |

Nineteen accusations, every one verified against the source: guava's
`ForwardingCacheTest.OnlyGet`, `Fingerprint2011Test.MAX_BYTES`,
`FuturesTest.MapperFunction` and kin — private test helpers whose names appear
exactly once in their file — and vapor's `insertOrReturn` and
`checkBodyStorage`, each declared once and never called. Zero false positives
survive the three fixes. Every conformance fixture stays byte-identical: not
one pinned fixture had a private member riding a surface.

**What the measurement was really for.** Removing a blanket keep is a lens: it
shows which adapters under-report. All three defects it found — Kotlin's
constructor defaults, Kotlin's simple string templates, Java's unhonored
suppression — were pre-existing under-reports that the surface keep was
hiding, and each is fixed at its root rather than papered over by keeping
everything alive. That is the exchange this milestone is for: the engine stops
guessing generously, and the adapters start reporting completely.

**Conformance.** The kmock language gained types and members (`type Widget`,
`pub member Widget.shown`, `member Widget.hidden`), so the engine's own
language can express the case its rule is about, and
`crates/kndo/tests/surfaces.rs` pins both halves: a private member accused
beside an exported sibling kept by both surfaces, and the same private member
kept the moment any reachable file references its name.

## 2026-09-06 — M8.b.4: a supertype is a promise, and a member on it is a witness

**The evidence.** `FileEvidence` gains `relations` behind the declared
`EvidenceStream::Relations`: a typed link from a declaration to a NAMED type,
`Extends` or `Implements`, with generics and qualification stripped so it
resolves the way a reference to that type resolves. The name stays unresolved
on purpose — an adapter that resolved it would be re-deriving the project.
kndo:java (5) emits them from `extends`/`implements` clauses. The wire carries
them in the same shape (`relation`, `relation-kind`, `file-evidence.relations`),
pins rebuilt in this commit, so the ABI keeps mirroring the contract rather
than owing a second toll.

**What the engine reads.** The navigation index gains three name-keyed maps,
built over EVERY file (a supertype in a file no root reaches still shapes what
its subtypes must declare) and walked transitively:

- A member whose owner promised a type declaring the same name is a
  **witness** — an override, an interface method, a protocol requirement — and
  is kept, because no call site can be required to exist: every caller holds
  the SUPERTYPE and dispatches through it. `used-by` says `witness`.
- A member some subtype declares is **overridden**, and `internal-only` says
  nothing about it: narrowing it below what its overriders need is a compile
  error, not advice. Both directions of one fact, which is why one stream
  answers both.

Same-named types union their surfaces — an over-approximation, and in the
keep-alive direction for both consumers.

**Measurement.** guava 9,854 → 9,620: **234 `internal-only` advisories gone**,
zero findings added, nothing moved on any other repository. The measurement
that ordered this work: of guava's 3,305 advisories, 1,855 name a symbol
appearing only in its own file (true advice), and the largest false class was
package-private members a same-package sibling OVERRIDES —
`AbstractMultisetSetCountTester.setCountCheckReturnValue` overridden by
`MultisetSetCountConditionallyTester`, and 233 like it. A textual estimate put
that class at 326; the engine's precise answer, with relations resolved and
the type graph walked transitively, is 234 — the difference being unrelated
same-named methods the text could not tell apart. The contract fingerprint and
the report schema move for the new stream; every conformance fixture is
byte-identical.

**Conformance.** kmock speaks `extends`/`implements`, and
`crates/kndo/tests/surfaces.rs` pins the keeper: a member with no reference
anywhere, kept by the promise alone, beside a sibling promising nothing that
is accused. The java fixture `overridden-package-member` pins the other
direction end to end: `Shape.area` and its override `Square.area` both silent,
`Shape.onlyHere` — package-private, used only in its own file, overridden by
nothing — still advised.

## 2026-09-06 — M8.b.5: a namespace is its name UNDER its source root, and a rung carries its language's word

**The scope forest gets its first layer.** `Reach::Namespace { up }` replaces
the adapter-computed region token for java: a file declares the segments it
lives in (`package com.foo;` → `["com", "foo"]`, segments and not a joined
name, so the engine never learns a language's separator), and `Scopes` keys a
namespace node by `(source_root, segments)` — the file's directory minus the
suffix its own segments spell. Java's convention falls out of the arithmetic
instead of being written down: a file at `src/main/java/com/foo/A.java`
declaring `com.foo` has source root `src/main/java`, and a layout the
convention does not fit roots at its own directory rather than guessing.

**The wide pool was tried first, and its number killed it.** A namespace keyed
by segments ALONE — every file spelling `com.google.common.collect` in one
pool, wherever it sits — took guava from 9,620 to 6,626: `+0 -2994`,
`Counter({'internal-only': 2961, 'unused': 33})`. Decomposing the 2,961
silenced advisories by WHERE the disqualifying sibling lives: **2,653 in a
different source root**, **263 in the same build's test tree**, **45 in the
same source root**. The 2,653 are guava's `android/` mirror — a SEPARATE
compilation of the same sources, whose files can no more see each other's
package-private members than any two unrelated projects can. Buying 263 true
fixes by silencing 2,653 correct advisories is the trade the oracle would have
made; v2 does not. The 263 need units — which files one compilation actually
contains, from Maven and Gradle — and arrive with M8.d, not with a looser
scope.

**Measurement of what shipped.** Source-root-scoped, guava's 9,361 findings
are identical to the previous run in every field: same ids, same messages, same
spans. The structure changed underneath and the numbers did not, which is the
result a refactor of this kind should produce.

**A rung carries its language's word.** The ladder was `Vec<Rung>`, and
retiring `narrowable_scopes` would have moved 3,018 java messages from
"declared `package`-scoped" to "declared `namespace`-scoped" — the engine's
vocabulary leaking into a report a Java developer reads. The ladder is now
`Vec<Step>`, one type pairing the rung every judgment reads with the word only
reports say, so the two can never name different things; java declares
`private`/`package`/`public`. `From<Rung>` supplies the engine's own word for
a component that declares rungs alone, and the match is exhaustive on purpose:
a new rung will not compile until it has one. The report's extension row
carries the ladder, restoring the answer to "why does `internal-only` fire for
this language" that the retired capability used to give — and giving more, since
the word tells the reader what to type. Report schema, contract fingerprint and
the eight java conformance fixtures move for that row; no finding moves.

## 2026-09-06 — M8.b.6: a unit is what a manifest compiles, and a package spans the classpath

**The prize this was measured for.** guava's tests are not in `src/test/java`;
they are a SEPARATE Maven artifact, `guava-tests`, that declares a dependency
on `guava`. Java's package-private access does not stop at an artifact
boundary — two jars contributing to `com.google.common.math` on one classpath
see each other's package-private members — so
`LongMath.FLOOR_SQRT_MAX_LONG`, used as exactly that from `LongMathTest`, was
being advised to narrow. Following that advice breaks the build. No
`src/main` ↔ `src/test` mirror rule reaches this: the two live in different
modules.

**What the manifests now say.** `Unit` gains `depends_on` — the units it
compiles against, NAMED as the manifest spells them, because the manifest
declaring a dependency has not read the manifest declaring the unit and cannot
spell a path it never saw. `ManifestEvidence` gains `members`: the manifests
one aggregates (Maven's `<modules>`, Cargo's `workspace.members`, Gradle's
`include`). The engine resolves each name — its own manifest first, so units of
one manifest can name each other, then the nearest aggregator above it — and
`Project::sees_into` answers who may name whom over the transitive closure.
Members are what make this resolvable at all: guava declares `guava` TWICE,
once per reactor, and `guava-tests` means the sibling its own aggregator lists.
`kndo:java` reads it from `pom.xml` through a shallow depth-tracking element
walk, which is what keeps `<parent>`'s artifact id and
`<dependencyManagement>`'s dependencies out of a project's direct children.

**The capability, because core cannot know.** `NamespaceSpan` says how far one
namespace reaches: `Compilation` for Java's package, `Unit` (the default, and
the narrower answer) for Go's package and Rust's module tree, where two units
spelling one name hold two unrelated namespaces. Consumer: `Scopes`, which
keys a namespace node by `(compilation, segments)` and unions the nodes whose
unit compiles against this one's. Conformance:
`crates/kndo/tests/surfaces.rs` pins both directions on kmock, and the
`kndo-adapter-java` fixture `package-private-across-modules` pins it on two
real Maven modules.

**Measurement, and the half of it that is debt.** guava 9,620 → 9,011; every
other repository byte-identical (Exposed is Gradle, which states no units yet,
so its files fall back to what each declaration implies). Of the 589 findings
retired — 577 `internal-only`, 12 `unused`, none added — 321 are corroborated:
the file whose use silences the advisory also names the member's owner, and the
three sampled by hand (`LongMath.FLOOR_SQRT_MAX_LONG`,
`BloomFilter.optimalNumOfHashFunctions`, `TreeRangeSet.rangesByLowerBound`) are
real cross-artifact uses that the advisory was wrong about. The other 268 are
silenced by a BARE NAME: `internal-only` disqualifies on a name occurring in a
pool file, with no owner, so `GcFinalizationTest`'s local `CountDownLatch
latch` silences `FinalizableReference.latch`. Ten of ten sampled from that
class are coincidences — locals, parameters, doc comments.

Shipped anyway, and the reason is the direction of the error. A wrong
accusation tells a developer to narrow a member their own test module uses;
a wrong silence loses an advisory. Precision over recall is this project's
standing choice, and the widening is never in the accusing direction: zero
findings added, on any repository. The 268 are the name-only reference test's
bill, not the scope model's — the scope model is now what javac does — and
qualified references is the next slice, which will be measured against exactly
this number.

`GRAPH_SEMANTICS_VERSION` 12 → 13: manifest evidence moves the graph knob and
not the contract fingerprint, which is why the fingerprint and the report
schema are unchanged here. `kndo:java` 6 → 7 for the manifest evidence it now
emits. One fixture is added, `package-private-across-modules`, and no existing
fixture moves — so the kndo-adapter-java fixtures this range touches are that
one plus the eight the previous entry moved for the ladder row. That entry
called them "java conformance fixtures", which is not the vocabulary the
loudness gate reads; naming them here closes the range.

## 2026-09-06 — M8.b.7: a member is reached through an access, not by spelling its name

**The debt the last entry named, paid.** `internal-only` disqualified a
declaration when any file in its pool spelled the NAME — no owner, no receiver
— so `com.google.common.cache`'s advisory for `AccessQueue.head` died on a
comment reading `// head`, and `FinalizableReference.latch` on a test's local
`CountDownLatch latch`. Widening the pool to the compilation made that bill
visible: 268 advisories lost to coincidence on guava alone.

**What a language now says.** `EvidenceStream::Qualifiers` — an adapter
declaring it reports, per reference, what the name was read FROM:
`Some("LongMath")` for `LongMath.FLOOR_SQRT_MAX_LONG`, `Some("queue")` for
`queue.head`, `None` for a bare name. Java's `this` and `super` report NOTHING
on purpose: a name reached through them is reached the way a bare name is, from
the enclosing declaration and whatever it inherits, so calling them a receiver
would say a member was named from outside when it was named from inside. Method
references (`Foo::bar`) name `bar` on `Foo` as surely as a call does, and read
the pair by position because the grammar gives it no field names.

**What the engine now asks.** A MEMBER of another file's class is reached
through an access, through an import binding (Java's static import already
lands there), or through the inheritance that puts it in that file's own
scopes — so a bare name disqualifies only from a file declaring a SUBTYPE of
the owner, transitively, over the relation stream that already answers
witnesses and overrides. A nested TYPE is exempt: it is named bare, after an
import or from inside its own package. And the stream is the REFERENCING
file's to declare — a file whose adapter is silent keeps its bare names
counting, which is what makes a mixed-language package degrade instead of
break.

**Measurement.** guava 9,011 → 9,741: **730 `internal-only` advisories
recovered, zero removed** (692 by distinct finding id — see the note below),
every other repository byte-identical (only java declares the stream). Fifteen sampled by hand, fifteen true: the outside
occurrence is another class's identically-named member
(`ImmutableListMultimap.fromMapEntries`, `RegularImmutableBiMap.MAX_LOAD_FACTOR`),
a test double's own field (`LocalCacheTest`'s `nextAccess`), a local
(`ExecutionListTest`'s `runCalled`), a static import of a DIFFERENT class's
method (`SerializableTester.reserialize`), or prose — a Javadoc URL fragment
`#comparators`, a comment saying "an integer". Against the pre-unit baseline
the two slices together are +121 findings on guava: 609 accusations withdrawn
because a sibling artifact really does use them, 730 restored because a
coincidence never did.

**Observed, not fixed: finding identity is not unique.** Counting these runs
by finding id and by row disagrees, because 152 ids on guava appear twice —
`FreshValueGenerator.generateRange` and 151 like it, where two OVERLOADS share
one `SymbolSelector::Member { owner, name }` and therefore one id. It predates
this work (156 such ids before the unit slice) and it means a report can show
one finding twice and health can count it twice. The fix is a selector that
tells two overloads apart, which is a contract change with its own measurement;
recorded here so it is not rediscovered.

Contract fingerprint and report schema move for the reference's new field;
`kndo:java` 7 → 8 for what it now emits; the WIT record and the four pinned
compat components move with them. Two fixtures are added,
`member-named-through-an-access` and — from the previous slice —
`package-private-across-modules`; kmock speaks `call x.name` for the access and
`call name` for the bare word. No existing kndo-adapter-java fixtures move.

## 2026-09-06 — A selector is a key: finding identity and query addresses tell every declaration apart

**The defect, at its root.** A finding's identity hashed the category and the
subject's render, and a symbol's render was `Owner.name` — so two declarations
a language legitimately lets coexist under one name shared one identity.
Measured across the corpus: guava 152 duplicated ids (277 rows), Exposed 18
(45 rows), Alamofire 21 (13 of them free functions), vapor 2, vite 1 —
overloads in Java, Kotlin and Swift, an overload signature in TypeScript. Two
consequences, both silent: a baseline naming one overload silenced every
namesake, and deleting one was never `fixed` while another stood; and the
query side, which shares the address space, answered
`describe …#FreshValueGenerator.generateRange` with "ambiguous — retry with
one of: X, X" — two identical suggestions, so the overloads were unaddressable.
`stale` had already met the same collision on suppressions and patched it
locally, with an ordinal in its discriminator: the right instinct, in the
wrong place, invisible to the address.

**The model.** Four words the glossary lacked now define it: a **Finding** is
one verdict on one subject and two findings with one identity are one finding;
a **Subject** is what a finding is about, rendered one way everywhere; a
**Selector** is the address of one declaration inside its file — owner, name,
signature, and position among the declarations sharing all three — unique
within the file by construction; a **Signature** is what a language reads
beyond the identifier to tell same-named declarations apart, as it spells it
(Java's parameter types, Swift's argument labels), never parameter names or a
return type. **Identity** is category plus subject and never the span.

**The module.** Uniqueness is a property the evidence sink now guarantees at
`finish()`: `Declaration.nth` counts the earlier declarations of the file
sharing owner name, name and signature, and a declaration's selector is built
in ONE place, `FileEvidence::selector_of`, which every analysis, the query
resolver, a fixture's expectations and a plugin's targets call. Seven copies of
"owner-or-free, then the name" — in `unused`, `untested`, `internal-only`,
`crap`, `duplicate`, `private-type-leak` and the conduct bridge — plus three
re-spellings of the render (the query's `selector_of`, the expectations'
`spell`, the gate's own match) collapsed onto it; the conduct bridge, which
resolved a plugin's target by first-name-wins, now goes through the query's
resolver and drops an ambiguous target instead of guessing. `SymbolSelector`
became a struct (`owner`, `name`, `signature`, `nth`) — the two variants were
already duplicating `name`, and would have duplicated three fields — with
`free`/`member` constructors for hand-built subjects, whose doc says evidence
never goes through them. The render is one function: `Owner.name`, the
signature verbatim, `#k` for the k-th of several a language cannot tell apart.

**The language's word.** `EvidenceSink::signature(id, text)` is how an adapter
states the signature; `kndo:java` states the parameter types as written,
`(int, List<String>, T...)`, so `Widget.size` the field and `Widget.size()` the
method are two addresses without needing position at all. Kotlin and Swift
state none yet and fall back to position — `Owner.name#2` — which keeps their
identities unique today and is the item M8.c pays when each migrates. The span
method that shared the name became `signature_span`, since a promise region
and an address are two things.

**Always, not only on collision.** Rendering the signature only when a
namesake exists was rejected: adding an overload to a baselined method would
have re-addressed the existing one, so the new method inherited the baseline
entry and the old one appeared new — a recurring silent lie. Rendering it
always costs one loud migration of every Java method's identity, which is the
sanctioned path: every java conformance fixture that names a method moved, 24
expectation subjects were respelled from the tool's own canonical answer
(`describe` accepts the bare `Owner.name` when unique and answers with the
exact address). And because the selector's wire shape changed for every symbol
subject — `{"Free": "x"}` and `{"Member": {…}}` became one object with `name`,
`owner`, `signature` and `nth` — every conformance fixture that reports a
symbol moves, in every crate, with no finding added or removed in any of them. The pins themselves are exact: `selector_exists` no longer extends the
verbs' leniency to an expectation, so `Widget.size` pinned against a tree
declaring `Widget.size(int)` names nothing and says so.

**Measurement.** Nine repositories, every finding row unchanged as a multiset
(category, subject, lines) and **zero duplicated ids** where there were 194.
guava now carries 6,084 signatures on its findings; the positional fallback
fires 117 times across the corpus and each sampled case is honest — flask's
three `@overload` stubs of `App.template_test`, guava's two `of(K, V, …)`
overloads whose parameter types spell identically and differ only in the type
variables' bounds. `describe` on the guava overload now lists two distinct
addresses and resolves each. The contract fingerprint, the report and query
schemas, the WIT `declaration` record and the four pinned compat components
move; the host now also replays a reference's receiver, which the previous
slice had added to the wire and the replay had dropped. `kndo:java` 8 → 9 for
the signatures it emits. One fixture is added, `overloads-are-two-declarations`,
pinning two dead overloads as two findings.

## 2026-09-06 — Identity closes as a family: every subject a file can hold twice carries its position, and a gate holds the line

**What the previous entry left open.** It fixed the symbol, the measured case,
and named two siblings it did not touch: `stale` kept its own ordinal in the
finding's discriminator (correct for uniqueness, invisible to the render and
the address space), and an import written twice in one file gave `unresolved`
two findings with one identity — zero in the corpus, latent everywhere. Two
mechanisms for one concept, and no rule stopping a third subject kind from
colliding silently.

**The rule, stated once.** A subject a file can hold more than once under one
spelling carries its **position** among those — an overload, the same import
written twice, the same allow written twice — so two of them are two subjects.
Identity is unique within a run by construction, and no analysis needs a
discriminator to keep two subjects apart. The glossary says so under Subject
and Identity.

**The two siblings, at their own seams.** `Import.nth` is computed by the
evidence sink at `finish()`, over the target as written, and
`FileEvidence::import_subject` is the one place an import becomes a subject —
`unresolved` calls it and re-spells nothing. `Subject::Suppression` now
carries what it allows and its position among the file's allows of the same
categories, counted over EVERY pragma rather than only the stale ones, so
fixing an earlier allow never re-addresses a later one; the ordinal left the
discriminator, which is empty again and means what it should. `Subject::label`
is the one spelling a display puts after the path — `import './x' #2`,
`allow unused #2` — and `identity_part` is what identity hashes: the same,
minus the display dressing an import wears, so an import's identity is the
specifier it always was and nothing moved for a wording change.

**The gate.** `finding_identity_is_unique` reads every pinned report — the
corpus and every conformance fixture — and refuses two findings with one id;
a `debug_assert` at the point the run's findings are final catches the same
thing in every test. A subject kind that forgets its position now fails
loudly where a baseline would have gone quiet.

**Measurement.** Nine repositories: zero identities moved (vite's 17 import
findings keep theirs, since a first import hashes as before), zero
duplicated, every row unchanged; no report file changed on disk. A stale
allow's identity moves — it now carries what it allows and no discriminator —
and no pinned report holds one. The contract fingerprint moves for the
import's position; the report schema for the two subjects; the CI workflow
for the gate. The two fixtures with import subjects are byte-identical.

## 2026-09-06 — M8.b.8: `internal-only` reads the ladder and nothing else — `narrowable_scopes` and `export_narrowing` retire, and a unit is a reach

**What was wrong.** Three facts answered one question. The Scoped rung asked
`narrowable_scopes` whether a token had somewhere narrower to go, the Exported
rung asked `export_narrowing`, and only the Namespace rung asked the ladder.
The first two were each a ladder with the words torn off: `narrowable(["module"])`
said "something below `internal` exists" without saying what, so the advice
could only read "the narrower rung would suffice", and the message repeated the
adapter's token — `module`, `crate` — which no Kotlin or Rust developer types.
And `ExportNarrowing::Expressible` conflated two facts that happen to coincide
in one ecosystem: that TypeScript spells a rung below `export` (true of Java,
Kotlin, Swift, Rust and Go as well) and that an npm package publishes through
its entries (true of npm alone) — the only reason js-ts alone could declare it
without flooding guava with advice to un-publish its API.

**The ladder is the one fact, and it says more.** `Ladder` answers the two
questions the analysis asks — the step a declaration standing on `declared`
could fall to while still covering `extent`, and what a rung is called — so no
analysis re-derives either. A `Step` now carries its `bearer` (any declaration,
a free one alone, a member alone), because Kotlin's `private` is two rungs
under one keyword (the file on a top-level declaration, the class on a member)
and Java's `private` exists for members alone; the advice never names a keyword
the declaration cannot take. The extent is read from the evidence: the owner's
rung when every same-file use sits inside the outermost declaration that owns
it (nested types share their enclosing type's private members), the file's
otherwise. What `export_narrowing` had smuggled in becomes its own fact,
`PublishedSurface`: `Exports` (the default — a jar, a crate, a Go package, a
Python distribution hand out every exported declaration, so none is advised to
narrow) or `Entries` (npm resolves through `main`/`exports`, so an export in a
file no entry reaches is internal however it is spelled). Every adapter
declares its ladder: Java `private` (members) / `package-private` / `public`;
Kotlin `private` twice / `internal` / `public`; Swift `private` /
`fileprivate` / `internal` / `public`; Rust `private` / `pub(crate)` / `pub`;
Go `unexported` / `exported`; TypeScript `unexported` (free) / `export`, with
`Entries`; Python none, by the owner's decision.

**A unit is a reach.** `Reach::Unit` is structured, and Kotlin, Swift and Rust
emit it for `internal` and `pub(crate)` (adapter versions 4, 2 and 8) — the
token could not stand on a ladder, since core would have had to know which rung
`module` means. Its pool comes from the adapter through `seen_from`, now typed
by reach, until the manifest names the unit (M8.d); `Index::pool_of` is the one
seam the keepers and `internal-only` read a reach through, and the graph keys
its adapter-bounded regions by reach (`GRAPH_SEMANTICS_VERSION` 14). Go keeps
its `package` token: its ladder spells nothing below the package, so nothing
changes until its namespace clause arrives with M8.c. The wire follows:
`narrowable-scopes` becomes `ladder` and `published-surface`, `reach` gains
`unit`, `seen-from` takes a reach; the compatibility pins are rebuilt.

**Measurement.** Nine repositories: zero findings added, 28 retired, every
other identity unchanged. guava 3,204 → 3,184: the 20 are top-level
package-private classes (`SneakyThrows` across its four packages and the
Android mirror, `SmoothRateLimiter`, `CollectionFuture`, `ClassPathUtil`,
`AbstractSortedMultiset`, `ImmutableEnumSet`, `UnmodifiableSortedMultiset`,
`AbstractFutureState`, `IThenable`, `Promise`) used only in their own file,
which no Java keyword can narrow — the old advice named a rung that does not
exist. Exposed 31 → 23: the 8 are `internal` members (`EnumTable.enumColumn`
and `initEnumColumn` in two test trees, `TestDb.dialects`, `dependencies`,
`ignoresSpringTests`, `ignoresPluginTests`) used from elsewhere in their own
file, where `private` would break the build and no file-wide keyword exists
for a member. Both classes hand-verified in the sources. Every surviving
message names the declared word and the keyword to type: Alamofire 264 (24
`private`, 240 `fileprivate`), vapor 85 (16 and 69), Exposed 23 (4 members and
19 top-level declarations, `private` both), guava 3,184 (`package-private` →
`private`, every one used only inside its class), ripgrep 1 (`pub(crate)` →
`private`), vite 51 (`export` → `unexported`). The contract fingerprint moves
(`Reach::Unit`, `Step.bearer`); the report schema (`published_surface` and
`ladder` on every row); every conformance fixture regenerated — rows for all
adapters, ten messages across the java, kotlin, rust and swift fixtures, zero
findings moved — and the js-ts fixture `export-narrowing` is renamed
`exported-but-used-only-here`, because the glossary avoids the retired word.
Two known gaps in the swift ladder fixture (`Widget.d`, `Widget.e`) re-point
at M8.d: a SwiftPM target publishes every export, so the Exported rung waits
for the unit's own publication.

## 2026-09-06 — M8.b.9: a unit's friends and its publication — the published surface is the engine's, and the instrument says the adapters' library-mode root can go

**The two facts.** `Unit` states `friend_of` — the units whose unit-reaching
names this one may use, named as the manifest spells them and resolved the way
a dependency is, own manifest first — and `publication`: declared where the
build system has a word for it, `Unstated` where it is silent, under which a
library is published and nothing else is (a published binary hands out no API;
`Unit::is_published` is the one reading). `ProjectUnit` carries both resolved;
the scope forest gains its unit layer, and a `Reach::Unit` declaration pools
over its unit's files plus its friends' wherever a manifest named the unit —
the adapter's own enumeration stands in until one does.

**The published surface is the engine's.** Five adapters spelled one
convention as a whole-file production root on every non-test file — "library
mode: any non-test class on the source path is importable surface" — a
statement about the UNIT made in the wrong place and made everywhere,
test-shaped names on the main path included. `GraphFile::published` now says
it once: the file holds an exported top-level declaration of a published
library unit, in a language whose units publish every export
(`PublishedSurface::Exports`; under `Entries` the entries already anchor). It
reaches the file as production, keeps each exported declaration through
`Keeper::Published { unit }` — `published` in `used-by` — and exempts the file
from `internal-only`'s Exported rung, which now also fires inside a unit that
publishes nothing: an executable's `pub` used only in its own file is advised,
a published library's never. Recomputed with the evidence, because it depends
on the manifest and the content both.

**The first producer.** The Maven reader emits two units per module: the main
set (its `<sourceDirectory>` when spelled, else the manifest's directory) and
`<artifactId>:test` (its `<testSourceDirectory>`, else `src/test/java` and
`src/test/kotlin`), which compiles against main and is its friend — Kotlin's
`internal` is visible to it, as package-private already is through the
namespace span. kmock's manifest says `friends=` and `publish=`; the kmock
ecosystem publishes through its entries, like npm, and `MockExtension::with`
speaks the jar-like variant. Conformance: `crates/kndo/tests/units.rs` pins a
friend's use against a stranger's, a published library's export against a
private one's and an executable's, and the Exported rung inside an
unpublished unit; `crates/kndo-toolkit/tests/jvm_structure.rs` pins the two
units and the spelled directories.

**Measurement, two ways.** The corpus: every report byte-identical — guava's
modules keep their tests in separate artifacts and declare their directories
in a parent pom this reader does not follow (M8.d), and the adapters'
library-mode roots still stand, so the new keeper only ever agrees with them.
The instrument: a copy of the tree with `kndo:java`'s library-mode root
deleted, run over guava on the engine's publication alone. 9,721 → 9,716: 9
added, 14 removed, nothing changed in place. The 14 removed belong to seven
GWT super-source `Platform`/`TestPlatform` files, each a package-private class
nobody imports and javac never compiles, that the convention had rooted as
production — each loses its `unused` symbol finding and its `untested` file
finding, and the seven `unused` FILE findings among the 9 added are those same
files, judged as the dead files they are. The other two added are
`BenchmarkHelpers.ListSizeDistribution.chooseSize`, in `guava-tests` and its
Android mirror: a public method of a public enum nested in a package-private
class, which nobody outside the package can name and nothing inside calls —
the convention kept it as "importable surface", and it is not. All 16
hand-verified in the sources. That is the licence M8.c needed: the java
adapter's library-mode root is a deletion with a number behind it, and the
other four (kotlin, swift, go, python) follow as their units land.

**Knobs.** `GRAPH_SEMANTICS_VERSION` 14 → 15: the graph carries `published`,
and its units carry `friend_of` and `published`. The contract fingerprint and
the report schema are unchanged — manifest evidence is not fingerprinted and no
report shape moved — and every conformance fixture is byte-identical: the two
java fixtures with a `src/test/java` tree gain a test unit whose files keep
the color and the pool they had.

## 2026-09-06 — M8.b.10: a unit's kind is its files' role, and Maven's test directory is read where Maven puts it

**The rule.** A test set's files are what the runner discovers and a tooling
or example set's are built to build something else, so every file of a `Test`
or `Bench` unit is a whole-file test root and every file of a `Tooling` or
`Example` unit a tooling root — `Certain`, because the manifest said which set
the directory is. A library's files are reached through its published surface
and an executable's through its entries, so neither anchors here. This is the
engine's statement of what five adapters spelled as a `src/test/…` path rule,
and the last of the three conventions the audit found in every adapter
(library mode, test directories, test names) to find its home in the unit
model; the name rule stays a convention, since it says nothing a manifest says.

**Reading Maven where Maven puts it.** guava keeps its tests in `test` and its
benchmarks in `benchmark`, and says so in the ROOT pom
(`<testSourceDirectory>test</testSourceDirectory>`, inherited by every module)
and in the build helper (`add-test-source` of `benchmark`). A reader that sees
one pom at a time can know neither, so `ResolveContext` now hands a manifest
reader every discovered manifest's content — manifests only, never source —
and the Maven reader inherits `<testSourceDirectory>` along `<parent>`
(`<relativePath>` when spelled, a directory meaning its `pom.xml`, an empty one
meaning no reactor lookup; `../pom.xml` otherwise) and adds the build helper's
test sources. The main set stays the manifest's whole directory minus the test
set's: what the build adds to main is not enumerable from the pom, and
over-inclusion there is the keep-alive direction — the `<sourceDirectory>`
narrowing the previous entry introduced is withdrawn for that reason.

**Measurement.** guava 9,721 → 9,593; every other repository byte-identical.
Retired: 130 `untested` — 128 benchmark files across `guava-tests` and its
Android mirror that the convention had colored production and the pom calls
test sources, plus two under `guava-gwt/test`. Added: 2 `internal-only`,
`NonPublicConstantIgnored.INSTANCE` in `ArbitraryInstancesTest` and its
mirror — a package-private constant a reflection test reads through the
class, now in a test unit whose package node no longer pools main's files: the
main-file `.INSTANCE` accesses that had silenced it were a name collision, and
the nine same-file `INSTANCE` namesakes that count as its own use are another;
the advice (a non-public constant may be `private`) holds, and the collision
is the name-only own-use test's residual, not the unit model's. The
instrument — java's `src/test/java` rule AND its library-mode root deleted, on
the engine's roles and publication alone — leaves every conformance fixture
byte-identical but one, `maven-gradle-dependency-skip`, whose Gradle half has
no unit yet; guava differs from this run by the seven GWT super-source files
the previous entry decomposed and by nothing else — `BenchmarkHelpers.chooseSize`,
the previous instrument's other case, is a test set's export now and its
runner's. Wherever a pom exists, the adapter's two directory conventions are
the engine's; M8.c deletes them, and Gradle's units (M8.d) close the last
fixture.

**Knobs.** `GRAPH_SEMANTICS_VERSION` 15 → 16: the same manifests anchor more.
The contract fingerprint, the report schema and every conformance fixture are
unchanged.

## 2026-09-06 — M8.b.11: the duplicate floor for files is 200 bytes outside comments, measured — and the function floor needs no byte twin

**Measure first, and the number pointed elsewhere.** The plan named a byte
floor for `duplicate`; the corpus said where it belongs. Function clones: the
smallest the 60-token floor admits is 155 bytes (`ImmutableList.of` with nine
elements, against its Android mirror), and every sampled small one is a real
clone — a byte floor on functions would retire nothing worth retiring, so
there is none. Byte-identical FILES are another matter: vite's 148 included
21 empty files, 57 under 40 bytes (`export default 'a'`, `import './a.js'`,
`invalid code`) and 111 under 100; flask reported two empty `__init__.py`
package markers as duplicates of each other; Exposed two 64-byte hello-world
snippets; and guava's mirror carried `package-info.java` files of 900 to 5,000
bytes whose content outside Javadoc is one annotated `package` clause, plus
empty holder classes under a 600-byte license header.

**The measure is bytes outside comments.** Raw bytes would keep every one of
guava's: a license header makes a stub look substantial, and a header is the
one thing every file of a project shares by design. So `FileEvidence` now
carries `len` — the sink's own file length, what every span is bounded by —
and `duplicate` measures a file as its length minus its comment spans (every
built-in declares the comments stream; a file whose adapter does not is
measured whole, the keep-judging direction). The floor is 200: below it sit
the markers, stubs and clause-only files above; at and above it, classes with
methods and modules with functions — and it is about the size of the smallest
function clone the token floor admits. Members of an exact group stay
shadowed from the structural pass whatever their size, since a copied file
must not also duplicate every function inside itself; only the file finding
is floored.

**Measurement.** Nine repositories: 181 findings retired, zero added, zero
changed in place. vite 828 → 689 (139: 0 to 362 raw bytes, median 33 —
fixture stubs and create-vite template configs); guava 9,593 → 9,555 (38: 22
`package-info.java` and 16 empty holders, forwarding shells, one-method
interfaces and an enum of test constants, every one a mirror of boilerplate;
raw sizes 95 to 5,122, content under 200); Exposed 973 → 971 (the two
hello-world snippets); flask 28 → 26 (the two empty markers). Every retired
file checked by name and content. Alternatives measured: at 150 bytes guava
keeps 24 of the 38 and vite 4 of the 139; at 256, guava loses 8 more and vite
4 more; nothing at either boundary changes the character of what is retired,
and 200 is where the samples turn from clause-only files into classes with
bodies.

**Knobs.** The contract fingerprint moves for `FileEvidence::len`; no report
shape, no conformance fixture and no adapter version moves — the wire's file
evidence never carried a length, since the host replays it through a sink
that knows the content's. Both floors stay constants beside the analysis
until the config registry names them.

## 2026-09-06 — M8.b.12: what a language's own tool never compiles is discovered and never claimed — and the corpus says `testdata` is not one

**The capability.** An extension declares `ignores`: globs of the paths its
language's own tool never compiles, segment-literal (`*` stays inside one
path segment, so `**/_*.go` is a file whose name begins with `_`, never a
file under a directory that does). Three named consumers in core, one rule:
the claim pass leaves a file under one unclaimed — no evidence, no judgment —
the manifest pass leaves a manifest under one unread — a dependency's
`package.json` inside `node_modules` declares nothing about the project — and
the dependency eligibility rule lets an unclaimed file under one cast no
doubt on its manifest's judgments, since the adapter itself left it unread.
Discovery is untouched: the file stays in the tree, so an import pointing at
it is scope, not breakage. The default is none; the WIT spec record carries
the field and the compat guests are re-pinned.

**The rule is the tool's, never a guess.** Go declares `**/vendor/**` (copies
of other modules, compiled as the dependencies they are) and `**/_*.go` (a
file the tool does not list even among its ignored files); JavaScript, HTML
and CSS declare `**/node_modules/**`; Python declares `**/site-packages/**`
— the interpreter's own directory, which no package can be named, and which
every environment holds whatever the environment is called (a `venv` glob
would be a name convention, and `.venv` is hidden from discovery anyway).
Not declared, deliberately: a JVM or Cargo output directory, because a
package may be named `target` or `build`; and a manifest's excludes, which
are that unit's membership, not the tree's.

**Measured twice, and the first measurement corrected the design.** The first
declaration for Go also named `**/testdata/**` and `**/_*/**`, as the
`go help packages` sentence reads; the corpus answered with one finding
ADDED on gin: an `undeclared` dependency of the module on its own
`github.com/gin-gonic/gin/testdata/protoexample`, imported by three test
files. Against go 1.24.7 in a scratch module: an explicit import of
`testdata/gen` builds, an explicit import of `_scratch/old` builds, `./...`
lists neither, and a `_draft.go` beside a package's files is absent from
`GoFiles` and from `IgnoredGoFiles` alike. So a `testdata` or `_`-prefixed
directory is skipped by patterns and compiled by imports — claimed, as
before — and only `_`-prefixed files and `vendor` are never compiled. The
second measurement then showed two vite findings retired for the wrong
reason: two `test-only` dependency verdicts on
`packages/vite/src/node/__tests__/package.json` vanished because the
`.scss` files of a committed `node_modules` fixture, unclaimed now, cast
`unclaimed-importers` doubt on the manifest that owns them. Hence the third
consumer: what the adapter never compiles imports nothing on its behalf.

**Numbers.** Nine repositories, one moves: vite 689 → 687, seven files
unclaimed (five JavaScript, two SCSS, every one under a committed
`node_modules`), two `unused` file findings retired —
`playground/glob-import/root/dir/node_modules/hoge.js` and
`packages/vite/src/node/__tests__/plugins/fixtures/sass-package-resolution/node_modules/sass-pkg-with-index/index.scss`
— zero added, zero changed in place. gin is byte-identical under the
corrected declaration (99 claimed, 108 findings); its one file under
`testdata` is a generated protobuf that is imported, claimed and exempt.
flask, guava, Exposed, ripgrep, vapor, Alamofire and lodash are
byte-identical: nothing under a declared ignore is checked into them.

**Fixtures and knobs.** Two conformance cases, both new: `tool-ignored-paths`
in the kndo-adapter-go fixtures (10 discovered, 5 claimed: `_draft.go` and
the vendored module unclaimed, the imported `testdata` and `_scratch`
packages claimed and resolved, and the module accused of no dependency on
itself) and `committed-node-modules` in the kndo-adapter-ts fixtures (5
discovered, 1 claimed: the package's files unclaimed, its manifest unread,
and its `.scss` casting no doubt, so the project's own unused dependency is
still judged). The engine test speaks kmock with `**/vendor/**`: the
vendored file is discovered and unclaimed, its manifest declares no unit,
the import into it is not `unresolved`, and the same tree without the
declaration is judged whole. No existing fixture moves. The same evidence now
assembles into a different graph — unclaimed files, unread manifests, the
adapter's ignores on each manifest's declarations — so
`GRAPH_SEMANTICS_VERSION` moves 16 → 17; no adapter version moves (the
evidence a claimed file yields is unchanged), and the contract fingerprint
does not move.

**Open, with the measurement that would decide each.** `describe` on an
ignored path answers as for any unclaimed file; naming the ignore and its
declarer is a query-surface change for when a frontend asks. Conduct
activation reads every discovered path, ignores included — a framework
template's `Info.plist` under `node_modules` could activate `kndo:info-plist`
— and the corpus holds no such tree. A `testdata` package reached from tests
alone would read `test-only` today; whether it is test material by the
tool's layout is a file-role question, and gin's only `testdata` package is
generated, so nothing measures it yet.

## 2026-09-06 — M8.b.13: an embedded region is its language's to read — html's second `TypeScriptAdapter` and hand-written scanner retire

**The capability.** A host adapter reports a span of its file written in
another language as an embedded region: the span, the language as the file
suffix its extension claims (`js`, `css`), and how the span runs — a module,
or a classic script whose top-level declarations are the page's globals.
The engine, after the host's own extraction, hands each region to the
extension claiming that suffix through the same sink: spans arrive relative
to the region's bytes and land in the host's coordinates, the region's
imports are marked as the region's so that extension resolves them, a write
to a stream the host never declared is dropped without a word (the host's
declaration bounds its file's evidence), a region reported inside a region
is refused, and a region of a language nothing claims is left unread with a
diagnostic on the file. `SourceFile::region` tells the reading extension it
is one, and which. What a region declares and imports is then judged,
resolved and addressed as the host file's own — `index.html#unusedInline` is
a finding like any. The evidence cache remembers which extensions read a
file's regions, by coordinate and version, and misses when one changed: the
part of the key only extraction could learn is checked on read instead of
hashed.

**What retired.** The html adapter carried a private `TypeScriptAdapter` to
resolve what inline scripts import and a hand-written scan of import
statements — a second JavaScript reader beside the grammar, string- and
comment-skipping by hand, every import a whole-surface `Glob`. Both are
gone: the adapter reports each inline `<script>` (module, or classic by its
`type`; an import map, JSON or a template is data, not a region) and each
inline `<style>` as a region, and resolves its own attribute URLs exactly as
a browser requests them — the path as written, or root-relative at the
nearest ancestor that holds it — with no guessed extension. The JavaScript
adapter reads a region with the grammar its language names, leaves a
region's roots to its host, and marks a classic script's top-level
declarations exported, since every other script and handler attribute on the
page can reach them.

**Measurement, decomposed.** vite 687 → 711 (+26, −2). Retired: two `unused`
files the old scanner never reached — `playground/assets/css/import.css`,
imported by an inline style's `@import url(...)`, and
`playground/assets/static/import-expression.js`, imported by an inline
module's dynamic `import()`. Added, twenty `untested` on playground pages
whose inline modules declare functions: a page with code is production code
no test file reaches, the verdict its sibling modules already carried, and
the page carried none only because it declared nothing. Added, six `unused`:
three constants an inline module declares and never reads
(`define/index.html#__VAR_NAME__`, `glob-import/root/index.html#notInvocation`,
`optimize-deps/index.html#globbed` — each a fixture of vite's own behavior,
each unread by the language); two named exports of
`glob-import/root/transform-visibility.js` (`globResult`, `dynamicResult`)
that the page never imports — it takes the default — and that the old
whole-surface `Glob` kept alive by not looking; and one that is a gap:
`wasm/imports.js#imported_func`, consumed by `light-with-imports.wasm`'s
import section, which vite's wasm plugin resolves from the binary and no
adapter reads. Ten fewer unresolved edges (165 → 155): inline imports now
resolve under JavaScript's spellings. One more diagnostic: the css
playground's inline `@import url(./imported.scss)` parses partially, as the
same text in a `.css` file would. flask 26 → 29: the three example templates
with inline functions (`fetch.html`, `jquery.html`, `xhr.html`) are
`untested` for the reason above. lodash 19 → 18:
`vendor/firebug-lite/skin/xp/firebug.css` is reached by `firebug.html`'s
inline `<style>@import "firebug.css"</style>`. Exposed's findings are
byte-identical at 971 while its declarations move 11,673 → 20,249 and its
references 238,593 → 345,849: 4,315 generated documentation pages hold
12,891 inline classic scripts (2.7 MB of `var pathToRoot = …`), now read —
their declarations are the pages' globals, the pages root themselves, and
none declares a function, so nothing fires. gin, guava, ripgrep, vapor and
Alamofire are byte-identical.

**Fixtures and knobs.** `document-entries` in the kndo-adapter-html fixtures
grows an inline module (a named extensionless import resolved as
JavaScript's, a side-effect import only the region makes, a dead function),
a classic script (its function alive as the page's global), an inline style
(`@import` reaching a sheet) and an import map (no region); the engine test
speaks kdoc, a document language embedding kmock, and pins the offsets, the
routing, the judgment, the unknown-language diagnostic and the host's stream
bound; the contract test pins the sink's shifting, clamping, marking,
dropping and refusal. The contract fingerprint moves (`FileEvidence`,
`Import`, `SourceFile`); `kndo:html` moves 1 → 2 (different evidence from
the same page); the JavaScript and CSS adapters keep their versions, since
no file they saw before yields different evidence; the graph semantics do
not move, since the assembly of the same evidence is unchanged; the WIT
`extract` takes the region and `file-evidence` carries the list, and the
compat guests are re-pinned.

**Open, with what would decide each.** A `.wasm` module's import section is
evidence (the `imported_func` gap above); an adapter for the binary format
would close it, measured on vite's wasm playground. Handler attributes
(`onclick="f()"`) are JavaScript expressions in classic mode and are not
regions yet — the corpus reaches every such function through its script's
globals rule, so nothing measures the need. Suppression pragmas inside a
region are dropped with the host's undeclared comments stream; a page that
wants `kndo:allow` in its inline script is the day to decide whether a host
declares its regions' streams.

## 2026-09-06 — M8.b.14: reach is structured, and a member reaches no farther than its owner — `Private` retires

**The vocabulary.** `Reach` now spells every address the scope forest has:
`Owner` (a private member), `File` (a top-level `private`, an ES
declaration without `export`, a Rust item without `pub` until its module
tree is declared), `Namespace { up }` (Java's package-private, Rust's
`pub(super)`), `Unit { up }` (`internal` and `pub(crate)` at `up: 0`;
Swift's `package`, the group of units one manifest aggregates, at `up: 1`),
`Directory { up }` (an exported Go name under `internal/`, fenced at that
directory's parent), `Named { namespace }` (Rust's `pub(in crate::a)`,
resolved by the engine against the forest inside the file's own
compilation), `Inherited` (a Rust trait item: exactly as reachable as its
owner), `Scoped` (the go adapter's `package` token, until its namespace
clause lands) and `Exported`. `Private` retires: it named two rungs under
one word, and the adapter always knew which — Kotlin, Swift, Java, the
TypeScript members and Python's underscore now say `Owner` or `File`. Each
reach stands on a rung (`Reach::rung`), and `Rung` grows `Directory` and
`Group` so a ladder can name them: Go spells `exported` on both the
directory rung and the exported one, so the advice below either is
`unexported`.

**Effective reach.** A member's reach is capped by its owner's,
transitively (`Reach::capped_by`, `FileEvidence::effective_reach`): never
wider by rung, `Inherited` the owner's exactly, a token compared as a
namespace. The engine pools and judges by the effective reach — the
navigator's keepers, `internal-only`'s pool, `private-type-leak`'s chain —
and the ladder's word by the declared one; `describe` reports both. A
public member of a file-private class is never handed out by a published
surface or an entry's; a package-private field of a private nested class has
no pool beyond its owner.

**Pools.** The forest gains three layers with their consumers: a directory
layer (every directory of the tree with the files under it, the root
included, read by `Directory { up }`), a group layer (each unit beside every
unit its aggregator lists, read by `Unit { up: 1 }`; `ProjectUnit::group`
names the aggregator) and a lookup by name inside a compilation (read by
`Named`). A climb that leaves the tree, a unit no manifest aggregates and a
name no file of the compilation declares are unbounded — keep-alive, as
every unbounded pool is.

**Measured.** Nine repositories: guava 9,555 → 8,242 (1,315 `internal-only`
retired, 2 `unused` added), Alamofire 591 → 538 (53 retired), vapor 218 → 199
(19 retired), gin 108 → 109 (1 added); Exposed, flask, lodash, ripgrep and
vite byte-identical. Every retirement is one shape: a member whose declared
word is wider than its owner's fence — 1,295 of guava's are members of
private nested classes (`AbstractIteratorTester.PermittedMetaException.UOE`,
package-private inside a private class), 16 of local or package-private
nested classes and 4 of private top-level nested classes;
Alamofire's and vapor's are `internal` members of `private struct`s and
`fileprivate` classes (`AuthenticationInterceptor.AdaptOperation`,
`URLEncodedFormTests.User`). v1 advised `private` on each; v2 does not: a
narrower word there changes nothing anyone outside the owner can name, and
the advisory is about surface, not spelling. The two additions are the same
rule from the other side — `BenchmarkHelpers.chooseSize`, a public method of
a nested enum of a package-private test class, referenced by nothing, which
the whole-file test root's entry surface kept while its owner's fence was
not read. gin's addition is `RandStringBytesMaskImprSrcSB`, an exported
helper of an `internal` package's test file used in that file alone —
`unexported` would suffice, and now Go's ladder can say so. ripgrep's trait
items read `Inherited` and judge as before, since the old code copied the
trait's reach onto them; ripgrep's 14 `pub(super)` items are `Namespace
{ up: 1 }`, unbounded until Rust declares its nesting, and its 0 `pub(in
…)` leave `Named` to the fixture and the engine test. vapor's 27 `package`
declarations are `Unit { up: 1 }`, unbounded until SwiftPM's targets are
units; no finding depended on them before.

**A regression the measurement caught.** The first run added ten `unused`
on gin's `TestXxx` and `BenchmarkXxx` functions in `internal` packages: an
exported name there is directory-reaching, and the whole-file test root's
entry surface keeps exported names alone. The runner's rule is the go
adapter's to state: a top-level `Test`, `Benchmark`, `Example` or `Fuzz`
function of a `_test.go` file is now a root on the function itself,
whatever its reach — which is what `go test` does.

**Fixtures and knobs.** No judgment moves in any fixture; every one of the
kndo-adapter-go fixtures re-pins because its run header lists Go's ladder,
which grew the directory step. The contract fingerprint moves (`Reach`, `Rung`); the report
and query schemas move (`Rung`'s two values, `describe`'s
`effective_reach`); the WIT `reach` variant carries every address and the
compat guests are re-pinned; every adapter that emits a changed value bumps
(`kndo:rust` 9, `kndo:kotlin` 5, `kndo:swift` 3, `kndo:java` 10,
`kndo:js-ts` 9, `kndo:python` 2, `kndo:go` 5); the graph semantics do not
move. The engine test speaks every rung through kmock (`owner`, `file`,
`ns`, `unit`, `group`, `dir(N)`, `named(a.b)`, `inherited`), and the contract
test pins the rungs, the cap and the chain.

**Cut, and named as such.** The design listed `Named` and `Unit { up }`
beside the others; both land, with a real emitter each (Rust's
`pub(in path)`, Swift's `package`). `Reach::Inherited` is the owner's reach,
not a subclass's: Java's and Kotlin's `protected` still fold to Exported,
since subclasses are a relation the forest does not walk yet. TypeScript
members read `Owner` whatever their modifier, as they read `Private` before
— the modifier is the js-ts migration's to spell.

## 2026-09-06 — M8.b.15: `protected` is the owner's, its subtypes' and, in Java, its package's — `Reach::Heirs` walks the relations, and a heirs member of a published type is published surface

**The gap, measured before the design.** `Reach::Inherited` is the owner's
reach, not a subclass's, so `protected` folded to `Exported` and was judged
as `public`: never advised, kept wherever its owner was. guava declares
3,615 `protected` members (512 in the published main sets, 1,306 in
guava-tests and guava-testlib), Exposed 106, and v1's oracle names
`protected` in 28 of its 20,244 guava findings (26 `internal-only`, 2
`unused`). A bounded precision gap, and one slice's worth.

**The vocabulary.** `Reach::Heirs { and_namespace }`: nameable inside the
owner and every transitive subtype of it, plus the owner's namespace when
the language grants it. Java emits `and_namespace: true` (`protected`
grants the package too), Kotlin `false`; TypeScript members stay `Owner`
whatever their modifier, the js-ts migration's to spell. `Rung::Heirs`
sits between `Namespace` and `Unit`, and is another axis of the ladder
rather than a rung between two: a member used from its package and no
subtype is never narrowed to `protected`, and `Ladder::step_down` offers
the heirs step only when the extent is the heirs' own. Java's ladder reads
`private` (members), `package-private`, `protected` (members), `public`;
Kotlin's `private` (members and free), `protected` (members), `internal`,
`public`. `Reach::capped_by` treats it as any rung; `Reach::Inherited`
remains the owner's exactly.

**The pool walks the relations.** The navigator builds one pool per fence
— the file, the owner that carries the heirs reach, its `and_namespace` —
from the owner's file, the files declaring any transitive subtype (the
`Extend` relation walk, by simple name over every file, so a same-named
type elsewhere widens the pool: the keep-alive direction) and, when
granted, the namespace pool. `Index::pool_for` resolves a declaration by
its effective reach and, for a heirs reach, against the fence whichever
member inherits it; `describe` renders `subtypes` and
`subtypes+namespace`.

**Published units.** In a unit that publishes its exports, a heirs member
of an owner whose effective reach is `Exported` is published surface: a
subtype outside the tree may name it, which no pool can hold. The keepers
keep it (`Keeper::Published`, riding its owner's surface though its own
pool is bounded), and `internal-only` says nothing, as it says nothing of
`public`. In an unpublished unit — a test set, an application — the pool
judges: used in the owner alone, `private` would suffice; used from the
package and no subtype (Java), `package-private` would; used from a
subtype, `protected` is the reach its use needs and there is no finding;
used by nobody, `unused`.

**Four rules the measurement taught.** The first run added 8 "`protected`
would suffice" on Exposed's `internal` members used within their file: the
ladder had read heirs as a rung between file and unit, and it is another
axis (rule one, above). It accused the fixture's `Fixture.shared()`, used
from a subtype in the same package, of `package-private`: a use from a
subtype pins `protected`, whatever package the subtype sits in (rule two:
the heirs' extent when every user file declares a subtype, the package's
only when none does). It advised `package-private` on guava's
`AbstractIterator.endOfData`, whose owner is package-private: the owner's
cap already holds every use, so only an extent below the effective reach's
rung is advice (rule three). And it advised `package-private` on guava's
`*Test.create()` methods from same-named accesses on other classes
(`ArrayListMultimap.create()` in the same package): positive advice rests
on uses that are this declaration's for sure — an access whose receiver is
the owner or a subtype by name — and a use with any other receiver keeps
the member alive and says nothing about where it is used (rule four).

**Measured.** guava 8,242 → 8,254: twelve `internal-only`, six in each of
the android and main trees, every one a `protected` member used within its
owner alone — `AbstractIteratorTester.MultiExceptionListIterator` (a nested
class of a package-private testlib class), `AbstractTableTest.cellValue`
and `nullableCellValue`,
`AbstractClosingFutureTest.assertFinalStepThrowsIllegalStateException`
(guava-tests), `AbstractBaseGraph.nodePairInvalidatableSet` and
`LineBuffer.handleLine` (the published main set, both of package-private
owners, whose effective reach is the package and whose owner alone is
below it). Alamofire, Exposed, flask, gin, lodash, ripgrep, vapor and
vite are byte-identical in their findings; Exposed's and guava's run
headers gain the `protected` step. v1's 28 decompose without a match: its
8 on guava-gwt's `ForwardingSortedMultiset` and failureaccess's
`InternalFutureFailureAccess` are published surface (v1 judged visibility
without a unit's publication); its 8 on `SourceSinkTester`'s fields and
`getLines` are used from the four subtypes (v1 never walked subtypes, and
called them "weaker matches"); its 8 on `OldAbstractFuture` are members
the same-file facade overrides (`set`, `setException`: a promised surface)
or that same-named accesses on other receivers in the package keep
(`interruptTask`, `wasInterrupted`: rule four); its 2 on
`WhitespaceMatcherBenchmark.setUp` are a caliper `@BeforeExperiment`
entry, kept by name dispatch until a rule pack says so; its 2 `unused` on
`SomeClassThatDoesNotUseNullable.protectedButDoesNotCheckNull` ride the
owner's import binding in `NullPointerTesterTest`, which subclasses it and
exercises it by reflection.

**Fixtures and knobs.** `visibility-ladder-and-nested-members` gains a test
set (`Fixture`, `FixtureSub`, `Neighbor`) that pins all four verdicts:
`Fixture.seed()` used from the owner alone (`private` would suffice),
`Fixture.packaged()` used from `Neighbor` in the package and no subtype
(`package-private` would), `Fixture.shared()` used from the subtype
`FixtureSub` (alive, no advice), `Fixture.nobody()` (`unused`);
`Widget.c()` and `d()` stay known gaps. Every one of the kndo-adapter-java fixtures
and kndo-adapter-kotlin fixtures re-pins because its run header lists the
ladder, which gained the `protected` step; no other judgment moves. The
contract fingerprint moves (`Reach::Heirs`, `Rung::Heirs`); the report
and query schemas move (the `heirs` rung); the WIT `reach` variant gains
`heirs(bool)` and the compat guests are re-pinned; `kndo:java` bumps to 11
and `kndo:kotlin` to 6; the graph semantics do not move. The engine tests
speak `heirs` and `heirs+ns` through kmock and pin the pool (owner,
subtype, package; a user from a package the member does not reach is a
stranger that keeps it and draws no advice) and the surface (a heirs
member of a published type is kept by `published`; of a file-private type,
`unused`); the contract test pins the axis.

**Open, and named as such.** `Reach::Inherited` is still the owner's;
TypeScript members read `Owner` whatever their modifier; a `protected`
member of a public type in a published unit is silent as `public` is —
narrowing it is a library author's call the pool cannot make. The
subtypes are walked by simple name: two same-named types in different
packages share heirs, which widens a pool and never narrows one.

## 2026-09-06 — M8.b closes: the milestone against its plan, the two items the corpus moved to M8.c, and the one knob two slices owed

**What the plan asked, and what stands.** Fourteen numbered slices and the
identity toll delivered M8.b: manifests read once into `ManifestEvidence` and
assembled into `Project` (units, roots, excludes, entries, friends,
publication, aggregating group); the published surface as a unit's fact rather
than nine adapters' whole-file root; supertypes as promises and members on them
as witnesses; namespaces under their source root, with a rung carrying its
language's word; units and classpath-spanning packages; a member reached
through an access, never by spelling its name; `internal-only` reading the
ladder alone, with `narrowable_scopes` and `export_narrowing` retired; file
roles from the unit's kind, which is how `test-only` and `untested` now take
their colors; the 200-byte duplicate floor; discovery ignores (the hidden
opt-in the walk already had needed nothing); embedded regions extracted by
their own language; structured `Reach` with the owner's cap; and `Heirs` for
`protected`. Of the plan's list, one line is unbuilt on purpose — path aliases
— and one keeper is unbuilt on purpose — glob pooled by name and seeing
through a namespace import. Both were measured before being moved, not
skipped: `EXPERIMENTS.md` carries each with its number.

**The two, with their numbers.** Declarations in reachable files kept by a
whole-surface importer ALONE across the nine corpus repositories: 63 of
101,240, and 39 of those stand behind a side-effect import, which is an opaque
importer by design. So the entire reachable prize for seeing through namespace
and glob imports is 20 findings (vapor 16, Exposed 4), and re-export chains 4
more. Path aliases: vite is the only repository that declares any — 57 import
sites of `#types/*` and `#dep-types/*`, two `tsconfig` `paths` entries, three
import maps — and at most 4 findings sit on the files they address. Neither
number justifies a contract in the engine milestone; both are demanded by the
adapter that owns the syntax, so they land in M8.c beside it (js-ts for
aliases, the first qualifier-emitting adapter for the keeper) and are measured
there again. Recorded so nobody rebuilds them from the design document alone.

**The knob two slices owed.** `GRAPH_SEMANTICS_VERSION` did not move in M8.b,
and twice it should have: `3c8aa92` put `Project` and each file's unit into the
graph and anchored entry roots, and `b259942` added the published flag and
rewrote the keepers around it — new assembled fields in both, which is the
knob's own stated trigger. Neither commit moved the contract fingerprint (the
manifest types carry no fingerprint derive) nor any extension spec, so nothing
else in the graph cache key moved either: a warm on-disk graph from before
them answers for the same tree afterward. The payload is bincode, positional
and not self-describing, so a shape change usually fails to deserialize and
degrades to a miss — but "usually" is not a contract, and the accident is not
the statement. One bump closes both: the graph semantics go 17 → 18 here,
where the milestone can name what earned it. No fixture and no finding moves;
the change is the cache key.

**What M8.c takes first.** Rust, in four slices: the engine learning that a
mount nests a namespace (so `Namespace { up }` and `Named` pools stop being
unbounded and `seen_from` can die for the language that has the deepest module
tree); the adapter emitting mounts and its Cargo targets as units; paths inside
macro token trees, the audit's largest Certain false-positive class; and
`#[path]` in non-mod-rs files with `include!`. Each with its corpus number
against ripgrep and this repository's own dogfood.

## 2026-09-06 — M8.c.1: a mount nests a namespace — the engine reads a module tree, and a private mount fences everything under it

**The gap.** Half the vocabulary M8.b built had no way to be answered. A
namespace pool climbed nowhere (`Reach::Namespace { up }` returned `None` for
any `up > 0`, keep-alive), a namespace by name resolved only where a file
wrote a clause, and `Reach::Unit { up: 0 }` fell back to the adapter's own
`seen_from` region. The reason is that a Rust module has no clause to read:
its name is written by the file that MOUNTS it (`mod x;`), and its address is
the chain of mounts above it. Until the engine reads that chain, ripgrep's 14
`pub(super)` items are unbounded, `pub(in crate::a)` names nothing, a `pub`
item of a private module is published surface forever, and a descendant
reading its parent's private name — legal Rust, and the audit's measured
false-positive class — has no pool to be read from.

**The shape.** `ImportShape::Mount { namespace, reach }`: the target becomes a
child namespace of the importing file's, named by the segment and attached
with the reach the mount carries. Two facts no other shape holds — the address
and the fence — and one it deliberately drops: a mount hands out no surface.
The parent reaches its child's items by qualifying them, which is a reference
of its own, so `pub mod` stops keeping a whole surface alive the way a
re-export does.

**The forest, and the pools that read it.** The engine builds one mount edge
per file (the first mount in file order, so two `#[cfg]` twins mounting one
file still give it one address), then each file's chain: the tree it is rooted
at, and the segments down to it. A namespace node is now either a clause
inside a compilation, as before, or a chain inside a TREE. A namespace pool is
that node's SUBTREE — its files plus every file mounted under it — which is
exactly what "private to this module" means in a language whose modules nest;
a climbing address (`up`) walks the chain and pools the node it reaches; a
named namespace resolves inside the file's own tree; and a unit reach with no
manifest unit pools the tree, which is the crate region the adapter used to
enumerate by hand.

**The fence.** A mount's reach is written in the mounting file, so the engine
reads it from where the mounted file stands: `Reach::shifted` moves a
namespace-relative address out by the hops between them, and the narrowest
mount on the chain becomes the file's cap. `Reach::capped_by` grows a level
compare to make that work — on one rung the address that climbs fewer levels
is the narrower, which was already true of an owner above a member and is now
asked twice. `Index::effective` is the one seam every judgment reads: the
declared reach, capped by every owner above it and by the mounts above its
file. The keepers, `internal-only`, the pools and `describe` all read that
one function, and publication reads it too — a fenced file's exports are on no
unit's surface.

**Measured: nothing moves, by construction.** No adapter emits a mount yet, so
every corpus repository is byte-identical and every conformance fixture
re-pins unchanged. That is the point of landing the engine first: the numbers
belong to the adapter slice, where rust's `mod` lines become mounts and the
audit's classes are answered. The kmock language grows a `mount` line and an
`ns(N)` reach so the four engine tests state the rules on evidence rather than
on Rust: a private mount fences an export off the published surface and leaves
it accusable, a namespace reaches down every mount it holds, a climbing reach
pools the node it climbs to, and a unit reach pools the tree where no manifest
named a unit.

**Knobs.** The contract fingerprint moves (`ImportShape` grows a variant); the
graph semantics go 18 → 19 (each file carries its mount edge and its cap, two
new assembled fields); the WIT gains `mount-point` and the compat guests are
re-pinned; the report and query schemas do not move, since a shape is not in
them. No adapter version moves and no fixture changes: nothing emits a mount
until the next slice.

## 2026-09-06 — M8.c.2: a Rust module is its mount chain, and a crate is what Cargo compiles — four convention hooks retire

**What lands.** `kndo:rust` speaks the vocabulary the engine grew: `mod x;`
is a mount carrying the `mod`'s own visibility; an item with no `pub` reaches
its module's namespace rather than its file; an inline `mod x { }` OWNS what
it declares, so the module caps its items the way a private type caps its
methods; and one `extract_manifest` states what four convention hooks used to
guess. Every cargo target is a unit entered through its own file — the lib,
each bin (declared or discovered under `src/bin`), each test, bench and
example, and the build script — with `publish = false` read as cargo's own
word for "no consumer outside this project". `roots`, `packages`,
`manifest_dependencies` and `seen_from` are gone from this adapter, and with
them the hand-enumerated crate region and the path table that decided which
files were "unimportable" targets.

**What the engine had to learn with it.** A file a tree holds is compiled by
the target the tree is rooted at: Cargo's lib and its bins share `src/` and
differ only in which module tree reaches them, so the directory cannot say who
compiles what and the entry does. And `internal-only` had to be told what the
navigator already knew — a mount hands out no surface, so it never marks its
target as namespace-imported. Without that one arm the whole analysis went
silent for every mounted file, which the `macro-template-names` fixture caught
before the corpus did.

**Measured on ripgrep** (the only corpus repository rust claims): 154 → 151.
Sixty-four `duplicate` findings are the same findings under a more precise
name — a test fn inside `mod tests { }` is now `tests.only_matching`, not
`only_matching`, because the module owns it. Three `test-only` findings retire
as false positives: `tests/index/basic.rs`, `disallowed.rs` and `mod.rs` are
modules of an integration-test crate, so "only tests reach this file" was
never a defect — the unit's kind says they are tests by role, which is the
same rule that retired this repository's own `kndo:allow-file test-only` in
`crates/kndo/tests/common/mod.rs` the day it landed. One `internal-only`
survives with a wider pool. No `unused` moves on ripgrep, and every other
corpus repository is byte-identical.

**The rule that could have gone wrong, and did not.** With `pub mod` no longer
a re-export, a crate's exported surface is kept by its unit's publication
alone — and this repository's own crates all declare `publish = false`. The
dogfood gate is the measurement: kndo on kndo reports nothing new. Every `pub`
item in this workspace is named by something in it, which is what an internal
library should be able to say about itself.

**Fixtures.** `module-tree-visibility` pins the mount rules end to end: a
`pub` fn under a private `mod` is accusable and IS accused; a private name is
read by a module mounted under it (legal Rust the old file-scoped reach called
dead); `pub(super)` names the parent; and the published crate's `pub mod`
surface stays kept. `bin-crate-surface` pins the other side — an executable
hands nothing to anyone, so its unreferenced `pub fn` is dead.
`test-only-and-cycle` moves one verdict and says why in its own file: its
crate declares `publish = false`, so `pong::rally`, which nothing names, is
dead where `pub mod pong;` used to keep it. `attribute-dispatch` and
`inline-mod-qualified` re-spell two subjects with the module that owns them.
Only the kndo-adapter-rust fixtures move, and only test-only-and-cycle changes
a judgment.

**Knobs.** `kndo:rust` bumps to 10 (a mount is different evidence from a
re-export, and a bare item now names its namespace). The graph semantics go
19 → 20: the same evidence assembles differently now that a unit walks down
its module tree. The contract fingerprint and the schemas do not move — the
shapes landed with the engine slice before this one.

## 2026-09-06 — M8.c.3: a path inside a macro's tokens is a use — measured at zero findings, kept for what it says, and marked as inferred

**What the audit measured, and what it measures now.** The nine-adapter audit
called qualified paths inside macro invocations rust's largest `Certain`
false-positive class, verified on two probes: `println!("{}", util::helper())`
left `helper` `unused certain`, kept by nothing. Reproduced today, before any
change here: nothing is accused. The mount model retired the class — a `pub`
item of a privately mounted module now reaches its parent's namespace, and the
bare names a token tree already yields are inside that pool. The class was
never about macros; it was about `pub` meaning "exported" with no pool to be
used from.

**What the reconstruction still earns.** A macro's arguments are handed over as
raw tokens, so `b::pull()` inside `println!` draws no EDGE: `used-by`, `trace`
and reachability see a bare name and nothing else, and a cross-crate use spelled
only inside a macro is invisible as an edge. Reading `::`-joined runs the way
attributes already do adds 3 import edges on ripgrep (309 → 312) and moves no
finding, on any corpus repository, in any category. It ships for what it says
rather than for what it counts: an adapter reports what the file spells, and the
file spells a path.

**Two defects the shared scan carried.** Folding the macro side into the
attribute scan exposed both, and both were live in shipped attribute reading.
An identifier that broke a run was DROPPED instead of starting the next one, so
`Box::<dyn std::error::Error + Send>` read as `error::Error` — a crate nobody
declares. And a run bridged a gap, so a template's `#name ::krate::Trait` read
as `name::krate::Trait`. The first run of the corpus with the old scan added
four false `undeclared` on ripgrep (`flags`, `time`, `thread`, `error`) and one
on this repository's own `quote!` templates (`impl_generics`). A path's tokens
touch: adjacency by byte position tells `a::b` from `a ::b`, and an identifier
that cannot continue a run heads the next one.

**Inferred, not parsed.** A run of tokens looks like a path and usually is one,
but a macro template's `$crate::x` resolves at every expansion site rather than
where it is written, and a proc-macro's `quote!` names crates its own manifest
has no reason to declare. So macro runs carry `Confidence::Possible`: the edge
keeps things alive and answers `used-by`, and dependency hygiene — which reads
`Certain` imports alone — never accuses a manifest on evidence the grammar did
not parse. Attribute runs stay `Certain`, because a derive path really does
name the crate that must be declared.

**Where the scan lives.** Not the toolkit: it is knowledge of one grammar's
`token_tree`, so it stays beside that grammar as one adapter-private function
both callers share. The second copy promoted, one floor down from where the
milestone plan guessed it would.

**Deferred with its number: qualified references for rust.** `Reference::on`
and the `Qualifiers` stream sharpen one thing — `internal-only` on members,
where a bare name elsewhere must stop counting as a use. ripgrep carries one
`internal-only` finding in total and none on a member; this repository's own
tree carries none. The population is zero, and a mis-emitted receiver would
turn real member uses invisible and invent advice. The keeper that will demand
qualifiers is the see-through one already carried in `EXPERIMENTS.md`, and its
20 findings sit in vapor (16) and Exposed (4) — Swift and Kotlin, not Rust. So
rust waits for a consumer with a number, and this entry is the number it waited
on. `kndo:rust` bumps to 11; no fixture, no fingerprint and no graph semantics
move.

## 2026-09-06 — M8.c.4: a `#[path]` is anchored where the Reference anchors it, an alias is an alias everywhere, and an `include` is sight

**Three Certain false positives the audit verified, none of them on the
corpus.** `include!` appears in no rust file of the corpus; the one `#[path]`
ripgrep writes sits in a `mod.rs`, which already worked. So the corpus cannot
measure this slice, and the probes and fixtures are the measurement: a project
with `#[path = "odd.rs"] mod odd;` inside `src/a.rs` reported `src/odd.rs`
`unused certain`, an `undeclared` on the alias, and — with an `include!` —
its target `unused certain` too. All three are gone; the corpus is
byte-identical in every repository, which is what a change with no corpus
population must look like.

**The anchor.** The Reference is explicit: a `#[path]` outside an inline module
block is relative to the DIRECTORY THE SOURCE FILE LIVES IN. For a mod-rs file
(a crate root, a `mod.rs`) that is where its child modules live, so `self`
names it; for any other file the children live one directory DEEPER, so the
same place is one module up. The adapter now spells the difference — `super`
where the file is not mod-rs — and the rule reads off the file name alone, the
way the resolver's own directory rule does.

**An alias is an alias everywhere.** The redirect table held raw path segments
and only `use` leaves consulted it, so `odd::run()` — an expression path —
resolved against a module named `odd` that does not exist. It now holds the
whole specifier, anchor included, and every path substitutes it: a `use` leaf,
an expression path, an attribute path. One table, one anchor, three readers.

**An include is sight, not a surface.** `include!("gen/tables.rs")` pastes a
file's items into the includer, so its private names are readable there. Two
shapes were wrong before the right one: no edge at all left the target
`unused` at file level, and a glob edge moved the same false positive down to
each item, since a whole-surface importer cannot name what is private to its
target. `ImportShape::Include` says what the language does, and the engine
reads it as SIGHT — the includer joins the target's viewers, which is the
relation the adapters' unit mates already spell from a different statement.
Nothing is handed out: a name the includer never writes is still dead, which
the fixture pins beside the one it does write. The argument is a file path
relative to the including file, so this adapter's specifier grammar gains one
form (`./gen/tables.rs`) that names a file rather than a module; a computed
argument (`concat!(env!("OUT_DIR"), …)`) names a file the build writes outside
the tree and draws nothing.

**Knobs and pins.** The contract fingerprint moves (`ImportShape` grows
`Include`); the WIT variant grows `%include` — the name is a WIT keyword, so
the escape keeps the wire spelling — and the compat guests are re-pinned;
`kndo:rust` bumps to 12. The report and query schemas do not move. One fixture
lands, `path-attribute-and-include`, pinning all three rules and the silence
between them, and kmock grows an `include` line so the engine rule has a
conformance case of its own. No existing fixture moves and no corpus finding
moves.

## 2026-09-06 — M8.c go 1/3: a package is a namespace, a module is a unit

**What Go's package is, said twice.** Until now `kndo:go` spelled the package
once, as `Reach::Scoped { scope: "package" }` — an adapter's own token, with a
`seen_from` hook enumerating the directory for it. The package is really two
facts and they live on different floors. It is a NAMESPACE, which extraction
now declares from the file's own bytes: the directory that addresses the
package plus the clause that names it. And its files CO-COMPILE, which stays
in `sees`, a query over the file set no file's bytes could answer. With the
namespace declared, `Reach::Namespace { up: 0 }` is what a lower-case name
takes, `Scoped` loses its last emitter, and the variant leaves the contract —
along with the ladder-less rung the pool machinery had to special-case.

**The segments are directory THEN clause, always.** `pkg/sub` + `package sub`
spells `["pkg", "sub", "sub"]`, and the repetition is the price of a unique
key: dropping the clause where it repeats the directory's last segment would
merge `a/b` + `package b` with `a` + `package b`, two different packages. It
also gives the external test package its own node — `pkg/sub` + `package
sub_test` is `["pkg", "sub", "sub_test"]` — which is stricter than the region
`seen_from` returned, and correct: an external test file may name only what
the package exports.

**The unit is the module.** `go.mod` now emits one published `Library` unit
over its own directory, entry-less (a Go module is entered through import
paths, never through a file) with every requirement as `depends_on`. That
retires the LIBRARY-MODE ROOT the audit filed as G11: `kndo:go` used to make
every non-internal, non-main, non-test file a `Probable` Production whole-file
root, so no file was ever unreachable and `test-only` was impossible at file
level. What roots a file now is the published surface it is on — an exported
top-level name in a published module — and its package siblings' sight of it.
A package that exports nothing and nobody imports is reached by nothing, which
is the true statement about it.

**A test file is on no published surface.** Retiring the library root exposed
an engine defect the go migration was the first to reach: `publishes` asked
only whether a file declares an exported top-level name in a published library
unit, and a `_test.go` file declaring `func TestWant` answered yes. Go is the
case that finds it, because a Go module is ONE unit holding production and
test files alike, where Cargo, Maven, Gradle and SwiftPM each give tests a unit
of their own that is never published. The rule is the engine's and it is
language-independent: a file that is a test as a whole is on no unit's surface,
because no importer can name what it exports. Publication therefore moved after
root anchoring — `mount_and_own` then `anchor_manifest_roots` then
`publish_surfaces`, in the full path and the surgical patch path alike, so both
read the same roots.

**`init` roots the binary it is compiled into.** The runtime calls every `init`
on package load, and the package a `_test.go` file loads into is the test
binary. Rooting it Production (the audit's G5) painted every package whose
tests use `init` production through the sight a test file has of its siblings —
eight files in gin. It roots Test in a `_test.go` file now.

**go.mod's grammar, read once.** Every directive has two spellings — `NAME
value` and a parenthesised block — and `//` comments are legal anywhere. The
module line was read by neither rule, so `module example.com/m // the API
module` produced a module path with the comment in it and every own-subpackage
import went `undeclared` (the audit's G7). One reader now answers `module`,
`tool` and `require` alike; the `tool` lines it can now see become `mentions`,
which is what a Go 1.24 tool dependency is — used with no import (G6).

**Measured: gin 109 → 110, one true positive.** The added finding is
`test-only testdata/protoexample/test.pb.go`, and the two changes compose to
produce it: gin's `_test.go` files were production-coloured before, because
each declares a `TestXxx` the module's published surface handed out, and the
generated protobuf file they import inherited that colour. With test files off
the surface they are test-only, and so is the only file they alone import —
which is what `testdata/protoexample` is. Nothing was removed, and
`internal-only` stays at 1 with `duplicate` at 108.

Retiring the library root moved nothing else in gin, which is the honest
reading of what that root was carrying: all seven of gin's packages declare an
exported top-level name, so every file either roots on the module's published
surface or is seen by a file that does. The two pools coincide there too — an
external `_test` package cannot name a lower-case declaration, so nothing left
the region `seen_from` used to return. The rest of the corpus is unmoved
(Alamofire 538, Exposed 971, flask 29, guava 8254, lodash 18, ripgrep 151,
vapor 199, vite 711): no other language's test files sit inside a published
library unit, because every other build system gives tests a unit of their own.

**What the corpus cannot show, the fixtures do.** Two land, and the previous
build is the control on both. `module-unit-and-package-namespace` now reports
`dead/dead.go` — a package of lower-case names nobody imports — where the
previous build, having rooted that file, reported one declaration inside it;
and it reports nothing on the module line the previous build called
`undeclared example.com/demo/lib`, the comment-in-the-module-path defect.
`test-file-init` now reports `internal/testutil/util.go` `test-only`, which the
previous build reported as nothing at all. Every existing fixture is
byte-identical: ten go projects, each with a `go.mod` and exported names, agree
across the whole migration.

**Knobs and pins.** The contract fingerprint moves (`Reach` loses `Scoped`);
the WIT variant drops `scoped` and the compat guests are re-pinned;
`GRAPH_SEMANTICS_VERSION` moves to 21 (the same evidence now assembles a
different `published`); `kndo:go` bumps to 6. The report and query schemas do
not move.

## 2026-09-06 — M8.c go 2/3: `sees` retires, co-visibility comes off the forest

**The mechanism the forest replaced.** `Extension::sees` asks each adapter to
enumerate, from paths, the files a given file compiles with. Four adapters
answer it and each answers with its own layout knowledge: go walked the
directory (asymmetric on `_test.go`), java the directory plus its main/test
mirror, kotlin the directory plus its mirrored main dirs, swift the target. All
four are re-derivations of structures the engine now holds — a namespace node
and a unit — which is why the design filed `sees` and `seen_from` under what
disappears. The go slice before this one declared go's namespace and then kept
`sees` anyway, on the true observation that reachability had no other input;
the honest reading of that observation is that reachability was missing a
capability, not that the old hook had earned its place.

**What a language compiles together is a language fact.** `Covisibility`, on
the spec: `Imports` (the default — the module graph is the whole story, which
is what rust, python and js-ts have always meant) or `Namespace` (go: `go build`
compiles every `.go` of the directory, so an importer that reaches one file
reaches all of them, and a file exporting nothing is alive because its package
is). The named consumer is reachability: `Reachability::compute` now takes the
scope forest and floods over `Scopes::covisible` — a namespace node's files —
beside the import edges. `Scopes` therefore moves ahead of both `Reachability`
and `Index` instead of being built inside the latter; one forest, read twice.

**The asymmetry is the engine's, once.** A file that is a test AS A WHOLE is
compiled into the test binary alone, so it neither carries the production
colour into the namespace it shares with production files nor takes it from
one; the test colour crosses in both directions. That is exactly what go's
`sees` encoded per file, and it is now one condition in the flood, expressed
against the whole-file test root the engine already computes for every
language. The same sentence already governs publication (M8.c go 1/3), so a
test file is off its unit's published surface and out of the production flood
for one reason rather than two.

**Measured: nothing moves.** gin stays at 110 and every corpus report is
byte-identical, which is the result a mechanism swap must produce when the new
mechanism holds what the old one re-derived. The proof that the capability is
load-bearing is the conformance case, not the corpus: kmock grows a `test-file`
line and `MockExtension::covisible()`, and the new engine test —
`a_covisible_namespace_reaches_its_own_files_but_never_through_a_test` — fails
with `unreachable` where it expects `production` when the extension is swapped
for the plain mock. `kndo:go` bumps to 7 and its `sees` test is replaced by one
that pins the declaration instead of an enumeration.

**What is left of the hook.** java, kotlin and swift still implement `sees`,
and until their slices declare their namespaces and units the engine has
nothing to read for them; `GraphFile.sees` therefore still carries two writers
— an `Include` import, which is sight a file states about itself, and the hook
for those three. The hook's doc-comment now says it is the pre-forest mechanism
and goes with the last adapter to leave it. `GRAPH_SEMANTICS_VERSION` moves to
22; the contract fingerprint does not move (the spec is not part of it — it is
part of the graph cache key, which the new field moves on its own).

## 2026-09-06 — M8.c go 3/3: the grammar's fields, read correctly

**Four fields and a receiver.** The audit's highest-severity go rows were not
design disagreements; they were readings of tree-sitter-go that the grammar
contradicts, each producing a wrong finding or hiding a right one.

- **Grouped `var`.** `var ( … )` wraps its specs in a `var_spec_list` and
  `const ( … )` does not — a grammar asymmetry, not a language one. A
  one-level walk over `var_declaration` reached the wrapper and stopped, so
  every name in a grouped `var` was invisible: not declared, not judged, not
  counted. The walk now descends through the wrapper and no further, because a
  `var` inside a function literal on the right of one declares a local.
- **The separating comma.** `const a, b = 1, 2` labels its commas with the
  `name` field, so reading the `name` children unfiltered declared a symbol
  called `,` — a finding on legal code that `go vet` is happy with. Only a
  NAMED child is a name.
- **Every name, not the first.** The same multi-name shape on the reading
  side: `is_use` asked whether a node WAS its parent's one `name` child, so
  the second name of `var a, b int` fell through as a use — of itself, on its
  own line. It now asks whether the node is ANY of them, which also covers
  `[T any]` type parameters for free.
- **The package clause.** `package_clause` carries no field at all, so the
  arm meant to exclude it never fired and `package foo` read as a use of
  anything named `foo` in the package.
- **The receiver.** Go requires a method's receiver base type to be declared
  in the same package: the receiver is part of that type's definition, not a
  use of it. Counting it made every type with at least one method unaccusable
  — the audit's G12.

Two binder classes go with them: the left of a `:=` (a short declaration, a
range clause, a receive) binds a local rather than naming a package
declaration, while the same position under `=` is an assignment whose names
must already exist, so writing one there IS a use.

**Two readings deliberately left as uses.** A composite literal's key
(`T{Field: v}`) is one grammar node for two languages' worth of meaning — a
struct field name, or a constant used as a map key — and the reading that
could ACCUSE is the one to avoid, so a key stays a reference. A selector's
operand (`fmt` in `fmt.Println`) stays one too: dropping it would lose the
evidence that the qualified-reference work resolves.

**Measured on gin: 1205 → 1253 declarations, 43,995 → 38,999 references,
110 → 110 findings.** Forty-eight names a grouped `var` was hiding are now
declared and judged, and roughly five thousand references that were never uses
— every `:=` binding, one package clause per file, the second name of every
multi-name spec, and a receiver for each of gin's ~430 methods — leave the
pool. The verdict does
not move, and that is itself the finding: gin has no dead grouped variable and
no type kept alive only by its own receiver, so eleven percent of its
reference stream was carrying nothing. Every existing conformance fixture is
byte-identical across the change; the new `grammar-fields` fixture reports one
finding per defect (`deadGrouped`, `first`, `gram`, `unreferenced`) and pins
the live counterpart of each beside it. `kndo:go` bumps to 8; no engine knob
moves.

## 2026-09-06 — M8.c go close-out: a header is as long as it is, and G10 is measured shut

**A generated marker sits in the HEADER, not in the first 24 lines.** Eight
adapters shared `kndo_toolkit::generated_marked`, which read the first 24 lines
of the first 2 KiB; `kndo:go` had its own stricter scan with the same bound at
20 lines. Both find a marker under a short licence and miss the same marker
under a long one, and then accuse code the generator owns — a `Certain` false
positive on every file with a long copyright block. `cmd/go` states the rule in
as many words: before the first non-comment, non-blank text. The toolkit now
answers exactly that with `header_lines` — a shebang, then every blank or
comment line (block comments carried) until the first line that is neither, no
line or byte cap — and `generated_marked` is that scan plus the needles. Go
keeps only what is Go's: the anchoring its toolchain writes, a line that IS
`// Code generated … DO NOT EDIT.` rather than a comment mentioning the words.
The mechanism promoted, the grammar knowledge stayed.

**A `_`-prefixed directory is NOT an ignore, and the toolchain said so.** The
audit filed `_ignored/i.go` beside `vendor/` as a path the go tool never
compiles, so the ignore list grew `**/_*/**` — and the `tool-ignored-paths`
fixture failed, exactly as it was written to: its own module imports its own
`_scratch` package, and an unclaimed directory turns that import into a
dependency no manifest declares. The question is decidable, and go1.24.7 is on
this machine, so it was decided by asking: `go list ./...` omits
`example.com/ask/_scratch`, and `go build ./app` on a package importing
`example.com/ask/_scratch` SUCCEEDS. The `_` rule governs pattern expansion,
not importability. The ignore is reverted and the criterion recorded in
M8.b.12 — a path the tool NEVER compiles — holds for `_`-prefixed directories
exactly as it holds for `testdata`. What the audit saw was a directory nothing
imports, which is an unreached file and a true finding.

**G10 closes as a measurement, not as code.** The audit's largest go gap — no
exported name in an imported package is ever accusable, though Go always
spells `pkg.Name` — was measured at zero for gin before the migration, and the
migration changed which keeper holds an exported name, so it was measured
again as an ablation: the whole-surface-importer keeper made inert, which is
the most a see-through keeper could ever retire. gin 110 → 110 and flask
29 → 29, against vapor 199 → 215 and Exposed 971 → 975. Go's zero is
structural: an exported Go name is kept by its module's published surface, so
the surface import is never its only keeper, and a keeper reading
`Reference::on` would retire nothing. Go therefore does not emit qualifiers
either — the evidence would have no consumer — and both the keeper and the
first qualifier emission belong to the swift and kotlin slices, where the
prize was measured twice. EXPERIMENTS carries the table.

**Measured: the whole corpus byte-identical.** No repository in the corpus
stamps a generated marker under a header longer than 24 lines, so the fix has
no corpus population and the probes are the measurement: a file with the
marker at line 26 reported its declaration `unused certain` before and is
silent now, and the `generated-late-marker` fixture pins it. Every existing
fixture is byte-identical. `kndo:go` bumps to 9; no engine knob moves.

## 2026-09-06 — `Covisibility` retires: a capability the shape already said

**It was not in the plan.** The design names eight new spec capabilities —
`ladder`, `nesting`, `dispatch_rules`, `file_roles`, `ignores`, `ecosystem`,
`dependency_importers`, `hidden_opt_in` — and `Covisibility` is none of them.
What the design does name, verbatim, is `Scopes::covisible(file)` and a flood
that reads it WITHOUT a condition ("colores = flood por imports Load/Lazy y
co-visibilidad, sin entrar a adjuntos TestOnly"). The go slice built the method
and the flood, then gated the flood behind a capability of its own invention.

**The gate was not decoration: it suppressed three true fixes.** Removing it
leaves every corpus report byte-identical except guava, 8254 → 8251, and all
three are `untested` on files that HAVE a test:
`guava/src/com/google/common/xml/XmlEscapers.java` and its android twin are
tested by `guava-tests/test/com/google/common/xml/XmlEscapersTest.java`, a
sibling source tree under the same declared package, and the third is a
`test-super` GWT mirror of the same shape. Java's `sees` walks a directory and
its main/test mirror, so it never bridges `guava/src` to `guava-tests/test`;
the namespace NODE is by declared package name across the compilation, which is
exactly what `NamespaceSpan::Compilation` exists to say, and it bridges them.
Gating the flood by a capability only go declared took from java an edge it had
already earned.

**Why no capability is needed.** What a namespace node holds is the language's
own statement, made in the shape of the namespace it declared: a package's
directory plus clause (go), a package name across a compilation (java), a
module's mount chain (rust — one file per node), a file that declared none
(everything not yet migrated — one file per node). A language whose namespace
is one file floods nothing, and says so by its evidence rather than by a knob.
The ablation is the proof: with the gate gone, only guava moves.

**A divergence recorded rather than assumed.** The plan's `nesting` capability
(`Flat`, `ByDirectory`, `Mounted`, `ByPath`, `PerFile`) has not landed either,
and this entry is where that stops being tacit. The engine derives the node's
shape from the evidence itself — segments declared, mount edges drawn, or
neither — which is the same distinction `nesting` was to carry and is closer to
"evidence in, exhaustive types out" than a second declaration of it. The claim
is re-checkable and will be re-checked when java, kotlin and swift declare
their namespaces: if any of them needs a shape the evidence cannot express,
`nesting` lands then, with that need as its named consumer.

`GRAPH_SEMANTICS_VERSION` moves to 23. The contract fingerprint does not (a
spec field is part of the graph cache key, not of the shape hash), no
conformance fixture moves, and `kndo:go` does not bump: its evidence is
unchanged.

## 2026-09-06 — `file_roles`: a convention is declared, never concluded

**The adapter was saying "dog".** `kndo:go` read a path, decided the file was a
test, and wrote a whole-file Test root into its evidence. That is a conclusion
about the ANALYSIS, reached inside an adapter, from a fact the adapter could
have simply stated. Forty-five sites across the nine adapters read a path or a
name and conclude a role that way; this is the vocabulary that lets them stop.

**The capability.** `FileRole { glob, kind, confidence }` on the spec, with the
two constructors a language actually needs: `certain` for a rule the toolchain
enforces (`go test` compiles exactly the `_test.go` files and runs nothing
else) and `probable` for a habit the ecosystem keeps but nothing checks
(`test_*.py`). The named consumer is root anchoring, and the PRECEDENCE is the
engine's, stated once: a unit that declared its files' role — a Cargo test
target, a Maven test source set — has already said so, and the conventions are
read only for a file no unit spoke for. Nine adapters would otherwise each have
to remember that ordering.

**What it retires.** go's whole-file Test root leaves extraction; the adapter
keeps only the narrower fact that pass still needs — which of two binaries a
declaration compiles into, since that changes what a root ON it means. The
glob compiles through the same `path_glob_set` an ignore does, so `**/*_test.go`
reads identically in both, and the engine matches it against the project path
rather than an adapter re-deriving a suffix test.

**Measured: the corpus is byte-identical.** A mechanism swap with the same
verdict is what a faithful one looks like: gin's `_test.go` files are anchored
Test by the engine now instead of by the adapter, and every report agrees to
the byte. `kndo:go` bumps to 10 — its evidence genuinely changed, one root fewer
per test file. `GRAPH_SEMANTICS_VERSION` deliberately does NOT move: the fact
is one (go declares the role rather than emitting the root), the adapter
version names it, and the spec is already part of the graph cache key, so no
warm graph can answer for a tree read under the old spelling. Bumping both
would be two places for one fact.

## 2026-09-07 — `Effect::Generated`: a generator's output is judged by one law, not by eight adapters

**Eight adapters, four different answers.** Every source adapter recognised its
ecosystem's generated banner and each DECIDED for itself what that meant.
go, java, kotlin, python and swift skipped the file's declarations and rooted
nothing. rust, ts and css skipped the declarations AND rooted the whole file
`Tooling` at `Probable` — so the same fact produced a whole-file `unused`
finding in one language and silence in another, for reasons no reader could
find in one place. The adapters were saying "dog": each concluded a verdict
from a fact it could have simply reported.

**The vocabulary.** `Effect::Generated` joins `Root` and `Exempt` in the spec's
dispatch effects: the adapter reports the banner it found as a file marker
under one language-neutral token (`kndo_toolkit::GENERATED_MARKER`, the LINE it
matched carried as the argument so `describe` shows what convinced it), a rule
in its spec says that token means `Generated`, and the engine holds the one
verdict. `RunContext::judges_declarations(file)` is the seam five analyses read
— `unused` (its declaration loop only), `duplicate` (both), `crap`,
`internal-only`, `private-type-leak`. `untested` deliberately does not: coverage
of a generated file is a fact about the test suite, not an accusation about a
name.

**The owner's decision: no judgment on the declarations AND no root.** A
generated file's names are the generator's, so accusing one asks the wrong
party. Whether the FILE earns its place in the tree is a different question and
still this project's: nothing imports a stale `.pb` either, and deleting the
generator's line for it is exactly the fix. So `Generated` withholds judgment
from what the file declares and roots nothing — the rust/ts/css `Tooling` root
is retired, and a generated orphan is reported like any other orphan. Both
halves are pinned in one fixture (`kndo-adapter-ts` `generated-file`) and both
kmock conformance cases.

**Measured on the corpus: 3 generated files across nine repositories, 2
findings move, both explained.**

| repo | generated files | findings | delta |
| --- | --- | --- | --- |
| gin | 1 | 110 → 109 | `test-only` on `testdata/protoexample/test.pb.go` goes |
| guava | 2 | 8251 → 8250 | `duplicate` on `PublicSuffixPatterns.java` goes |
| Alamofire, Exposed, flask, lodash, ripgrep, vapor, vite | 0 | unchanged | — |

gin's is the law working in both directions at once: the file now reports its
declarations, and one of them is `func init()`, which the go runtime calls on
package load — so a Production root reaches the file and "only tests reach it"
was never true. The generated banner withholds judgment from the file's names;
it never hid the program's shape. guava's is the seam: two byte-identical
copies of a `PublicSuffixPatterns.java` that a tool wrote from the public
suffix list, and telling this project that one duplicates the other is telling
the wrong party. Nothing anywhere became MORE accused: no generated file in
this corpus is an orphan, so the owner's "no root" half costs nothing here and
buys the law its consistency.

**Not v1's rule, and not v2's old one.** v1 rooted generated files as tooling
output, which is where v2's rust/ts/css inherited it; the vice is that a root
is a claim that something outside the graph USES the file, and a banner makes
no such claim. It says who WROTE it.

**The pairing is structural, not remembered.** `mark_generated` needs two spec
declarations to work — `EvidenceStream::Markers` and the rule — and a missing
one drops the marker with a diagnostic and no verdict. Rather than ask eight
adapters to remember, `source_adapter_builder` declares both, and
`ExtensionSpecBuilder::emits`/`dispatch` now UNION and APPEND instead of
replacing: a shared builder's declaration can no longer be silently clobbered
by an adapter that lists only what its own grammar adds. css builds its spec
directly and declares the pair itself; html declares nothing here because it
declares no symbols at all.

`GRAPH_SEMANTICS_VERSION` moves to 24 — the engine assembles the same evidence
differently, and an external adapter could trigger it with no built-in changing.
Every source adapter bumps its own version too, because each genuinely emits
different evidence now: `kndo:rust` 13, `kndo:go` 11, `kndo:java` 12,
`kndo:kotlin` 7, `kndo:swift` 4, `kndo:python` 3, `kndo:js-ts` 10, `kndo:css` 2.
These are two facts, not one fact in two places. The contract fingerprint does
not move (a dispatch effect is spec data, part of the graph cache key rather
than the shape hash). The WIT `effect` variant gains `generated` and the four
pinned reference components are re-pinned in the same commit, as the ABI's own
pre-release rule requires. Four conformance fixtures move: `generated-file`
(kndo-adapter-go), `generated-late-marker` (kndo-adapter-go), `generated-file`
(kndo-adapter-ts), and `generated-sheet-stays-silent` (kndo-adapter-css), which
is renamed `generated-sheet-is-an-orphan-like-any-other` because its old name
claimed the verdict the owner just reversed.

## 2026-09-07 — `Trigger::Name`: a name that dispatches is declared, and the file's ROLE is what qualifies it

**The adapters were concluding again.** `go test` runs a `_test.go` file's
`TestXxx` by name; the Go runtime calls every `init` when the package loads;
cargo calls a target's `fn main`. Three toolchain behaviors, and all three were
`if name == …` inside extraction, with the qualifier read off a PATH:
`ends_with("_test.go")` in go, `build.rs || examples/` in rust. A path is not
what a toolchain reads.

**The vocabulary.** `Trigger::Name { pattern, kind, in_files }` joins
`Trigger::Marker`: a declaration whose own name matches the pattern, narrowed
to a symbol kind and to the files where the convention holds. Only a
declaration NOTHING OWNS matches — a member dispatched by name (`test*` on an
`XCTestCase`, JUnit 3's `testFoo`) is its owner's business and waits for the
trigger that says so.

**The deviation, named.** The plan spells the qualifier `in_unit:
Option<UnitKind>`, and its own go row then asks for rules "in TestOnly" and
"outside TestOnly" — a per-FILE attachment, not a unit kind. Both cannot be the
same field, and go is the case that decides: a Go module is ONE library unit,
so a unit kind can never say "the `_test.go` files". What both sentences
actually mean is the file's ROLE, and v2 already has one spelling for it — a
unit's kind and a declared `FileRole` glob both land as a whole-file root, so
`InFiles::{Any, Rooted(color), NotRooted(color)}` reads either without the rule
knowing which the project used. `NotRooted` is not a convenience: `init` runs
in whichever binary links the file, so coloring it takes both halves.

**What it retires.** go's `runner_entry` helper and its `init` root leave
extraction, and rust's `main_root_kind` path convention goes with the root it
colored — rust now reads the target kind CARGO declared, so a `fn main` under
`examples/` is tooling because cargo said the file is an example, not because
the path contains the word. The one root extraction still concludes is go's
`func main`, and it is deliberately left: it is a NAMESPACE fact (`package
main`), not a name one, and the plan's own answer for it — a module's entries
are its `main` packages — is manifest work (M8.d). Named here so it cannot go
quiet.

**Where the phase runs.** A marker's meaning is a pure function of a file's
evidence; a name rule's qualifier is the file's role, which only the assembled
project knows. So dispatch moved out of the graph constructor into one pass
(`dispatch_files`) that runs after manifest anchoring and before publication,
in both the full and the patched build. One place where dispatch happens, and
the patched graph and a full build still agree to the byte.

**Measured: the corpus is byte-identical, and the ablation says the rules
earn their place.** All nine repositories agree with the previous run to the
byte — a mechanism swap with the same verdict is what a faithful one looks
like. The number that shows the rules are load-bearing comes from ablating
them: dropping go's runner rule takes gin from 109 findings to 119, and all
ten are `TestXxx`/`BenchmarkXxx` under `internal/bytesconv` and `internal/fs`
— exactly where the `internal` fence caps an exported name's reach below the
published surface, so the runner's root is its only keeper. Everywhere else in
gin the entry surface already keeps them; under the fence, nothing does.

**The pattern grammar's cost, measured.** `Test*` is a prefix glob, while
`go test` runs a name only when the character after the prefix is not a
lowercase letter (`TestFoo` yes, `Testing` no). Across gin's 658 runner
entries, zero names would be over-matched by the bare prefix; the direction
where it happens is keep-alive (a `Testing` in a test file kept, never accused
wrongly), and it can only reach a declaration in a file already rooted Test.
The grammar gains a character class the day a corpus repository pays for it,
and not before.

`GRAPH_SEMANTICS_VERSION` moves to 25 — the engine derives roots from evidence
it derived none from before, and an external adapter could trigger it with no
built-in changing. `kndo:go` bumps to 12 and `kndo:rust` to 14: each emits
genuinely different evidence (two conclusions fewer, one fewer). The contract
fingerprint does not move (a trigger is spec data, part of the graph cache key
rather than the shape hash). The WIT `trigger` variant gains `name` with its
`in-files` and `name-trigger` records, and the four pinned reference
components are re-pinned in the same commit. One conformance fixture is added,
`main-in-every-target` (kndo-adapter-rust), pinning that a build script's, a
binary's and an example's `fn main` are each kept in the color cargo's target
kind implies; no existing fixture's pinned report moves.

## 2026-09-07 — `Effect::Witness`: a promise its owner made, not a root, and not a hardcoded name list

**Two adapters were concluding, in two different registers.** `kndo:java`
turned `@Override` into a Production ROOT and carried a five-name
`SERIALIZATION_HOOKS` list in extraction that rooted `readObject`,
`writeObject`, `readResolve`, `writeReplace` and `readObjectNoData` wherever
they appeared — by name, on any class, whether or not it was serializable at
all. A root is a claim that something OUTSIDE the graph is entered here, and
it paints the file with a color; an override makes no such claim, and neither
does a method on a class the runtime will never serialize.

**The vocabulary.** `Effect::Witness` joins `Root`, `Exempt` and `Generated`:
the declaration satisfies a surface its OWNER promised, so it is alive while
its owner is and carries no color. `Trigger::ExternalWitness { base, members }`
states the requirements of a base the project does not contain — the case the
graph can never resolve, because `Comparable` and `Serializable` live in the
JDK. Where the base IS in the project, its own members are the requirements
and no rule is needed: that half has worked since M8.b, and this slice makes
the two halves one keeper, `Keeper::Witness { of }`, which now RENDERS the
base it names (`witness:Comparable`, `witness:Override`) instead of carrying
it write-only.

**A deviation taken here and reversed the same day — see the entry below.**
This slice shipped `ExternalWitness` alone, on the argument that `MemberOf {
owner: Relation { base }, name }` is the same predicate. That argument was
wrong: the plan's `Relation` carries a `RelationKind`, so the composition
distinguishes `extends` from `implements` and the shorthand does not. The
owner's ruling is that the plan stands as written; the next entry lands the
whole enum.

**A relation is matched through the whole declared chain, and the first
attempt got that wrong.** Reading only the relation the file itself reports
put 18 new `unused` findings on guava — every one a `readResolve` on a class
like `Absent`, which is serializable through `Optional` and never says so
itself. The serialization runtime does not care which link named the base, so
neither does the rule: the matcher walks the project's supertype edges by NAME
(the base a rule names is only ever a target, never a key), cycle-safe.

**Measured on the corpus: every report byte-identical, and three ablations
behind it.**

| what | guava |
| --- | --- |
| shipped (both rules) | 8250 — byte-identical to the previous run |
| without `@Override` | 8302 (+52 members it keeps) |
| without the base table | 8250 (+0 — the name-collision keeper covers them) |
| without the base table, name-collision keeper ablated | 27405 vs 27396 (+9) |

The base table is invisible today because a much broader keeper is standing in
front of it: `keepers` keeps a member alive on ANY reachable reference to its
name, and ablating that takes guava from 8,250 to 27,405. The table is 9 of
those nineteen thousand, precisely. EXPERIMENTS carries the number, because
the plan's "witnesses replace the keep-alive by name collision" is a real
direction and this is its first measurement.

**A defect the change exposed, fixed in the same commit.** `internal-only`
already stood down for a witness the graph RESOLVED — narrowing one is a
compile error, not advice — but read only that half, so the 37 `setUp`/
`tearDown` overrides guava declares against JUnit's `TestCase` became findings
the moment `@Override` stopped being a root. `GraphFile::stated_witness` is
the seam both halves now read.

`GRAPH_SEMANTICS_VERSION` moves to 26; `kndo:java` bumps to 13 (it emits one
root fewer per serialization hook, and none at all for `@Override`). The
contract fingerprint does not move. The WIT `effect` variant gains `witness`
and `trigger` gains `external-witness`, with the pinned reference components
re-pinned. One conformance fixture is added, `runtime-required-members`
(kndo-adapter-java), which pins that a `writeObject` on a class implementing
nothing is judged like any other member — the control that makes it a rule
about a base and not about a name; no existing fixture's pinned report moves.

## 2026-09-07 — The dispatch vocabulary is the plan's, entire: `Relation`, `MemberOf`, `Marker.target`, and a Pattern its bindings qualify

**The owner's ruling, and the standing rule it sets.** Where the plan replaces
a mechanism, nothing of the old one stays; and the plan is followed as
written, because it does not contemplate two shapes of the same fact living
side by side. Three earlier judgment calls are reversed by it, and one
correction is mine to own.

**What was missing, and now is not.** The plan's `pub enum Trigger` block
declares five shapes. The tree carried three:

| plan | before | now |
| --- | --- | --- |
| `Marker { path, target: Option<SymbolKind> }` | `Marker { path, arg }` | `Marker { path, arg, target }` |
| `Relation { kind: RelationKind, to: Pattern }` | absent | landed |
| `Name { pattern, kind, in_unit }` | `Name { pattern, kind, in_files }` | unchanged — see below |
| `MemberOf { owner: Box<Trigger>, name }` | absent | landed |
| `ExternalWitness { base, members }` | landed | unchanged |

**My argument for folding two of them was wrong, and the reason matters.** I
claimed `MemberOf { owner: Relation { base }, name }` was `ExternalWitness {
base, [name] }` written twice. It is not: the plan's `Relation` carries the
KIND of the relation, so the composition can say "a member of a type that
IMPLEMENTS this" and the shorthand cannot — it reads any relation, of either
kind, through the whole supertype chain. Two different predicates, both
useful, both in the plan. And my second claim, that a type-shaped `Relation`
had no consumer, was circular: I had folded the predicate into
`ExternalWitness` myself and then observed that nothing used it.

**`Marker.target` earns its place immediately.** `@Override` means something
only on a method, and java's rule now says so rather than trusting the Java
grammar to put the annotation nowhere else.

**A Pattern is compared against two spellings, which is what the plan asks
for.** "El motor califica el path de un marcador o el nombre de una relación a
través de los bindings del archivo antes de comparar." So a marker path and a
relation name are matched against the name AS WRITTEN and against the name
the file's own binding imports qualify. A rule written `com.vendor.Closer`
reaches an `implements Closer` in a file that imports it and says nothing
about the identically-named type from another package — measured in the
`runtime-required-members` fixture, where `Qualified.shut()` is
`witness:com.vendor.Closer` and `Homonym.shut()`, package-private and
promised by nobody, is reported. Matching the written spelling too is not a
loophole: it is how a language's IMPLICIT scope reaches a rule, since
`java.lang.Comparable` is spelled `Comparable` in every file that uses it and
no import qualifies it. Java's built-in table keeps the bare names for that
reason; a rule pack for a framework is where the full names earn their keep,
which is the plan's own example set (`org.junit.jupiter.api.*`,
`*.XCTestCase`).

**The one thing still not the plan's literal shape, and why.** `Name` carries
`in_files: InFiles` where the plan writes `in_unit: Option<UnitKind>`. The
plan's own go row then asks for rules "en TestOnly" and "fuera de TestOnly" —
a per-FILE attachment, which a unit kind cannot express because a Go module
is ONE library unit. The two sentences cannot both be honored by one field,
and `InFiles` is the reading that satisfies both: a unit's kind and a declared
file role both land as a root on the file, so `in_unit: Some(Test)` is
strictly what `InFiles::Rooted(Test)` says. Recorded, unchanged, and
re-checkable.

**The ABI carries the recursive trigger without an unrepresentable case.** A
WIT variant cannot name itself, so a rule's trigger crosses as a FLAT LIST of
nodes with the root last and every `MemberOf` owner pointing at an earlier
one. The host rebuilds the tree and REFUSES a rule whose owner index is not
smaller than the node naming it — a rule the host cannot read is one it must
not guess at — rather than the alternative of an ABI that expresses less than
the trait.

**Measured: every corpus report byte-identical.** Nine repositories, no
finding moves: `Marker.target` on `@Override` (a Java compiler already
rejects the annotation elsewhere), the qualification (java's own rules name
JDK types the source writes bare), and the two new triggers (their language
consumers arrive with the rule packs) all land without changing a verdict.
Two kmock conformance cases carry the vocabulary's proof —
`a_rule_can_name_a_base_and_its_requirement_separately` shows `implements`
matching and `extends` NOT matching the same base, and
`a_marker_rule_can_name_the_kind_it_means` shows one marker on a function and
a type with only the function rooted.

`GRAPH_SEMANTICS_VERSION` moves to 27 and `kndo:java` to 14. The contract
fingerprint does not move. The WIT `trigger` becomes `trigger-node` with
`relation` and `member-of`, `dispatch-rule.when` becomes a list, and the four
pinned reference components are re-pinned. One conformance fixture moves,
`runtime-required-members` (kndo-adapter-java), which gains the two
same-simple-name classes.

## 2026-09-07 — Attachment is evidence, and `InFiles` is retired

**A file's membership in its namespace is its own statement, not a root.** The
plan's `Attachment { Regular | TestOnly }` lands as a spine field of
`FileEvidence`, written through `EvidenceSink::attachment`, and `InFiles` is
deleted. `Trigger::Name` carries the plan's literal signature —
`{ pattern, kind, in_unit: Option<UnitKind> }` — and `in_unit` narrows to the
KIND OF COMPILATION the file lands in: its unit's kind, or the test build
where its attachment is `TestOnly`. `GraphFile::compiled_into` derives that
once, and `is_test_file` reads it.

**The correction this makes.** The 2026-09-06 entry recorded that
`in_unit: Some(Test)` is "strictly what `InFiles::Rooted(Test)` says". That
reading was wrong in one direction and it mattered: a whole-file Test root
answers "is this an ENTRY", which a `LoadTest.java` on the main source path
carries while being compiled into the library like anything beside it. The
membership question is a different one, and only the file (or its unit) can
answer it. Two facts, two carriers.

**The writers, by the same rule everywhere.** A file states `TestOnly` where
the language's own tooling compiles it into the test build ALONE, never
because a name merely looks like a test: go's `_test.go`, the JVM's
`src/test/{java,kotlin}` source sets, SwiftPM's `Tests/`, what pytest
collects (`test_*.py`, `*_test.py`, `conftest.py`), and the web's
`*.test.*`/`*.spec.*`/`__tests__/`. `*Test.java`, `*Tests.kt` and
`LoadTests.swift` outside those trees keep `Regular` and keep their
importable surface — the adapters' own prose already said so, and now the
type says it too.

**Measured: every corpus report byte-identical, and the ablation says why.**
Nine repositories, no finding moves — the carrier changed, the verdicts did
not. Ablating the seven emissions (attachment written nowhere, `is_test_file`
still reading it) moves gin 109 → 119 and vite 711 → 704: gin's ten are the
`Test*`/`Benchmark*` runners in `internal/**_test.go`, accused the moment
`in_unit: Test` stops matching, and vite's seven are `test-only` dependencies
of two `__tests__/package.json` files that stop being test files.
guava, Exposed, vapor, Alamofire and flask do not move under the ablation at
all: their manifests declare test units, so the unit already answered and the
attachment only agrees with it. The attachment is load-bearing exactly where
no manifest declares a test compilation.

**The ABI stops dropping two spine fields in silence.** `record file-evidence`
gained `namespace` and `attachment` — a WASM adapter could write either
through the sink and watch it vanish at the boundary, which is the same
failure the wire's own comment warns about. `unit-kind` joins the vocabulary,
`in-files` leaves it, and a rule whose `in_unit` this SDK build cannot spell
is DROPPED WHOLE rather than sent without its narrowing: absent means "every
compilation", so a silent widening would be worse than the missing rule.

`GRAPH_SEMANTICS_VERSION` moves to 28. The contract fingerprint moves —
`FileEvidence` gained a field. `kndo:go` moves to 13, `kndo:rust` to 15,
`kndo:java` to 15, `kndo:kotlin` to 8, `kndo:swift` to 5, `kndo:python` to 4,
`kndo:js-ts` to 11 and `kndo:html` to 3. No conformance fixture moves. The
four pinned reference components are re-pinned, and the kmock DSL gains
`test-only` beside `test-file` so a fixture can state the two facts apart.

## 2026-09-07 — `sees` retires in java and swift: the forest answers

**Measured first, then deleted.** Ablating `Extension::sees` one adapter at a
time: swift **0** findings, java **2**, kotlin 15. Swift's carried nothing
because its `internal` declarations already pool over `seen_from`'s
`module_region`, a superset of the target files `sees` enumerated, and its
public ones ride the whole-file root its extraction still emits. Java's two
were the co-visibility edge a test needs to reach the class it exercises, and
that edge belongs to the forest.

**The forest's missing half: what a build PULLS IN.** `Scopes` already spanned
a namespace across units for POOLS — who may name a package-private member:
the units that compile against mine. `covisible`, which reachability floods
over, read only the node's own files, so a Java test module never reached the
library it exercises. It now reads the span in the OPPOSITE direction — the
same-named files of every unit THIS one compiles against — and the asymmetry
is the whole point: a test build holds the library, a library build holds no
test of it.

Taking the union of both directions was the first attempt and guava says why
it is wrong: its GWT super-source declares `com.google.common.base`, guava-gwt
depends on guava, and `src-super/…/Platform.java` is compiled INSTEAD of the
library's, never beside it. The union made fifteen replacement-source files
look exercised. A dependent's files are never in the dependency's build.

**Measured: guava 8250 → 8249, every other repository byte-identical.** The
one finding is `guava-gwt/src/com/google/common/ForceGuavaCompilationEntryPoint.java`,
which stops being `untested`. guava's parent pom declares
`<sourceDirectory>src</sourceDirectory>`, so guava-gwt's main set is `src/` and
its test set `test/` — a layout java's directory mirror (`src/main/java` ↔
`src/test/java`) could never pair, which is why `sees` missed that
`guava-gwt/test/com/google/common/GwtTestSuite.java` is that file's own
package-mate in its own module's test set. The forest reads the pom and gets
it right where a path convention was blind. v2 reporting one finding LESS,
because it read the project instead of guessing at directories.

Kotlin's `sees` stays: kotlin emits no namespace clause, so the forest has
nothing to answer with. Its 15 are the EXPERIMENTS ledger's row 1c, and they
go with M8.d.

`GRAPH_SEMANTICS_VERSION` moves to 29. One conformance fixture moves,
package-private-across-modules (kndo-adapter-java), which loses an `untested`
finding on a library file its tests module demonstrably calls — the fixture's
own `[[alive]]` and `[[dead]]` claims are unchanged. The contract fingerprint
does not move, and no adapter version does: `sees` was never evidence.

## 2026-09-07 — `sees` is gone from the vocabulary, not just from three adapters

**The owner's rule, and it is the right one.** A capability that one adapter
keeps and eight do not is a second toolset, and a second toolset is a second
thing to explain to whoever writes the tenth adapter. `Extension::sees` is
deleted — from the trait, from the `extension` world in the WIT, from the SDK
shim, from the host bridge, from the reference guests, from `sees_of`, and from
`GraphFile.sees`, which had two writers and keeps only the one that is a fact
the file states: its `Include` imports. The field is now `includes` and
`Index::seen_by` is `included_by`, because a name that means one thing is worth
more than a name that used to mean two.

**Kotlin pays with the same coin as everyone else.** It has `package com.foo`,
exactly as Java does, so it emits a namespace clause and declares
`NamespaceSpan::Compilation`. That is floor 1 of where language knowledge
lives: a fact about ONE FILE's content is an evidence stream, never a hook that
walks directories.

**The reference guest was teaching the old thing.** `kmini` said its two halves
compile together by handing the engine a mate path; it now DECLARES the
namespace `x` for both `x.kmini` and `x_part.kmini`, and its unexported
declarations reach `Namespace { up: 0 }` rather than `File` — which is what the
language actually means, and what a reader learns to copy.

**Measured: Exposed 971 → 976, every other repository byte-identical.** The
ablation ledger sized kotlin's `sees` at 15 findings; the namespace clause
recovers 10 of them. The other 5 are files whose own module's test source set
exercises them (`exposed-migration-r2dbc`, the springboot sample): main and test
are different namespace ROOTS there, so only a unit's friendship joins them, and
kotlin reads no Gradle. They close with M8.d and are listed in EXPERIMENTS.

**What was tried and abandoned, so nobody rebuilds it.** Emitting Gradle units
from `build.gradle[.kts]` inside this slice — a main unit and a test friend,
mirroring the Maven half — moved guava 8249 → 8665 and Exposed 976 → 984, worse
in both directions. Gradle's structural manifest is M8.d's, with its own
fixtures and its own measurement, and improvising it here was guessing dressed
as progress.

**The rule is born with its gate.** `every_adapter_declares_its_namespace_in_one_vocabulary`
extracts one file per language and fails if an adapter states neither a
namespace clause nor a mount, and fails the other way if an adapter on the
"still owes one" list has quietly paid up. The exception rows name what each
remaining adapter owes and where.

`GRAPH_SEMANTICS_VERSION` moves to 30 and `kndo:kotlin` to 9. The contract
fingerprint does not move — a trait is not a contract type. The `extension`
world loses an export, so the four pinned reference components are re-pinned.
No conformance fixture moves.

## 2026-09-07 — M8.d begins: `Package.swift` is a structural manifest

**The first of the milestone's seven parsers.** `Package.swift` is read with
tree-sitter-swift, as the plan's swift row states: targets with `path`,
`exclude` and `sources`, products, and TEST TARGETS AS FRIENDS OF THEIR
DEPENDENCIES — `@testable import App` reaches App's `internal`, and SwiftPM is
the only thing that knows which targets a test target may do that to.

**The bug the real manifests found.** A first cut walked the whole tree for
`.target(...)` calls and read vapor's `Vapor` five times: a `.target(name:)`
inside another target's `dependencies:` NAMES a target, it does not declare
one. SwiftPM labels both `targets:` and `products:`, so the labels are the
reading — the same reason the plan says to use the grammar and not a scan.
Against the real files: Alamofire yields exactly its two units with `Source`,
`Source/Info.plist` excluded, and the test target friends of the library;
vapor yields its eight.

**Measured: vapor 199 → 218, every other repository byte-identical.** All 19
are `internal-only`, and all 19 are true: the old `module_region` bounded
`internal` by "the target's files plus EVERY file under any `Tests/` tree", a
deliberately generous superset that hid narrowable declarations. With the real
units, `TestError` and `Payload` are each declared and used in one file — and
`Performance/` turns out to be its own package with its own `Package.swift`.

**`seen_from` measured 486 → 4, and stays for those 4.** Deleting it now turns
three gates red for one reason: Alamofire's `Example/` and `watchOS Example/`
are XCODE PROJECTS (`.xcodeproj`), covered by no SwiftPM target, so their
`Unit{0}` has no bound and every declaration there becomes keep-alive. Two
`apple-bundles` known gaps close on their own (the gate correctly demands they
be promoted) and the `InterfaceBuilder` conduct proof loses what it was
proving. The plan's sentence for this is "sin manifest: unidad por primer
segmento + `*Tests` como Test", and its SCOPE is the owner's to settle —
per-project (no `Package.swift` anywhere) or per-file (outside every declared
target). Improvising the seam is what this entry refuses to do.

**A debt this environment cannot pay.** The plan requires each parser validated
against the real tool in captured fixtures. `mvn`, `gradle`, `go`, `npm` and
`cargo` are installed here; `swift` is not, so `swift package dump-package` is
owed for this parser. It is validated against the two real corpus manifests and
four unit tests instead, and the capture stays on the ledger.

`GRAPH_SEMANTICS_VERSION` moves to 31 and `kndo:swift` to 6. The contract
fingerprint does not move. No conformance fixture moves.

## 2026-09-07 — `seen_from` is gone, and no adapter enumerates files any more

**The owner's question was the right one: why was it still there.** `sees` had
been deleted; `seen_from` had not, and I had been treating its last four
findings as a plan-scope question when they were fixture maintenance. Both
hooks are now gone from the vocabulary — trait, WIT, SDK, host, both reference
guests — and with them `sees_of`, `regions_of`, `GraphFile.regions` and
`region_of`, the whole apparatus for holding a list of files an adapter walked.

**What answers instead.** An adapter DECLARES: a namespace clause, a mount, a
structured reach, and its manifest's units. `Reach::Unit { up: 0 }` reads the
unit pool where a manifest named the unit and the mount tree where the language
mounts its namespaces. Where neither speaks, the reach is UNBOUNDED and the
contract's own law applies: keep-alive, the typed absence. v2 abstains rather
than guessing from a directory layout.

**Measured: Alamofire 538 → 534, Exposed 976 → 934, seven repositories
byte-identical.** Alamofire's 4 are in `Example/` and `watchOS Example/`, Xcode
projects no SwiftPM target covers. Exposed's 42 are kotlin `internal`
declarations that had been bounded by a `/src/`-shaped path convention;
kotlin reads no Gradle, so no manifest names its units and every one of them
now abstains. Both are v2 declining to accuse on a guess, and both close when
their parser lands — swift's already did for 482 of its 486.

**Three fixtures the deletion moved, each recorded rather than papered over.**
`apple-bundles` now declares its two app targets in its own `Package.swift`,
because a source tree no target declares is keep-alive and would prove nothing
— the `InterfaceBuilder` conduct proof needs its subjects judgeable, which is
what a real SwiftPM package shipping a storyboard looks like. The `ladder`
kmock fixture gained the `kmock.pkg` that names its unit, for the same reason.
And `internal-scope` (kotlin, `build.gradle.kts`) turns its `lonely` claim into
a `known_gap` with `fix = "M8.d kotlin (Gradle units)"`: the declaration IS
dead and SHOULD be reported, and saying so in the ledger is more honest than
swapping the fixture's build system to make the number come out.

`GRAPH_SEMANTICS_VERSION` moves to 32, `kndo:kotlin` to 10 and `kndo:swift` to
7. The contract fingerprint does not move. Two conformance fixtures move,
internal-scope and ctor-arg-and-default-value (both kndo-adapter-kotlin), plus
apple-bundles (kndo-apple). The `extension` world loses an export, so the four
pinned reference components are re-pinned.

## 2026-09-07 — Twenty whole-file roots become nine declarations, and four rules the deletion exposed

Nine adapters read a path and concluded a root. Twenty of those conclusions
were the same sentence in nine dialects — "this directory is the test source
set", "this filename is the entry", "this page is a document" — and every one
is now a `FileRole` the spec DECLARES and the engine applies, once, with the
precedence a manifest deserves: a unit that named its files' role outranks
every convention. The globs are ADDITIVE where the code was exclusive, because
an `if test { … } else { … }` is not a fact about a path, it is a fact about
one adapter's control flow.

**Six roots did not become declarations, and each says why in its own place.**
A shebang (js-ts) and `if __name__ == "__main__"` (python) are the FILE's own
statements, not its path's: they stay evidence. Two library-mode Production
roots — kotlin's and python's — stay with their measurement in a comment, and
they are the same debt: "any non-test file is importable published surface" is
the engine's `publishes()` to say, and `publishes()` reads a unit neither
language's manifest parser exists to name yet. **Measured: deleting kotlin's
costs Exposed +125, deleting python's costs flask +15** — `src/flask/app.py`
and `cli.py` among them, unreached because no root was left to reach them from.
They go with the Gradle and pyproject parsers (M8.d), not before.

Swift's library-mode root DID go, because its replacement is now live: swift
declares its namespace. Swift spells nothing between a module and a name, so
the SwiftPM target IS the namespace, and `resolve`'s own `target_of` — path
only, by construction — became the namespace clause each file emits. The
adapter that owed the scope forest a namespace has paid; the gate that names
the debtors lost a row.

**Four rules the deletion exposed, each a false positive the old root was
hiding.** (1) A file the test build alone compiles seeds no production or
tooling flood, whatever colour a root on it claims — lodash's `npm run test:fp`
script had been making `test/test-fp.js` a TOOLING entry, laundering that
colour onto everything a test reaches (**lodash +2 test-only, both true**).
(2) A unit-wide reach whose unit no manifest named falls back to the namespace
the file declared, for a language that says the two are one thing
(`UnnamedUnit::Namespace`, a new capability with a default that abstains):
swift's Xcode example apps are bounded again, kotlin's `internal` still
abstains — **without the capability the same fallback costs Exposed +54.**
(3) An export is nameable inside its own namespace with no import at all, so a
sibling's bare use is a use — corpus byte-identical, and it is what the swift
conformance fixture caught. (4) A CERTAIN root on a member keeps its owner: a
class whose `main` the launcher names is named through it. `Probable`/`Possible`
do NOT travel that way — inheriting a maybe is how silence spreads, and letting
them travel costs another 115 findings of it on Alamofire alone.

Java's `main` root became `Certain` by matching the JLS rule whole — `public
static void main(String[])`, return type and signature included — instead of
the name and two modifiers.

**Measured across the corpus: Alamofire 534 → 426, vapor 218 → 176, vite 711 →
691, flask 29 → 26, guava 8249 → 8247, lodash 18 → 20, three repositories
byte-identical.** The 155 that leave Alamofire and vapor are XCTest case
CLASSES — `ApplicationTests`, `CacheTestCase` — which the runner instantiates
by reflection and v2 had been calling dead. The 23 that leave vite and flask
are HTML pages: their Production root is now an ENGINE anchor, and `untested`
has always held that an engine-anchored production entry is wiring rather than
logic to test. lodash's +2 are the laundered-colour fix above.

`GRAPH_SEMANTICS_VERSION` moves to 33; `kndo:swift` to 8, `kndo:java` to 17,
`kndo:python` to 5, `kndo:js-ts` to 12, `kndo:html` to 4. The contract
fingerprint does not move — `ExtensionSpec` is a declaration, not a contract
type. One conformance fixture moves: document-entries.

## 2026-09-07 — M8.d: Python's manifests are parsed, and swift stops reading its own twice

`pyproject.toml`, `setup.cfg` and `requirements*.txt` are read as TOML and INI
instead of scanned by line, into the units, entries, packages and dependencies
the project model needs. The line scanner they replace could only find
dependency NAMES, which is why python had no units at all — and no units is why
its adapter still concluded a whole-file Production root on every non-test
module. That root is now deleted: `publishes()` reads the unit, and the unit
exists.

**Two facts decide everything, and Python states neither in one place.** The
first is the source root: PEP 621 names the distribution and says nothing about
where its code lives, so each backend is asked in turn — setuptools'
`package-dir` and `packages.find.where`, poetry's `packages = [{from = …}]`,
hatch's wheel target — and where none speaks, setuptools' own auto-discovery
rule answers: a `src` directory beside the manifest MEANS a src-layout. Its
existence is the rule, not a name matching the distribution's, which is how
`type-checking-cycle` ships `pkg`. The second is publication: a `[project]`
table is a distribution, and the one thing that says otherwise is `Private ::
Do Not Upload`, the classifier the index itself refuses an upload for.

**The reader is graded against Python's own tools, not against our reading of a
PEP.** `crates/kndo-adapter-python/tests/captured/tooling.json` holds what
`packaging` 24.0 answered for fourteen PEP 508 specifiers and their PEP 503
canonical forms, and what `setuptools` 68.1.2 answered for three pyproject
layouts and a setup.cfg; `tests/tooling.rs` replays those answers through the
adapter. The capture also settled a question the docs do not: setuptools'
`read_configuration` does NOT run discovery, so `packages.find.where` stays in
the file and the file is where it is read from.

**Two entries, because Python has two.** A `[project.scripts]` value
`pkg.mod:func` names a file, and `import pkg` executes `pkg/__init__.py`
whatever that file declares — so a re-export-only `__init__.py` under a source
root is the door the name opens and cannot be dead while the distribution
ships. A plain module is NOT that: `import dark` reaches `dark.py`'s own
exports, which the published surface already answers, and calling it an entry
tells `untested` it is wiring. Measured: the broad reading silenced the
coverage ingester's own proof, which is the gate that caught it.

**Measured: flask 26 → 20, every other repository byte-identical.** Five
`untested` and one `unused` under `tests/` go, because
`[tool.pytest.ini_options] testpaths` is now a Test unit rather than a
directory the library root had painted production. Ablating that unit alone
costs flask +16, and ablating the library root with the units in place costs
nothing — which is the whole measurement this slice was for.

Six python fixtures gained the `pyproject.toml` a real src-layout project has;
without one, a source tree no manifest describes proves nothing. All eight
python conformance fixtures move, and their FINDINGS are identical — only the
health block's unit and package fields differ.

Swift's `manifest_dependencies` is gone: `Package.swift` is read once, and the
package's `dependencies:` list is read from the `Package(...)` call's own
argument rather than from anywhere in the tree, so a `.package(url:)` a target
mentions is not counted twice. And `kndo-testkit` gains `manifest_evidence` —
kndo's dogfood reported the fourth verbatim copy of that harness as a
`duplicate`, which is the rule working.

`kndo:swift` moves to 9 and `kndo:python` to 6. Neither the contract
fingerprint nor `GRAPH_SEMANTICS_VERSION` moves — an adapter emitting
different evidence is the adapter's knob. Eight conformance fixtures move, all
in kndo-adapter-python fixtures.

## 2026-09-07 — M8.d: the pom is read as a document, and one manifest states a unit once

`pom.xml` is parsed with `roxmltree` instead of scanned by line, and the two
line scanners it replaces are deleted. The scanner could not tell a
`<dependency>` from the `<exclusion>` inside it, nor a declared dependency from
a `<dependencyManagement>` entry nobody declared — and both are Maven's own
answers, captured here rather than reasoned about:
`crates/kndo-toolkit/tests/captured/maven.json` is what `mvn help:effective-pom`
(Maven 3.9.11) returned for the two-pom reactor beside it, and `tests/maven.rs`
asks the reader the same questions.

**What the scanner actually answered, on those exact files.** For the child it
reported `hamcrest-core` and `org.hamcrest:hamcrest-core` — an EXCLUSION — and
lost `junit` entirely, because the exclusion's `<artifactId>` overwrote the
dependency's before the closing tag. For the parent, which declares no
dependency at all, it reported `managed-only`, reading the
`<dependencyManagement>` block as declarations. A document parser makes both
impossible rather than harder.

The corpus does not move, and that is the honest result: java's
`DependencyIdentity` is `Underivable` — a JVM coordinate names no importable
package — so every usage judgment on these already abstains and a wrong
dependency list changed no finding. The defect was real and invisible, which is
exactly the kind a capture catches and a corpus cannot.

**One manifest states a thing once, however many adapters claim it.** java and
kotlin both claim `**/pom.xml`, because a project with both languages has one
Maven build; with kotlin now reading `extract_manifest` too, the merge would
have produced two of every unit. The engine unions per manifest instead of
concatenating — what each adapter adds is what the others did not already say.

java's and kotlin's `packages` and `manifest_dependencies` hooks are gone, and
the toolkit's `dependencies` dispatcher and `maven` scanner with them. Four of
the eight remaining trait-impl hooks retire here; js-ts's four are M8.d's last
row. `kndo:java` moves to 18 and `kndo:kotlin` to 12 — kotlin now reads a
manifest it did not before. Neither the fingerprint nor
`GRAPH_SEMANTICS_VERSION` moves, and no conformance fixture moves.

Gradle keeps its scanner for now, moved inside the one hook rather than left as
a second door: the block scanner and the version catalog the plan calls for are
their own slice, and it is the one that lets kotlin's library-mode root die.

## 2026-09-08 — Gradle is a block scanner over a comment-blanked copy, graded against Gradle

A `build.gradle(.kts)` is not a line-oriented file, and the three cases that
prove it are all in the two-module build captured beside the reader
(`crates/kndo-toolkit/tests/captured/`, what a `kndoReport` task printed from
inside Gradle 8.14.3): `include(\n    "app",\n)` resolves to a project and a
line reader misses it; `// include("legacy")` names nothing and a line reader
makes a module out of it; and `implementation(libs.guava)` has no coordinate in
the script at all — it is in `gradle/libs.versions.toml`, which is now a
manifest the JVM adapters claim as data for the scripts beside it, never a unit
of its own.

So the reader is a scanner over calls, bounded by their PARENTHESES, on a copy
with comments blanked to spaces (offsets preserved, so every span still points
at the original). `settings.gradle(.kts)` states the modules as packages and
member manifests; each `build.gradle(.kts)` states the two units Gradle's java
plugin gives it, what each compiles against, and — the reason the reader exists
— that the test set is the main set's FRIEND, because Kotlin's `internal` and
Java's package-private both reach a module's own tests.

`kndo:kotlin` moves to 13 and `kndo:java` to 19: both claim the same manifests
through `jvm_manifest::MANIFEST_GLOBS`, so both now emit different evidence from
the same sources. Neither the fingerprint nor `GRAPH_SEMANTICS_VERSION` moves.

## 2026-09-08 — kotlin's library-mode Production root is deleted

The last whole-file root an adapter concluded from a path convention. With
Gradle read, a published unit's surface is what roots its files and the engine
says so; a module that publishes nothing roots nothing.

Measured on Exposed, 934 → 972, decomposed in `corpus-findings/COMPARISON.md`:
+28 `internal-only` in a category that reported ZERO for Kotlin before (with no
module, `internal` had no bound and the rung was unjudgeable), +18 `unused`
(14 Spring/JUnit in one sample module, owed to M8.e; 3 the pinned grammar, owed
to M8.f; 1 true), −8 `untested` (3 are the same Spring beans, now `unused`
rather than two verdicts at once; 5 are genuinely reached by their module's own
test source set, which is a stated unit for the first time).

Every other repository is byte-identical.

Conformance fixtures moved, all `kndo-adapter-kotlin`: `internal-scope` (the
file-level finding now subsumes the two declaration verdicts, and its
expectations say so), `ctor-arg-and-default-value` (two `internal-only`
advisories the module bound makes sayable), and three new ones —
`gradle-multi-module`, which pins that `include(…)` across lines is a package
and a commented-out one is not, plus `when-guard-grammar-gap` and
`infix-get-grammar-gap`, which hold the two grammar gaps open.

The ablation the ledger recorded before the parser existed stands as the reason
this waited: deleting this root with no Gradle reader cost Exposed +125. It now
costs the 38 above, each one named.

The price of the judgment is real and recorded rather than hidden: Exposed goes
1.0s → 1.8s (release, warm page cache, cold graph cache) and 7.5s → 26s in a
debug build, because unit pools are now computed for 5150 files that had none.

## 2026-09-08 — Killed: a file is alive whenever any declaration in it has a keeper

Under test while `internal-scope` was rewritten. The idea was to close the gap
that fixture holds open — `Caller.kt` calls `com.pkga.helper()` by qualified
name with no import, so the file is compiled and used yet no root reaches it.

Measured: Exposed +4, guava +1, **vite +26**. The rule turns a genuinely dead
file into a report of every declaration inside it instead of one report of the
file, which is strictly worse output for the same defect. Killed.

The narrower rule the measurement points at — a file is reached by a resolved
reference FROM A REACHABLE FILE — is its own slice with its own number, and the
three known gaps in `internal-scope/expectations.toml` name it as their fix so
the day it lands the fixture fails.

## 2026-09-08 — js-ts speaks the one door, and a tsconfig alias is a package

`kndo:js-ts` was the last adapter reading manifests through `roots`, `packages`,
`manifest_dependencies` and `manifest_mentions`. It now writes `ManifestSink`:
a `package.json` states the unit npm compiles, its entries, its publication
(`"private": true` → `Unpublished`) and its declared dependencies; the files its
`scripts` run stay the manifest's own tooling roots. The corpus is unmoved by
that half and no conformance fixture moved, which is the equivalence — the
entries changed door, not colour.

`tsconfig.json` joins the manifests js-ts claims, for one fact: a
`compilerOptions.paths` alias. An alias is a NAME that resolves to a FILE, which
is what a `PackageEntry` already is, so it travels as one rather than as a new
concept — exact aliases carry an entry, wildcard aliases carry the directory
their subpath resolves against. Two spellings sharpened in `resolve`: the whole
specifier is tried against the package map before it is split (an npm name is a
scope and a name and stops there, so a declared name with more segments can only
be an alias), and `@/` is not a scope.

Measured: vite 691 → 687 findings and 2342 → **2474** import edges; every other
repository byte-identical. The six `unused` that leave are files nothing could be
seen to use — `playground/test-utils.ts` has 127 importers spelling `~utils` —
and two return as `test-only`, the true verdict.

Measured and NOT read, so nobody rebuilds them expecting a number: `references`
(14 on the corpus, all between configs declaring no unit), `include`/`exclude`
(kndo claims by suffix; no finding turns on them), `extends` (55 tsconfigs, no
alias inherited rather than declared). A multi-target wildcard takes the first
target that lands; the corpus's one such mapping has no import site.

One conformance fixture is added and none moved: `tsconfig-path-aliases`
(`kndo-adapter-ts`) pins both alias shapes resolving, a target outside the
project stating nothing, and a file under a wildcard's directory that no
specifier spells staying dead — an alias makes a name resolvable, never a file
reachable.

`kndo:js-ts` moves to 13. The four hooks are gone from every built-in adapter;
they stay on the trait and in the WIT for the WASM guests that still export
them, and closing that door is M8.d's last row.

## 2026-09-08 — the manifest door closes: four hooks and three ABI exports retire

`roots`, `packages`, `manifest_dependencies` and `manifest_mentions` are gone
from the `Extension` trait, and `roots`, `packages` and `manifest-dependencies`
from `wit/extension.wit`. `extract-manifest` takes their place on both sides:
one export, one `ManifestSink`, the same validation for a WASM guest's writes as
for a native adapter's, because the host replays them through the real sink.

The ABI can now carry what the old exports could not, which is why this is a
replacement rather than a rename: `unit` (with its kind, roots, excludes,
entries, `depends-on`, `friend-of` and `publication`), `dependency-declaration`
with its scope and version requirement — the SDK's "until a versioned world
carries them" is spent — plus `members` and `diagnostics`. The compat pins are
rebuilt in this commit, which is the reviewable record of the break.

One phase retires with them. `Phase::Manifest` existed only for the bytes-in
names-out `manifest-dependencies`; a manifest read resolves the entries it
names, so `extract-manifest` runs in the PROJECT phase like `resolve`, and the
rude-probe's gate test is re-aimed at what is still illegal there — reaching for
the assembled graph while the graph is being built.

Activation reads the same door: it needs the dependency NAMES a manifest
declares, and a manifest states them beside everything else, so the pre-graph
pass calls `extract_manifest` over the discovered paths instead of a second hook.

The corpus is byte-identical across all nine repositories — the door changed,
not a verdict. What proves the new surface is exercised rather than merely
declared: the compliance suite drives a guest's whole unit across the ABI
(kind, entries, `depends_on`, publication, packages and dependencies in one
call), and the compat matrix drives the pinned components to a real verdict.

`anchor_manifest_roots` loses two parameters it no longer needs; nothing in the
engine consults an adapter about a manifest except through the one door.

## 2026-09-08 — a rule pack is an extension that claims nothing and declares rules

What a FRAMEWORK means is no language's to own, and it is not core's either.
A rule pack is an `Extension` with no suffixes and nothing in its spec but
`DispatchRule`s; `dispatch_files` applies every pack's rules beside the
claiming adapter's, in composition order, for every file.

No activation, deliberately. `Activation` exists for conduct extensions, which
RUN; a declarative rule is gated by its own trigger — a `@RestController` no
file carries fires nowhere — so a pack needs no manifest to read, no cache key
to widen, and it works unchanged in the surgical patch path.

Two packs ship, each with its corpus number in its own module doc:
`kndo:spring` (stereotypes root, handlers witness — 6 findings on Exposed) and
`kndo:swiftui` (the runtime's entry protocols root, their requirements witness
— 2 on Alamofire, which stay unreachable until swift emits relations).

## 2026-09-08 — measured: eight of the ten planned packs have no corpus population

The corpus was instrumented for all ten before any was written. Whole
addressable population: ~16 findings — 13 Spring, 2 SwiftUI, 1 JUnit. `testng`,
`lombok`, `rstest`, `pytest`, `storybook` and `vitest` are **zero** on the
pinned nine repositories. They are not written, and this is the entry that says
so: the corpus covers languages, not frameworks, and shipping a pack against
zero would be a rule whose first run was against our own fixtures.

## 2026-09-08 — the rule-pack machinery was unreachable for six of nine languages

Of the nine adapters, only `kndo:java` emits markers AND relations; rust and go
emit markers alone; kotlin, swift, python and js-ts emitted NEITHER. The
`DispatchRule` vocabulary landed in M8.a with one producer, which is why the
packs fired on nothing when first composed.

`kndo:kotlin` moves to 14 and closes its half: annotations as markers,
supertypes as relations (a constructor call is the superclass, a bare name an
interface — the compiler's own rule and the only signal one file carries), and
the receiver a member access was read from.

Measured on Exposed, 972 → 966: −6 `unused` (spring), −4 `internal-only`
(`protected` members of abstract bases whose subtypes are in other files — the
heirs pool is real now), +3 `untested` (the beans are production-reachable and
no test reaches them), and **+2 `internal-only` that are wrong**, on
`SqlTypeProvider`'s two members, decomposed in `corpus-findings/COMPARISON.md`
with the minimal reproduction that does NOT reproduce them. Shipped with the
defect named and counted rather than held: the net is six true silences and
four withdrawn advisories against two wrong ones, and the wrong two now have a
written account for whoever fixes `internal_only`'s member branch.

One conformance fixture is added and none moved: `spring-beans`
(`kndo-adapter-kotlin`) pins a `@RestController` alive with its `@GetMapping`
handler and an identical unannotated class dead beside it — the trigger is the
gate, and a pack silences only what it means.

## 2026-09-08 — `rules_for` is reverted: the plan's two gates are activation and a qualified path

The owner read the root design against the tree and found the deviation. Both
of the symptoms this repository hit last week — `kndo:spring` rooting Vapor's
`@Controller struct` and its routes (−20 on vapor), and `builtin_conduct_proofs`
over `apple-bundles` collapsing to zero findings — come from ONE decision that
was mine and not the plan's: rule packs were built as ordinary extensions whose
rules `dispatch_files` applies to every file unconditionally, under the entry
"No activation, deliberately" of this same day. The design says a pack is a
CONDUCT extension that "declares activation and rules", riding the existing
mechanism — which already has activation, reported contributions and
baseline-then-plugin proofs — and this file's own 2026-09-05 entry transcribed
that correctly before the code went the other way.

`ExtensionSpec::rules_for`, the patch over the first symptom, is reverted with
its number: **the vapor bleed is closed by qualification alone.** With the pack
forced `Activation::Always` and its rules written as full paths, vapor measures
176 findings — byte-identical to the pack being absent. `DeclarationCx::spells`
already resolves a marker's path through the file's own import bindings; what
was wrong was the DATA, ten rules written as bare names. A bare `Controller`
is the same six letters in two ecosystems, which is exactly what the plan's
`Pattern`-over-qualified-names is for. `rules_for` was a second mechanism for a
problem the contract had already solved, and it named languages inside a
framework pack, which is the ignorance rule inverted.

What lands instead, all of it the plan's:

- **`ActivationRule::FileImports`** in the contract — the M8.a debt the plan
  marks "viene de M8.a, se consume acá". Activation runs before anything is
  parsed, so it matches the pattern's literal stem against discovered file text
  and is coarse BY CONTRACT: the gate opens, the triggers decide. Not yet a
  producer among the shipped packs; `kndo:testng`, `kndo:xctest` and the SwiftUI
  pair are the named consumers, each blocked on its adapter.
- **`ActivationRule::ManifestDependency` matches a pattern**, not an exact
  name: the plan writes `junit*` and `org.springframework*`, and one ecosystem
  spells one framework many ways. `matches_pattern` is promoted to the
  contract's one `Pattern` semantics, shared with `DispatchRule`.
- **A pack is a conduct extension.** `dispatch_files` consults only ACTIVE
  packs; the active coordinates enter the graph cache key, which is how a pack
  keeps `MutatesGraph::No` and the cache both — the plan's "un pack activo NO
  invalida la caché: las reglas son dato y entran en la clave".
- **A pack's contribution is counted where it happens.** It runs no code, so
  `Dispatch` counts the roots its rules alone derive and the conduct round
  reports them in the same row every plugin gets. Exposed: `kndo:spring`, 77
  roots.
- **The `rule_packs_are_data` gate**, from the plan's gate table: a pack
  declares activation and rules and nothing else (no claims, no manifests, no
  content access, no report paths, no finding rules, never `Always`), and every
  pattern it compares through bindings is qualified. This gate fails
  `fd52ea4` as written, which is why it is the deliverable and not the fix.
- **`kndo:spring` on the plan's shape**: stereotypes AND handlers are
  `Production` roots at `Probable` (this repository had shipped `Certain`
  stereotypes and `Witness` handlers). Same silences on this corpus, an honest
  claim instead of an overstated one.

`GRAPH_SEMANTICS_VERSION` 33 → 34: the same evidence assembles differently now
that pack rules are gated. The WIT `activation-rule` variant gains
`file-imports`, the fingerprint moves with the contract shape, and
`abi/compat/*.wasm` are re-pinned. One conformance fixture moves,
`spring-beans` (`kndo-adapter-kotlin`): its build file now DECLARES Spring —
which is what activates the pack at all, and what the fixture must pin — and it
gains a `fun main` so the project has a root, because a rootless graph abstains
and the baseline of a baseline-then-pack proof cannot be empty.

## 2026-09-08 — the packs whose adapter cannot feed them yet, named rather than shipped

`kndo:swiftui` is withdrawn from the composition, not deleted in silence. The
plan's dependency line for M8.e is "depende de M8.b y **del adapter
correspondiente**", and swift emits neither markers nor relations: every rule
in that pack fires on nothing, and `builtin_conduct_proofs` — which now closes
over every conducting coordinate — cannot be given a baseline-then-pack proof
for a pack that changes nothing. It returns with the swift tranche of M8.c,
alongside `kndo:uikit`, `kndo:xctest` and `kndo:swift-testing`, and its rules
return in the plan's shape: `PreviewProvider.previews`, `View.body`, `App.body`
and `Scene.body` as EXTERNAL WITNESSES. The version shipped in `fd52ea4` also
rooted the conforming type `Tooling` — an invention, and the direct cause of
`apple-bundles` reporting nothing: a witness keeps a member alive while its
owner lives, so `ContentView_Previews` stays accused, while a root on the type
silences it and the proof's own control disappears.

Still owed to M8.e by the same rule, each blocked on its adapter and none of
them measured against zero: `kndo:junit`, `kndo:testng` and `kndo:lombok` (JVM,
reachable now — next slice); `kndo:rstest` and kin (rust, markers only, so the
name and marker rules are reachable); `kndo:pytest`/`django`/`flask` and
`kndo:storybook`/`vitest` (python and js-ts emit no markers). The crate is
`kndo-packs`, the plan's name, not `kndo-rules`.

## 2026-09-08 — Swift's promise is one list, so it is one relation kind

`kndo:swift` moves to 10 and reports markers and relations: every attribute a
declaration carries with its arguments, plus `override` (the glossary counts a
MODIFIER as a marker, and it is the one Swift modifier a rule reads), and every
name in an inheritance list — including a retroactive `extension Foo: Codable`,
which is FOO's promise and is attributed to Foo.

**One kind, and it is `Implements`.** Swift's grammar does not separate a
superclass from a protocol: `class A: B, C` is legal with B either, and only
the whole program knows which. The contract's `Implements` is documented as
"promises another type's surface", which a subclass does as much as a
conformer, and nothing in the engine reads the kind — a `Trigger::Relation`
and the supertype edges both compare NAMES. So the adapter states the sentence
it can prove instead of guessing which of the plan's four kinds applies. The
two unused variants of the design's `RelationKind` (`Conforms`, `Overrides`)
stay unbuilt until an adapter can tell them apart AND a consumer reads them —
a capability whose consumer cannot be named does not land.

Measured: Alamofire 426 → 419, vapor 176 → 175, everything else
byte-identical; all eight are `internal-only` advisories withdrawn because the
member sits on a promised surface. Ablation pins the cause: markers alone,
relations withheld, reproduces 426/176 exactly.

**The conformer-methods heuristic stays one more slice, with its number.**
Deleting swift's hand-written `Possible` root on every non-private method of a
conforming type — which the design replaces with witnesses — measures Alamofire
+63 and vapor +15. Those 78 are answerable by the design (relations resolved in
the project, plus `kndo:xctest`/`kndo:swiftui` stating the requirements of
bases outside it) and unanswerable without it, so the deletion lands in the
same commit as its replacement. `protocol-requirement-reach`
(`kndo-adapter-swift`) is added and carries it as a `known_gap` naming that
fix; the fixture pins the half that works today, both directions of it: an
overridden member cannot narrow, and the member beside it still can.

## 2026-09-08 — a value is a use, a backtick is spelling, and a Swift member says what it was read from

`kndo:swift` moves to 11 with the rest of its evidence row, each half priced by
ablation against the 419/175 baseline.

- **S5, by deletion.** `property_declaration` was listed as a binder seat. Its
  `name` field IS a `pattern`, and `pattern` was already on the list, so the
  only identifier the entry ever suppressed was the `value`: `let alpha = beta`
  threw `beta` away. Worth 2 `unused` on Alamofire. The design asks the toolkit
  for `Seats` — (kind, field) pairs replacing kind lists — and that promotion
  waits for its second consumer, which is python's parameter default and
  kotlin's constructor default in this same close-out. One caller does not
  earn an abstraction when the fix is removing a word.
- **Backticks come off both sides.** Worth 2 more `unused`: `Endpoint.default`
  and `TestParameters.default` are declared `` `default` `` and reached
  `.default`, so declaration and use never joined.
- **`Reference::on` for `expr.member`,** taken from the navigation's `target`
  where the source spells it as a name and absent where it does not (`a.b.c`,
  `f().x` name nothing a pool can use). Declaring `Qualifiers` opens
  `internal_only`'s member branch, which abstains for any adapter that cannot
  show a receiver: +58 advisories, 50 of Alamofire's 53 sampled as ordinary
  internal members of internal helper types named in one file.

Seven of the 58 sit on a `Codable`/`Content` conformer, whose stored properties
the compiler also reads for the synthesized conformance. The advice is legal —
narrowing them compiles — and the design's `codable-synthesis` rule would still
prefer to make them witnesses. It is NOT taken: expressing it needs a kind
filter on `Trigger::MemberOf` (the synthesis reads stored properties, not
methods) that the contract does not have, and seven findings do not buy a
contract change. Recorded so it is not rediscovered.

Two fixtures added, none moved: `backtick-names` (both directions of the
spelling, plus the value of a binding read across files) and — from the
previous slice — `protocol-requirement-reach`.

## 2026-09-08 — an operator is a name, and a requirement is as visible as its protocol

`kndo:swift` moves to 12 and closes the rest of its row. Four grammar-level
gaps, measured together because each is small and they interact.

- **Operators, both halves or neither.** A `func ==` has no `name` field —
  tree-sitter leaves the operator an anonymous token — so the declaration was
  dropped whole, and `a == b` spends no named node, so the use was invisible
  too. Two silences that cancel: 10 declarations across Alamofire and vapor,
  0 findings before and 0 after. Shipping only the declaration half would have
  invented 10 accusations, which is why the reference half (a fixed list of the
  expression kinds whose operator token is a name a `func` could declare) lands
  in the same commit. The declarations now reach `describe`, `used-by`, the
  metrics and `duplicate`.
- **`protocol_property_declaration` is its own node kind** and was walked past:
  a protocol's `var v: Int { get }` was never declared.
- **`Reach::Inherited`.** A protocol requirement is as visible as its protocol,
  and a `public extension` hands its modifier down to members that spell none.
  Worth −2 `internal-only` — advisories that would not have compiled.
- **A `private extension`'s members are the FILE's.** This is the reach the
  source writes, and it unmasked a TRUE positive: vapor declares `static var
  space` in two files, and the module-wide pool let the used one keep the
  unused one alive. +1 `unused`, correct.

Closing that reach also exposed a pre-existing hole, fixed here: `case .space`
is a PATTERN, every identifier under a pattern was a binder seat, and so
enum-case dot-shorthand — the pervasive use form in Swift, and the pool-side
counterpart of never declaring the cases — produced no reference at all. A
pattern that starts with `.` binds nothing.

Alamofire 468 → 467, vapor 180 → 180 (−1 advisory, +1 true accusation),
everything else byte-identical. One fixture added, `operators`, pinning a used
operator alive and a declared-never-written one dead.

Still owed on `kndo:swift`, each with its blocker: the conformer-methods
`Possible` root and its 78 findings (needs `kndo:xctest` and `kndo:swiftui`,
M8.e), and ERROR-tolerant traversal (M8.f, where the design puts it).

## 2026-09-08 — Python: decorators are markers, bases are relations, and `_x` reaches its distribution

`kndo:python` moves to 7 with three of the design's row, measured on flask
(20 → 18; every other repository byte-identical).

- **Decorators as markers, with the path the source writes** (`pytest.fixture`,
  `app.route`), which the engine qualifies through the file's own import
  bindings before any rule compares it — the same protection that stops a JVM
  rule from speaking about a Swift file. Class bases as relations, one per
  base: `class C(Base, metaclass=M)` names ONE supertype, because a keyword
  argument configures a class rather than being one. Additive; no rule reads a
  Python marker yet, and the blanket `Possible` root on a decorated definition
  stays until `kndo:pytest`/`django`/`flask` can replace it (M8.e).
- **`Seats` lands in the toolkit**, on its second consumer as the law requires.
  Swift's version of this bug was fixed by deleting a word; Python's cannot be
  — `a: int = D` and `a = D` bind one name and read another under one node, so
  the seat must name the FIELD. Audit finding P3. Zero corpus movement: flask
  defaults to imported names that other evidence already kept, and the
  `default-values` fixture is what pins it.
- **`_x` → `Unit{0}`**, the owner's decision of 2026-09-05, replacing
  `Reach::File`. Both lost findings are here and both are true positives:
  `app.py`'s dead `_make_timedelta` is joined by name to `sansio/app.py`'s live
  one (the cost every unit-scoped rung pays), and `cli.py`'s
  `_path_is_ancestor` is kept by an opaque namespace import, because the
  design's keeper list gives `Unit` reach to a surface-importer. Python's
  `Unit{0}` means "internal to the distribution", which is not "part of the
  module's surface" — every other language's rung makes those the same thing.
  Named, not patched: one finding does not buy a keeper rule that reads a
  language. The rung still accuses (`_has_encoding` stands), which is the whole
  point of the decision over `Exported`.

Still owed on this adapter: qualified references (`via: Binding(mod)`), string
annotations as references, nested and conditional defs, `type X`, and
`Nesting::ByPath`.

## 2026-09-08 — Python: the quotes, the guard, and the dot

`kndo:python` moves to 8. flask holds at 18 findings and gains 9 SUBJECTS —
this slice widens what is judged rather than what is accused.

- **A forward annotation is a type.** `target: "_Later"` names `_Later` the way
  the unquoted form does; the quotes exist because the name is not bound at
  runtime under `if TYPE_CHECKING`, which is the dominant idiom in typed
  Python. Reading only identifiers made every such class read as dead. The
  string is split on the type-expression punctuation (`[] , |`) so
  `"list[_Later] | None"` names what it names, and a string anywhere else — an
  argument, a plain expression — is data and reports nothing.
- **A def behind a guard is module surface.** `if TYPE_CHECKING:`, a
  `try/except ImportError` fallback, a `sys.version_info` branch: the guard
  decides WHICH definition binds, never whether the name exists. With PEP 695's
  `type X = …`, this is the +9.
- **`Reference::on` for `obj.attr`.** Zero corpus movement, and the reason is
  the design's own: Python's ladder is deliberately empty — no keyword sits
  between module-private and public — so `internal-only` never speaks for it
  and the receiver opens no advisory branch as it did for Swift. What it buys
  is attribution inside `unused`: a qualified use lands on the member it names
  instead of on every member of that name.

Fixture `string-annotations` added, pinning both directions.

Still owed on this adapter: `Nesting::ByPath{roots}` — the last item of its
design row.

## 2026-09-08 — `Nesting::ByPath`, and the `_x` rung re-decided on new evidence

Two changes that only make sense together, and the second REVISES the owner's
decision of 2026-09-05 rather than inheriting it. Decisions are answers to a
context; this context has units, a keeper list, and `Reference::on`, none of
which existed then.

**`Nesting` becomes a spec capability the ENGINE consumes** — `PerFile` by
default, so every other adapter is byte-identical, and `ByPath` for python,
where the namespace is the file's dotted path under its unit's source root
(`src/app/views.py` under root `src` is `app.views`, `src/app/__init__.py` IS
`app`). The engine derives it because extraction never sees a source root and
the manifest is the only thing that knows one. The design also names `Flat`,
`ByDirectory` and `Mounted`; none is built, and this is the disposition rather
than a silent cut: each is already achieved from the other side — java and
kotlin emit the `package` clause, go emits its clause and reads `internal/` as
`Directory{up}`, rust declares `ImportShape::Mount` — so an engine-side variant
nothing reads would be vocabulary without a caller. Corpus: byte-identical.

**`_x` moves from `Unit{0}` to `Namespace{0}`, the module.** Measured on flask,
counting the underscore accusations each rung keeps: the module rung 3
(`_make_timedelta`, `_path_is_ancestor`, `_has_encoding`), the distribution rung
1, the package rung 0. All three are hand-checked true positives; the wider
rungs lose them to name collision across the distribution and to the
surface-import keeper, not to any use.

**And the thing that made the old reasoning right is now built.** The
2026-09-05 rationale was "`mod._x()` from a sibling is legal and common, so
never accuse it". The design's keeper list carries exactly that case — a
qualified reference resolved to this file — and the engine had NO such rule:
`Reference::on` was read only by `internal_only`. `Keeper::Qualified` closes it:
a file that imports this module under a local name and writes `local.name` keeps
the declaration, whatever the reach, because the qualifier names the module out
loud. A bounded reach bounds who may name a declaration WITHOUT a qualifier.

The `underscore-namespace-access` fixture found the gap — it was written to pin
the decision and instead reported the used function dead — and now pins both
directions. flask 18 → 20; everything else byte-identical.

## 2026-09-08 — the contract's shapes, all of them, consumer or not

The owner's instruction, and a correction to how this repository has been
reading its own law: a mechanism the design details EXISTS, whether or not a
consumer for it exists yet. "A capability whose consumer you cannot name does
not land" is a wrong-floor test for a capability someone INVENTED; it is not a
licence to ship less than the approved design. Absent consumers are transitory.

Audited the whole of contract v3 against the tree and closed the evidence half:

- **`RelationKind` gains `Conforms` and `Overrides`** — the plan's four. An
  adapter states the narrowest its grammar can PROVE; `Implements` stays the
  word for "promises another type's surface" where the syntax separates
  nothing, which is what Swift, Java and Python can say today.
- **`Qualifier`** (`Binding(local) | Path(segments)`) and **`TypeRef`**
  (`{ name, via }`) land as the plan writes them, and `Relation::to` becomes a
  `TypeRef`. A `Trigger::Relation` now matches both spellings — the bare name
  the language wrote and the qualified one its own bindings make of it — the
  same two a marker path already got.
- **`ImportTarget::Pattern(glob)`** lands: many files at once, resolved by the
  engine against what it discovered rather than enumerated by the adapter.

`GRAPH_SEMANTICS_VERSION` did not move (the same evidence assembles the same
way); the contract fingerprint did, once, and `abi/compat/*.wasm` are re-pinned
against `kndo:vocab` with the four relation kinds, `qualifier`, `type-ref` and
`import-target::pattern`.

**And the dogfood found a real clone the change created.** With `relation_kind`
grown to four arms, `ref_kind` beside it is a structural clone of it — the same
code with two type names swapped, once per enum, on both sides of the ABI.
`kndo_contract::variant_map!` is the promotion: the variants are the only fact,
the macro writes the rest, and both the SDK and the host use it. The previous
commit (78ea211) moved `GRAPH_SEMANTICS_VERSION` without naming the knob in
this file; naming it here is that entry's completion.

## 2026-09-08 — the manifest's shapes, all of them, and the ranges they make comparable

The manifest half of the contract-v3 audit, under the same standing
instruction as the evidence half: every shape the design details exists, and
where an adapter can already state a fact of that shape from what it ALREADY
reads, it states it. New PARSING waits for each adapter's own row; typing an
existing reading correctly does not.

**Types.**

- **`Version` / `VersionReq { spelled, range }`** — `DependencyDeclaration
  .version_req` is no longer a `SmolStr`. `range` is the half-open `[lo, hi)`
  the declaring adapter's ecosystem reads the text as, `None` wherever the
  adapter cannot map it; `disjoint` answers `None` for a comparison that could
  not be performed, because unknown is never a conflict.
- **`UnitRoot { path, recursive }`** — a non-recursive root is a real shape: a
  target that compiles ONE directory and leaves what nests under it to another
  unit. `ProjectUnit::depth_of` honours it, and a bare path is recursive,
  which is every build system's default and what every adapter meant before.
- **`UnitDep { unit, friend }`** replaces `depends_on` + `friend_of`. A
  dependency and a friendship are one fact the manifest states once, and two
  parallel lists could disagree; now they cannot.
- **`Unit.namespace_root`** — the name a unit's namespaces hang under when its
  ROOTS do not contain it. `Scopes::build` prefixes the segments a `ByPath`
  language derives.
- **`PackageEntry.aliases`** — the other names a package answers to. The graph
  indexes them beside the name (never over one), so `ResolveContext::package`
  finds an entry through a rename and only where nothing carries the name
  outright.
- **`PathAlias { prefix, targets }`**, **`ManifestEvidence.aliases`** and
  **`.ignores`**, with `ManifestSink::alias` and `::ignore`. An alias with no
  prefix or no target is dropped and described: resolution would read it as
  "this prefix resolves", and the honest answer is silence.

**Consumers.** `version-skew` compares ranges; the claim pass reads project
ignores; `Scopes` reads namespace roots; `package_map` reads aliases;
`ProjectUnit::depth_of` reads recursion. `ManifestEvidence.aliases` is the one
shape whose consumer is still owed — it is read through
`ResolveContext::project()`, which lands with the rest of that query surface,
and converting tsconfig's `paths` from today's `PackageEntry` emission before
the query exists would break resolution rather than move it.

**Adapters, from what they already read.** rust states its hyphen spelling as
a package alias and its crate name as the unit's namespace root, and normalizes
cargo's requirement to a range; ts normalizes npm's; go states the module path
as its namespace root; python states PEP 503's normal form and the underscore
spelling as distribution aliases, and reads setuptools' NAMED `package-dir`
key as a namespace root — under which the mapped root's own `__init__.py` is
the package door `import mypkg` executes. `kndo_toolkit::semver_range` is the
one reader for two ecosystems (`Bare::{Caret, Exact}` is the only difference);
it is the toolkit's because it is identical for a grammar it has never seen.

**Measurement.** ripgrep 151 → 141 and vite 687 → 685, all `version-skew`,
every removal a false positive the text comparison manufactured and every
survivor a range that genuinely cannot hold beside another (vite's
`tailwindcss` at `^4.3.3` beside `^3.4.19`). Decomposed in
`corpus-findings/COMPARISON.md`.

**Knobs.** `GRAPH_SEMANTICS_VERSION` 35 → 36: the same evidence now assembles
into a different graph (units carry roots that may be non-recursive, packages
carry aliases the map indexes, the claim pass drops what a manifest excludes).
The contract fingerprint did NOT move — manifest evidence is not cached by
file content, which is the whole reason it takes the other knob.
`abi/compat/*.wasm` are re-pinned against `kndo:vocab` with `unit-root`,
`unit-dep`, `version`, `version-req`, `path-alias`, and `manifest-evidence`'s
`aliases` and `ignores`.

## 2026-09-08 — three things a language says about itself, and the spec crosses whole

The spec half of the contract-v3 audit: `ecosystem`, `hidden_opt_in`, and the
`Nesting` enum the design writes, plus the wire gap those three exposed.

- **`ecosystem: Option<coordinate>`** — whose dependencies this language's BARE
  specifiers name. css and html declare `kndo:js-ts`: a stylesheet's
  `@import "tailwindcss"` and a page's bare `<script src>` name npm packages,
  and there is no registry of their own to judge them against. The dependency
  judgment reads it as the other half of `dependency_importers` — that one
  names suffixes a manifest's ecosystem MAY be imported from and casts doubt
  when nothing claims them; this one is the claiming extension saying out loud
  which ecosystem it speaks. The rule only ever ADDS users, so it can withdraw
  an accusation and never make one.
- **`hidden_opt_in: Vec<name>`** — dot-named directories discovery must ENTER,
  beside the ones a language's manifest and launcher globs already imply.
  js-ts declares `.storybook` and `.vitepress`.
- **`Nesting::{PerFile, Flat, ByDirectory, ByPath { roots }, Mounted}`** — the
  shape of the namespace forest, declared. `Scopes::build` dispatched on
  evidence PRESENCE before (a mount chain means Mounted, a path answer means
  ByPath, a clause means the fallback); now it dispatches on the capability,
  and `Flat` (the clause is the whole key — Java) and `ByDirectory` (the
  directory keys the clause — Go) are two answers rather than one accidental
  one. go, java, kotlin, rust and python each declare their own.

**The wire carried half a spec.** Adding these three meant looking at
`ExtensionSpecParts`, where six capabilities were filled with host-invented
defaults and three comments said so: a WASM guest could declare no nesting, no
file roles, no namespace span, and not one word of the dependency vocabulary.
`extension-spec` now carries all of it, and `ExtensionSpecParts::nesting` is a
field rather than a hard-coded `PerFile`. Old guests break on the added fields
— an ABI break, taken deliberately, with `abi/compat/*.wasm` re-pinned.

**Measurement.** Every repository byte-identical except vite: 685 → 690.
Declaring the four non-default nestings changed nothing anywhere — the
inference agreed with the declaration everywhere the corpus reaches, which is
the result, not the absence of one. `ecosystem` withdrew nothing because no
corpus stylesheet spells a bare specifier a manifest declares. The five are
vite's `.vitepress` docs site, entering the graph for the first time: two are
VitePress's own conventional entries and are owed to the `kndo:vitepress` pack
(M8.e), two are reachable only through `.vue` components no adapter claims —
the blind spot this corpus has recorded since M2 — and one rides the theme
entry's root. Decomposed in `corpus-findings/COMPARISON.md`; not discovering a
directory was never the same as judging it correctly.

`GRAPH_SEMANTICS_VERSION` 36 → 37: the same evidence assembles into a different
graph (the forest's shape is declared, discovery enters more, and a bare
specifier can name another ecosystem's declaration). The contract fingerprint
is unmoved — none of this is file evidence.

## 2026-09-08 — `cx.project()`: the five questions resolution asks

The last item of the contract-v3 audit. `ResolveContext::project()` returns a
`ProjectView` with the queries the design names — `unit_of`,
`source_roots_of`, `alias`, `namespace_of`, `files_in_namespace` — beside the
`package` lookup that already existed. The engine assembles the indices once
(`ProjectIndex::build`, from the manifest reads and every file's own namespace
clause) and both resolve sites hand the view to `resolve`: the full pass and
the surgical patch, which builds the same view off the graph it is amending.

The point is what it takes AWAY. An adapter resolving a specifier had two
choices before: re-derive a source root from a path convention, or re-parse
another ecosystem's alias table. Both are the wrong floor — the manifest is
the only thing that knows a source root, and the engine already read it. The
plan's deletions across the adapter rows (python's resolution by suffix and
`parent_dir`, css reading js-ts's `exports` conditions, ts's `paths`) all
resolve to this one door.

`alias(from, specifier)` carries the declaring manifest's directory with each
alias, so the LONGEST prefix wins and the NEAREST declaration breaks a tie: two
manifests spelling one prefix are two build configurations, and the inner one
is in force for the files under it. A specifier no alias names rewrites to
nothing rather than to itself — silence, not an identity that would look like
an answer.

`ProjectView` is data with borrows, not a callback: every query is a lookup
over what the engine assembled, so determinism is structural and an adapter
cannot ask the project a question the engine did not already answer.

The kmock proof is the whole path: a manifest declares `alias @app/ src/app`, a
nested one rewrites the same prefix elsewhere, and the mock's `resolve` reads
both through `cx.project()` — only the file no alias reaches is accused. The
contract's own tests pin the rest: the longest-prefix and nearest-manifest
rules, an empty target rewriting to the rest alone, and the empty answers a
file no manifest covers and a namespace nothing declares both get.

**Measurement.** Every repository byte-identical: the query surface is
additive, and until each adapter's row consumes it nothing resolves
differently. `GRAPH_SEMANTICS_VERSION` did not move, and neither did the
fingerprint — no evidence changed and no graph assembled differently.

## 2026-09-08 — what the design does not name does not exist

The owner's rule, after the fourth time an old mechanism decided a verdict
while its replacement sat beside it unused: **the design is the only thing that
exists.** Two ways to answer one question is not a safety net — whichever runs
first wins, and it is always the old one, because the old one is what
everything already calls.

A gate now carries it. `a_retired_mechanism_stays_retired` holds a table of
every name the design replaced beside what replaced it, and fails if any is
back in the tree. It is deliberately a TEXT scan and not a type check: the
point is that the name is gone, so a reader grepping for it finds nothing and
cannot reach for it — a type-level check would pass on a hand-rolled second
copy under the same name, which is exactly how the first one came back. Beside
it, one assertion of shape: `Extension` has the design's seven hooks and no
eighth, which is how the four manifest hooks stay dead without scanning for
three words as ordinary as `roots`, `packages` and `mentions`.

A row lands in the table the moment its replacement is PROVEN, never before:
the gate records what is already true so it can never quietly become false.
A row whose replacement is still owed belongs in its milestone.

**What the audit found, and it is the point.** Every name already retired is
genuinely gone from the tree — `Reach::Scoped`, `Reach::Private`,
`ImportShape::TypeOnly`, `sees`, `seen_from`, `scoped_regions`,
`narrowable_scopes`, `export_narrowing`, `root_for_attrs`, `generated_marked`
and the four manifest hooks. But `manifest_dependencies` was alive as ENGINE
VOCABULARY in four files: the hook was deleted and its name kept walking, so a
reader grepping it found something live. Renamed to `declared_dependencies` —
what it is. Two adapter tests carried the dead hooks in their own names and
were renamed; `project.rs` still documented a compatibility bridge that no
longer exists ("still read and merged" — false since M8.c); a doc-link in
`graph.rs` pointed at a trait method that is gone.

**And the number the audit produced, which is the real work:**

| adapter | dispatch rules | hand-written roots |
|---|---|---|
| rust | 5 | 1 |
| go | 4 | 1 |
| java | 3 | 1 |
| kotlin | 0 | 3 |
| swift | 0 | 4 |
| python | 0 | 5 |
| ts | 0 | 3 |

The design says `DispatchRule` replaces the root code of every adapter. Four
adapters declare not one rule and hold their roots in the extractor —
swift emits `@main` as a marker and then concludes the root itself from the
same attribute, two lines apart. That is the coexistence, exactly. Each row
retires its own, python first (its row is the open one), and each lands its
names in the gate's table as it closes.

## 2026-09-08 — python's roots become rules, and a gate holds the line

The first row of the cleanup, and the shape every other adapter's row now
follows.

Five branches in python's `extract.rs` concluded a root from evidence the same
function had just emitted. Six `DispatchRule`s in the spec replace them: the
`__main__` guard reported as the file marker its own source spells, a decorator
on a class/function/method at `Possible`, `test*` by name in a test compilation
(a `Trigger::Name` for free functions and a `Trigger::MemberOf` for members),
and a member dunder as `Effect::Witness` — the design's word for a promise its
owner made, alive while the type is and of no colour.

**Byte-identical: the whole corpus and every conformance fixture.** The rules
reproduce the hand-written roots exactly, so this changed the mechanism and not
one verdict — which is the only honest way to retire a mechanism that decides
things. The adapter's version moved 9 → 10 (the same source, different
evidence: a marker where a root used to be); neither the graph semantics nor
the fingerprint did.

`test_file` threaded through five functions to reach those branches; with
nothing concluding from it, four of those parameters had no reader and are
gone. That is the deletion the migration exposes, and the reason a replaced
mechanism has to actually be deleted rather than left beside its replacement:
the dead weight is invisible until the live path stops feeding it.

**The gate that keeps it.** `a_retired_mechanism_stays_retired` grew a second
assertion: no adapter concludes a root of its own, except the ones named in
`ROOTS_STILL_IN_THE_EXTRACTOR` with what each waits on. Six adapters are on
that list today; each leaves it when its rules land and cannot come back, and
an empty list is the milestone's finish line. Verified by regression: putting
python's `__main__` root back fails the gate at its file and line.

Two entries there are not waiting on a row and should be read as decisions:
ts's `#!` root is FILE CONTENT (the file says it is run), and go's
`package main` + `func main` is a unit fact go.mod cannot state — a Go module
is one Library unit, so no `in_unit` reaches it. Both stay until their row
gives them a shape; neither is a convention read from a path.

## 2026-09-08 — python's row closes: resolution against declared roots, the entry-point tables, and the P-ledger

Python's row of the design table is done. Everything in its deletion column is
gone, everything in its capability column is present, and the ten fixtures the
plan names exist with expectations that bite.

**Resolution stops asking the tree for a look-alike tail.** The old resolver
searched for a file whose path ENDS in the dotted name. It now asks
`cx.project()`: the unit compiling the importing file first, then every other
unit, stripping each one's `namespace_root` off the specifier before joining
the path; the project root answers last and always, because `sys.path` holds
the directory the interpreter starts in — which is why `from src.logic import
add` works from a test run under a `package-dir = {"" = "src"}` layout while
the installed distribution calls the same module `logic`. Both readings are
real; the declared roots answer first because that is the shape the package
ships as.

**flask 20 → 19, and the withdrawn finding is a false positive v1 also
shipped.** `src/flask/json/provider.py` writes `import json`, meaning the
standard library; suffix matching found `src/flask/json/__init__.py`, drew an
edge from flask's JSON package to itself, and reported the cycle it had just
invented. Import edges fell 226 → 197 on the same run: 28 more stdlib and
third-party names that had a look-alike tail inside the distribution. An edge
that should not exist is not free even where nothing accuses on it. Eight
repositories are byte-identical, which is the claim — the new resolver
reproduces every correct answer and drops one class of wrong ones.

**`[project.entry-points]` is read.** `[project.scripts]` was, its sibling
table was not, so a pytest plugin, a Flask command or a Django app registered
through the installer had no witness at all: the manifest is the only place
such a module is ever named. Every group is read now, because what registers a
callable is what calls it — the group decides who does the calling, never
whether anyone does. No corpus distribution registers one, so the number is
zero and `entry-points` is the proof.

**A whole-file root makes a file an entry surface, and that is why one fixture
had to be rebuilt.** `unittest-discovery` first claimed `setUp` as a known gap
owed to `kndo:pytest`. It was not: the file's Test unit roots it whole, so its
exported surface is the door the runner opens, and `setUp` needs no framework
knowledge. Ablating the `test*` member rule changed nothing — `describe` shows
`dispatch:test` sitting AHEAD of `entry-surface` on `test_add`, a second keeper
where the first already held. A fixture whose ablation is silent pins nothing,
so the fixture now turns on a helper module under `testpaths` that matches no
runner pattern: with the Test unit it is test code, without it the whole file
is accused as an orphan.

**The P-ledger.** Every python row of the nine-adapter audit, what closed it,
and what holds it shut:

| finding | mechanism | pinned by |
|---|---|---|
| P1 (resolution) | resolve against the unit's declared source roots, `namespace_root` stripped, project root last | `stdlib-shadow`, `src-layout-roots`; flask −1 `cyclic` |
| P2 (namespace packages) | no package initializer required, every declared root searched | `namespace-package` (PEP 420 across two roots) |
| P3 (defaults) | toolkit `Seats` — binder seats by field, not by parent kind | `default-values` |
| P4 (`_x`) | `Reach::Unit{0}` with an empty ladder, re-measured in the current context | `underscore-cross-module`, `underscore-namespace-access` |
| P5 (string annotations) | annotation strings emitted as references | `string-annotations` |
| P6 (cycles) | `Timing` — `TYPE_CHECKING` → Erased, in-function/`try` → Lazy; `cyclic` on Load alone | `type-checking-cycle` |
| P7 (manifests) | `toml` → `ManifestEvidence`: PEP 621, PEP 735, poetry/PDM/uv/hatch, setup.cfg, requirements with PEP 508/503 | `pyproject-tables`, `entry-points`, `manifest-dependency-skip` + the manifest unit tests |
| P8 (test roles) | `testpaths` → a declared Test unit, outranking the spec's `file_roles` | `unittest-discovery`, `test-convention` |
| P9 (grammar silences) | OPEN — the generated node-types inventory ledger is M8.f's | nothing yet; the only python row still owed |

P9 is the one that stays open, and it stays open honestly: no adapter has the
inventory ledger, it is a single generated test for all nine, and building it
one adapter at a time is how it ends up nine times.

**Deletions.** The private `parent_dir` and `GENERATED_NEEDLES` copies are gone
(`"Generated by"` moved into the toolkit's list, where the other eight readers
already were); `nearest_suffix_match` has no python caller left. The adapter's
version moved 10 → 11 — the same source, different manifest evidence and
different resolutions, which is one adapter's behaviour and therefore one knob.
Neither the graph semantics nor the fingerprint moved.

## 2026-09-08 — a JVM import names a package, and a package is a name inside a compilation

The owner's instruction, and it governs this entry: what the plan does not
detail is deleted, whatever still consumes it, and whatever breaks is the
reminder of what the plan still owes. `nearest_suffix_match` was the last
resolution-by-convention mechanism in the tree. It is gone from the toolkit,
and both its callers went with it.

**What replaced it is the plan's own word.** Kotlin's row says "resolución por
`files_in_namespace`"; java's deletion column names `package_dir_files` and
the layout mirrors; kotlin's adds the directory fallback. A Java or Kotlin
import names a PACKAGE, a package is a clause every file in it declares
(`Nesting::Flat`), and the import's binding picks the type among that
package's files. The mechanism sits in the toolkit as `resolve_in_namespace`
because it holds for a grammar it has never seen — nothing in it reads a path,
a suffix or a directory — and because two implementations of one question are
two answers to it.

**Then guava said the question was under-specified.** Answering with every
file that wrote a clause merged packages that never share a classpath:
`guava-gwt/src-super/com/google/common/base/super/.../Platform.java` declares
`com.google.common.base` and REPLACES the real class under the GWT compiler.
A namespace is a name inside a COMPILATION, so resolution filters the
namespace to the importer's own unit and the units it compiles against.
`Project::compiles_against` already held that closure, transitive and sorted;
`UnitView` now carries it rather than the contract re-deriving a graph walk.

**guava 8247 → 8271: 26 withdrawn, 50 new, everything else byte-identical.**
Eight `internal-only` findings moved from the GWT copy to the file that ships
— nearest-suffix had been attributing the use to whichever copy sat closer in
the directory tree. Thirty-nine of the new findings are `untested` on
`guava-gwt/src-super/` and `futures/failureaccess/`: the tests that appeared
to reach them were suffix matches, and an invented edge invents coverage as
readily as it invents accusations.

**The tests that died with the mechanism were the mechanism's own.** Four java
cases pinned the directory convention and the nearest-module tie-break; they
are replaced by cases stating the clause, including two the old resolver could
not have passed — a package no directory mirrors, and a Kotlin file whose
directory contradicts its package, which is ordinary Kotlin outside
`src/main/kotlin`. Both files are table-driven now, because the dogfood gate
caught the second one as a structural clone of the first and it was right.

`kndo-testkit` gains `resolve_in_namespaces`: a language whose imports name a
namespace cannot be tested by a file list alone, so the helper takes each
file's clause and builds the index the engine builds.

## 2026-09-08 — the rest of the sweep, and the debt it made visible

Continuing the owner's instruction to delete what the plan does not detail,
whatever consumes it: three more entries from the adapter table's deletion
columns.

**swift's private `GENERATED_NEEDLES`** — gone, moving nothing. Python's commit
had already promoted `"Generated by"` into the toolkit's list, so the copy was
identical to the thing it duplicated and had been for one commit.

**css's `unquote`** — gone. The plan says to read `string_content` from the
grammar; `tree-sitter-scss` 1.0 does not emit one, but it does spell the
delimiters as the value node's own first and last children, so the content is
the span between them. Same idea, against the grammar that exists rather than
the one the plan's sentence assumed, and exact where trimming characters was
lucky.

**js-ts's `split_bare`** — gone, and this one has a price. It cut a bare
specifier into name plus subpath and mirrored the subpath onto the package's
directory, which agrees with `package.json`'s `exports` only where no map
exists. A subpath is now an external specifier until M8.d reads the map:
unresolved, keep-alive, never an accusation.

**vite 690 → 701, and all 14 new findings are in `playground/`.** That is
vite's resolver test tree — small packages built to exercise the `browser`
field, `exports` and tsconfig paths — so every one is a subpath into a local
package whose map decides where it lands. They are false positives with a
named owner, which is what this repository asks a gap to be.

**The `deep-import` fixture came out the other way, and that is the argument.**
`@org/ui` publishes `{ ".": "./index.ts" }`. Its sibling imports
`@org/ui/secret`, a path the package refuses — Node raises
`ERR_PACKAGE_PATH_NOT_EXPORTED` on it — and the directory mirror had been
resolving it anyway, past the boundary the sibling explicitly drew. The file
behind it is now accused, correctly. One mechanism, two directions of error:
it invented reachability where a map said no, and it will be replaced by the
map itself rather than by a better guess.

The fixture records both readings rather than only the convenient one: today
the subpath is unresolved because EVERY subpath is, and the two reasons happen
to agree here. Where they would disagree — a package whose map does publish
the subpath — is M8.d's to make resolve.

**`a_retired_mechanism_stays_retired` grew three rows**, so
`nearest_suffix_match`, `package_dir_files` and `split_bare` cannot come back
under any spelling. A deletion that is not gated is a deletion that gets
rediscovered.

## 2026-09-08 — java and kotlin: four roots become rules, and `override` is a witness

Both rows leave `ROOTS_STILL_IN_THE_EXTRACTOR`. What is left on that list is
go's `func main` (a unit fact go.mod cannot state), rust, swift and ts.

**Kotlin's `override` and `operator` were roots and are now witnesses.** The
plan says so, and the reason is the vocabulary's own: a root is an ENTRY —
something outside comes in here — while an override says "this member is alive
while its type is", which is `Effect::Witness` exactly. The extractor now
reports both as MARKERS, because the design's word for a marker covers "an
attribute, annotation, decorator, **modifier** or directive over a
declaration", and two rules say what they mean.

**Exposed 966 → 965, and the one file is the argument.**
`EmailEnvironmentPostProcessor.kt` overrides Spring's
`EnvironmentPostProcessor`, and that override was the only thing anchoring the
file. Two findings became one, moved from the member to the file: nothing in
the project reaches this class, which is the truer sentence about it. It stays
a false positive until `kndo:spring` reads `META-INF/spring.factories` (M8.e),
and it is now one instead of two.

**Java's `main` is a marker plus a rule, guava byte-identical.** Four facts
make the JLS launcher — the name, `static`, `public`, `void`, `(String[])` —
and no trigger spells a modifier or a signature. So the adapter recognizes the
SHAPE, which is grammar knowledge and belongs there, and reports it as the
marker `main(String[])`; the rule says it is a production root, Certain,
because the shape is matched whole. The same split python's `__main__` guard
takes, and the reason a signature coordinate was not added to `Trigger`: the
fact is the adapter's, the meaning is the rule's, and the existing vocabulary
already carries both.

**`com.vendor.Closer` is out of java's shipped rule table.** It was a
fabricated vendor type living in the language's own witness list so that one
fixture could prove qualification distinguishes two same-simple-name types.
The contract already proves that where the trigger lives — `spells` against
`com.vendor.Closer` and `com.other.Closer` in `extension.rs`'s own test — so
this was a second proof paid for with invented data shipped to users. Gone,
with the two fixture classes and the two interfaces they implemented —
`kndo-adapter-java/runtime-required-members` loses four files and two claims,
keeping the two that turn on a base rather than on a name.

Test data in a production table is the same defect as a convention in an
extractor: something that decides real answers, put there to make a test pass.

**A correction the gate extracted.** `contract_changes_are_loud` refused this
commit until `kndo-adapter-ts/tsconfig-path-aliases` was named: the `split_bare`
commit moved it alongside `deep-import` and only `deep-import` was written
down. Same cause — an alias whose target is a subpath of a bare specifier no
longer resolves through the directory mirror. Naming it here is the entry that
commit owed.

`kndo:java` 19 → 20 and `kndo:kotlin` 14 → 15: the same source, different
evidence (a marker where a root used to be, and modifiers reported at all).
Neither the graph semantics nor the fingerprint moved.

## 2026-09-08 — swift: a framework's dispatch is not the language's, and the bill for saying so

Swift's row leaves `ROOTS_STILL_IN_THE_EXTRACTOR`. Four hand-written roots are
gone, and the corpus paid **+1553 findings** for it: Alamofire 446 → 1448,
vapor 178 → 729. Every other repository is byte-identical.

**1505 of those 1553 are XCTest methods inside `Tests/` trees.** The adapter
rooted `test*` in any `Tests/` target by hand, `Certain`. The design's own
sentence decides where that belongs: "the stdlib packs of each language are not
packs — they are the adapter's `dispatch_rules`, because they are facts about
the LANGUAGE." XCTest is not Swift. It is a library you import, a package that
does not import it is not collected by it, and its collection rule is
`kndo:xctest`'s, gated by the dependency that proves it is there. Same for
`@Test` and `kndo:swift-testing`.

So the number is not a regression discovered; it is a debt that was being paid
by a convention in an extractor and is now on the books with its owner named.
`xctest-discovery` pins it as two `[[known_gap]]` entries plus a control that
must stay dead once the pack lands — it fails the day the pack closes it.

**The blanket conformer keep is the deletion worth arguing for on its own.**
Every non-private method of a type declaring ANY conformance carried a
`Possible` root: no protocol named, no requirement named. That is not a rule,
it is a silence with a confidence attached, and the vocabulary already has the
honest version — `Trigger::ExternalWitness` has to name the base and its
members. `protocol-requirement-reach` had written this down as a
`[[known_gap]]` on `Cube.volume`; the gap closed the moment the keep went, and
the entry is promoted here.

**`@main` and `override` moved without moving anything.** `@main` is Swift's
own entry attribute, so it stays an adapter rule — a `Trigger::Marker` on a
type, production, Certain. `override` is a witness: invoked through the
superclass, alive while its type is, of no colour. The extractor already
reported both as markers; only the roots beside them were the problem, which is
the whole shape of this milestone in one adapter.

**What the deletion exposed.** `FileCx` and its `in_test_target` field,
`owner_conforms` threaded through four functions, `scoped_or_wider`,
`has_modifier`, `has_attribute`, `has_conformances` — all had exactly one
reader, and it was a root. The conformance FACT survives where it belongs: the
relation stream, which a rule reads.

**The apple-bundles proof changed shape and got more honest.** Without the
plugins the watchOS files are accused WHOLE — nothing reaches them once the
conformer keep is gone. With the plugins the storyboard and plist roots land,
the files become reachable, and the members that were dead all along behind the
file-level finding surface. The invariant "a root can only keep something
alive, never accuse" is restated at the level where it is true: nothing appears
whose own file was not already accused whole.

`kndo:swift` 12 → 13. Neither the graph semantics nor the fingerprint moved.

## 2026-09-08 — rust and ts empty the list, and one gap gets a name

`ROOTS_STILL_IN_THE_EXTRACTOR` holds one row: go's `package main` + `func main`,
a UNIT fact go.mod cannot state. Every other adapter's roots are rules.

**ts: `#!` is a marker plus a rule.** The one thing here no path convention can
say is that the file says the loader runs it — content, not a path — so it
stays the adapter's, as the marker `#!` and one production rule, Certain.
Nothing moved.

**rust: a macro template's names are references, and that costs two findings.**
A name a `macro_rules!` body mentions was rooted `Possible`. The plan's shape
for names inside macro token trees is a reference, and a reference is what the
file can honestly say: the name appears here. ripgrep 141 → 143, references
61733 → 61751, everything else byte-identical.

**The two are `internal-only`, and they name a coordinate the design lacks.**
The root was doing a second job by accident: suppressing narrowing advice. A
`macro_rules!` body resolves at every EXPANSION site, so `crate::messages::
set_flag` must stay `pub(crate)` however local its mention looks; the reference
is recorded where the template is written, so the ladder pools one use in one
file and advises a rung the macro cannot live at. Both fixtures now carry it as
a `[[known_gap]]` whose `fix` names what is missing: **a reference that
travels** — a use recorded in one file and performed in another, which this
vocabulary cannot express and `macro_rules!` is the case that needs it.

Recording it is the point. A root that suppresses advice by being a root is not
a rule about macros, it is a rule about entry points borrowed for a job it does
not describe; the gap was invisible while the borrowing worked.

`kndo:rust` 15 → 16, `kndo:js-ts` 13 → 14. Neither the graph semantics nor the
fingerprint moved.

## 2026-09-08 — the sweep audited against its own list, and what it did not clean

The owner asked whether anything legacy survives. Audited against the design's
deletion columns, name by name, rather than answered from memory. Twenty of
twenty-two named mechanisms are gone; the ledger of what is NOT:

**A duplicate this sweep itself introduced, now collapsed.** Making a namespace
a name inside a compilation put `compiles_against.binary_search` inside
`ProjectView::files_in_namespace` — beside `Project::sees_into`, which had asked
the same question since M8.b. Two crates carrying one predicate is the case this
repository's own rule sends to `kndo-contract`, and it now lives there as
`unit_sees(closure, viewer, target)` with both callers reading it. Naming it
also names what it is: a unit sees itself and everything it compiles against,
and the closure is sorted so the test is a search.

**`("sees", None)` survives as a query ANSWER**, in
`kndo-core/src/query.rs`. The hook is gone — `a_retired_mechanism_stays_retired`
pins `fn sees(`, `sees_of`, `seen_by` and `seen_from` dead — but the gate scans
function names, and this is a string literal in the generated query contract:
user-facing output, so renaming it is a contract change and not a cleanup. It
belongs to the naming decision below, not to this sweep.

**The naming toll is still unpaid, and it is now four words.** `Extension`
(407 mentions), `Conduct*` (96), `plugins` (53, including the report envelope's
own key) and `rule pack` (5) orbit one concept: something that is not a language
adapter and contributes rules or findings. The design's word is *pack*. Three of
the four are internal and mechanical; the fourth is a published schema key, so
the change is a contract change with a fingerprint move behind it. This is the
owner's word to choose and has been outstanding since M6.a.

**What is deliberately not deleted**, each with its sentence:

| kept | why |
|---|---|
| `Project::sees_into` | the engine's caller of the one predicate, not a second copy |
| `split_attribute_text` (rust) | parses inside `unsafe(…)`, which the grammar does not split — grammar knowledge at the adapter floor |
| swift's `ladder(&[Step…])` | declarative data, which is what replaced the ladder-by-text |
| one `specifier_shaped`, one `GENERATED_NEEDLES` | the design deletes the DUPLICATE; one copy is the survivor |
| go's `func main` root | a UNIT fact `go.mod` cannot state — the last row of `ROOTS_STILL_IN_THE_EXTRACTOR`, named rather than pretended away |

**And 28 known gaps on the books**, each naming its owner: 5 for `kndo:xctest`
and `kndo:junit`, 2 for `kndo:swiftui`, 8 for M8.c/M8.d rows, 2 for the pinned
Kotlin grammar, 2 for the reference that travels, and the rest for engine work
with its milestone written down. A gap with an owner is a plan; a gap without
one is a bug nobody filed.

Also names `kndo-adapter-rust/macro-template-names` and
`kndo-adapter-rust/macro-use-mod`, which the previous commit moved and described
without spelling. Second time the gate has extracted a fixture name from me in
this sweep, which is the gate working.

## 2026-09-08 — one word: plugin

Owner decision, and it reverses mine. On 2026-08-31 I renamed the `Plugin*`
cluster to `Conduct*` under the reason "the conduct cluster stops speaking v1's
plugin". That reason does not survive this repository's own law: the quarry rule
is about not inheriting v1's JUDGMENTS, while its shipped surface — verbs,
formats, words — is inherited value that is either kept, carried with a
disposition, or dead with its vice named. Nobody ever named a vice for the word.
It was dropped for being v1's, which is the one thing the law forbids.

**Why `plugin` and not `extension`.** `extension` has a permanent collision in
this domain: a tool that reads files talks about FILE extensions constantly, and
that same 2026-08-31 commit already paid one rename for it —
`PluginSpec::extensions` → `suffixes`, with the reason written down as
"'extension' already means the species". A word that costs a rename the first
time it meets its own domain is the wrong word. `plugin` collides with nothing
here, and the report envelope's key has said `plugins` all along: the published
noun never moved, only the internals drifted off it.

`pack` fails the owner's test — one word for anything you can build. "A Rust
language pack" reads wrong where "a Rust plugin" does not.

**What moved.** The WIT world `extension` → `plugin` (file `wit/plugin.wit`);
`ExtensionSpec`/`Builder`/`Parts` → `PluginSpec*`; the trait `Extension` →
`Plugin`; `ConductTarget`/`ConductSeverity`/`ConductSink`/`ConductBuilder`/
`ConductHook` → `Plugin*`; `MockExtension` → `MockPlugin`; `WasmExtension` →
`WasmPlugin`; `Category::is_extension` → `is_plugin`; modules
`kndo-contract/src/extension.rs`, `kndo-core/src/conduct.rs` and
`kndo-host-wasm/src/extension.rs` all become `plugin.rs`; crate `kndo-packs` →
`kndo-plugins`; `docs/src/extensions.md` → `plugins.md`; gate
`builtin_conduct_proofs` → `builtin_plugin_proofs`. And the category namespace
`ext:<coordinate>/<rule>` → `plugin:<coordinate>/<rule>`, which is FINDING
IDENTITY and the reason this rides one commit.

**What deliberately did not move, with its reason:**

- **`Phase::Conduct`** and `declares_conduct`/`conducts`. A phase is not a
  species: extraction is done by plugins too, so "the plugin phase" would name
  nothing. The conduct round is the round where the assembled graph exists, and
  that is a fact about TIME, not about who is speaking.
- **`ExtensionDelegate`, `WatchKit Extension`.** Xcode's own names inside the
  apple-bundles fixture. A blanket rename caught three of them and the
  `builtin_plugin_proofs` gate refused the commit until they were put back,
  which is the fixture discipline doing exactly its job.
- **DECISIONS.md, EXPERIMENTS.md, COMPARISON.md.** The rename pass rewrote them
  and the edit was reverted: an append-only record says what happened in the
  words used then, and "`PluginSeverity` → `PluginSeverity`" is not a history,
  it is a lie about one. Living documents (README, CONTEXT, CLAUDE, the docs
  site) carry the new name; the ledgers keep theirs.

**Measured: the corpus is byte-identical, all nine repositories.** The
fingerprint did not move either — it is structural, and renaming a type changes
no shape. What moved is the ABI (`cargo xtask pin-abi` re-pinned all four compat
guests) and the plugin-contributed categories in the conformance fixtures.

The cost was symmetric — whichever word won, exactly one of (ABI, schema key)
had to break. The ABI is pre-1.0 with a compat matrix built to make this
visible; the schema key was already `plugins`.

## 2026-09-09 — a keeper names the rule that made it, and 44 rules name no fixture

The first of the plan's seven extensions ("Las formas que faltan", the artifact
that captured what the plan did not design). `Derivation`'s load-bearing half:
a derived root, exemption or witness now carries **which rule derived it**.

`kndo_contract::plugin::RuleId { plugin, index }` — the coordinate of the spec
that DECLARES a rule, and its position in that spec's own list, never in the
combined list a language is dispatched with. So a pack's rule is `kndo:xctest#0`
whichever language it rides into. `kndo-core::dispatch` grew `Rules` (the
attributed list), `DerivedRoot`, `Exemption` and `Witnessed`; `GraphFile` holds
those instead of bare `Root`/`u32`/`(u32, SmolStr)`; `Keeper::Dispatch`,
`Keeper::Exempt` and `Keeper::Witness` carry the `RuleId`.

**One spelling, three consumers.** `kndo_core::query::ground` is the single
render of "why is this alive", and `EdgeRef.kind` IS that string: `kndo
describe` prints it, `kndo used-by` lists it, and a fixture's new
`because = "..."` pins it. The old `dispatch:test` / `exempt` spellings are
gone — "dispatch" was never the interesting half of that answer. A rule-derived
ground reads `rule:kndo:python#6`; a witness a rule named reads
`witness:Comparable:rule:kndo:java#4`; a witness the graph's own relations
resolved carries no rule and reads `witness:Comparable`, because there is no
rule to ablate.

**The fixture vocabulary gained `because`, and the testkit gained a seam.**
`Expectations::check` took two closures' worth of questions and now takes one
`Tree` — `names_something` and `grounds`, the same two questions the query
verbs answer. `Violation::BecauseAbsent` is the new failure. Pinned in this
commit: `unittest-discovery` (`rule:kndo:python#6`), `dunder-and-decorated`
(`witness:*:rule:kndo:python#7`, `rule:kndo:python#3`),
`runtime-required-members` (`witness:Comparable:rule:kndo:java#4`,
`witness:Serializable:rule:kndo:java#13` twice), `dispatch-and-cross-package`
in java and kotlin, `attribute-dispatch` (`rule:kndo:rust#7`, `#17`, `#21`),
`main-in-every-target` (`#25`, `#26`), `global-allocator` (`#9`),
`test-file-init` (`rule:kndo:go#1`), `apple-bundles` (`rule:kndo:swift#1`).

**Measured, and it is the point of the whole thing: 73 of 100 dispatch rules
fire in NO fixture.** 29 of those are `kndo:spring`'s, which activation gates
and `builtin_plugin_proofs` covers separately. The remaining **44 belong to
claiming adapters and are exercised by nothing** — `#[bench]`, `#[proc_macro]`,
`allow(unused)`, go's `Benchmark*`/`Example*`/`Fuzz*`, java's `Runnable`,
`Callable`, `AutoCloseable` witnesses, every adapter's `generated` marker. A
rule no tree fires is a claim about a language this repo has never watched hold:
it can be wrong, or deleted, and the run is byte-identical either way. That
silence is exactly what `RuleId` exists to break, so the 44 are enumerated in
`RULES_WITHOUT_A_FIXTURE` and the gate `every_dispatch_rule_fires_in_some_fixture`
holds the list from three sides — a new cold rule fails, a row that warmed up
fails, and a row naming a rule no extension declares fails. The list only
shrinks; each row is a fixture owed.

**The ablation was verified, not asserted.** Deleting python's rule 6 (the
`test*`-member rule) from a file copy turns `unittest-discovery` red with
"`tests/test_calc.py#CalcTest.test_add` claims `rule:kndo:python#6` keeps it —
the run derived [entry-surface]". That is the same diagnosis I previously had to
reach by hand, three separate times, on a run that came back byte-identical.
The same ablation also reddened `dunder-and-decorated`, whose pin named rule 7:
positions renumber, and the ledger is deliberately loud about it.

`GRAPH_SEMANTICS_VERSION` 37 → 38: `GraphFile`'s dispatch fields changed shape,
so a cached graph from before this commit must not be read back. The contract
fingerprint did NOT move — `RuleId` is the engine's attribution, not evidence an
adapter writes. No `expected.json` moved: the findings are identical, and only
their explanation gained a name.

**Retired in the same commit:** the second dispatch pass in
`graph::dispatch_files` that re-ran every pack's rules over every file to count
`pack_roots`. The attribution answers it directly — a derived root credited to a
pack's coordinate is that pack's claim — and the language-first ordering means a
root the language would have derived anyway is credited to the language, which
is what the second pass was approximating.

## 2026-09-09 — the two deletions the extension proposed, both refuted by ablation

"Las formas que faltan" proposed removing two capabilities as my own inventions,
one outright and one as a hypothesis to verify. Both were verified. Both stay,
and the numbers are here so nobody rebuilds the argument from scratch.

**`PublishedSurface` — kept. The claimed derivation does not exist.** The
proposal was that a unit's published surface is the closure of its entries, so
the enum is redundant. It is not: swift is `Nesting::PerFile` and hands out
every `public` declaration of the module with no entry file anywhere, while
js-ts is `PerFile` and hands out only what `main`/`exports` reach — same
nesting, opposite surface. The other candidate derivation, "does the unit
declare entries", fails the same way: rust and python declare entries and hand
out everything. The fact is irreducible and the question is one sentence, now in
the doc-comment: does an outside consumer's import of this unit name a FILE the
manifest declared, or the unit itself?

Ablated (every unit `Exports`, on a file copy): **vite 701 → 644, −57**, and
every other repo byte-identical. Those 57 are the `internal-only` advice on
exports no npm entry hands out — the same family the export-narrowing
experiment measured at 51 before the ladder landed.

**`namespace_span` — kept. `Attachment` answers a different question.** The
proposal was that the plan's glossary already covers it: "a `_test.go` with
`package x` belongs to x only in test builds". That is attachment, and it is
about which BUILD a file's membership holds in, inside ONE unit. `namespace_span`
is about whether two UNITS spelling one name hold one node — guava's
`guava-tests` naming `guava`'s package-private members across the classpath.
Neither implies the other.

Ablated (every namespace `Compilation`, on a file copy): **−401 findings —
vite 701 → 319, lodash 20 → 2, flask 19 → 18.** Merging namespace nodes by name
alone makes co-visible what no compiler ever compiled together, and the
accusations dissolve into a co-visibility that does not exist. The default
(`Unit`) is carrying 401 findings.

**What landed instead of the deletions.** Each capability now carries its
ablation number in its doc-comment, so the next reader sees the cost before
proposing the cut. `namespace_span` already had its two-sided conformance case
(`a_namespace_spans_the_unit_compiled_against_it_when_the_language_says_so`:
same project, capability flipped, opposite verdict). `PublishedSurface` did not,
and now does — `what_a_published_unit_hands_out_is_the_ecosystems_rule` runs one
project through both values and asserts the jar hands out `helper` and the npm
package does not.

**The precedent this sets for the extension itself.** The artifact put its
tiers on the table as an instrument of trust, and Tier B ("borrar — ley del
repo") turned out to be the weakest of the three: retiring a capability is the
repo's stated ideal, which made two untested derivations feel like law. The
ideal is real, but it is a reason to TRY the derivation, never to assume it
holds. The remaining five proposals get the same treatment — the number decides,
in whichever direction it points.

## 2026-09-09 — an alias is a pattern with conditions, and npm's two tables are read apart

The third of the plan's extensions, and the one the plan's own js-ts row asked
for in prose: "`exports`/`imports` with conditions (real subpath and
self-reference resolution)". Nothing could express it, because `PathAlias` was
`{ prefix, targets: Vec<SmolStr> }` — no capture, no template, no conditions,
no way to say "refused".

**The shape.** `PathAlias { pattern, targets: Vec<AliasTarget> }` with
`AliasTarget { template, conditions }`. The pattern carries at most one `*` and
it CAPTURES; a pattern without one is a prefix and everything past it is the
capture, which is how the old directory form keeps working unchanged. The
template's `*` receives the capture, and a template with none takes it appended.
An EMPTY template is a deliberate dead end — npm's `null` subpath — because "the
manifest refused this specifier" is a different fact from "no alias named it":
the first stops the search, the second falls through. `PathAlias::rewrite` is
the one matcher, and `Project::alias` calls it rather than carrying a second.

**Conditions do not filter, and that is the design, not a shortcut.** The engine
cannot know a runtime, so every condition's target is a possible resolution and
all of them are offered in declaration order — the keep-alive direction, stated
once instead of guessed per call site. What conditions buy is that the caller
can SEE which branch it took: `Project::alias_targets` hands back the conditions
and the refusals, and a target reached only under `types` is a declaration file
the runtime never loads. The consumer that reads them is owed; the fact is
carried now because the alternative is flattening the map and losing it.

**Two tables, read apart — which is where the old code was wrong.**
`package.json`'s `exports` is what a CONSUMER may name, so it lands on
`PackageEntry.subpaths` with its keys spelled as one writes them (`.` is the
package name, `./client` is `<name>/client`) and applies to whoever names the
package, from anywhere. `imports` is the package talking to ITSELF, so it lands
on the manifest's own dir-scoped alias table and only files under that manifest
may spell `#shapes/payload`. Before this commit both were flattened into one bag
of ENTRY strings: every `#` target was rooted as a published entry, which rooted
every internal type a package happens to alias. `entry_fields` now keeps
`exports` (the published surface really is entries) and drops `imports`.

**Measured on vite.** Unresolved `#` imports **91 → 8** (the 8 left are
playground specifiers a vite config aliases, not `package.json`); scoped
subpaths **81 → 63**; bare subpaths **461 → 459**. Import edges **2,447 →
2,549, +102**. Findings: **byte-identical, all nine repos** — the 102 new edges
reach files that were already alive by a relative import, so the graph gained
edges and the verdicts did not move. That is the honest result and the reason to
report the edge count rather than a finding count: `uses`, `used-by`, `trace`
and `impact` answer differently now, and a future analysis over subpath cycles
has something to run on.

**The conformance case, and what it caught.** `conditional-subpaths` is a
workspace of two packages: a conditional `.` (both `import` and `require`
branches alive), a `./client` the directory layout does not mirror, a `./tools/*`
capture, a `null`-refused `./internal/*`, and a `#shapes/*` internal table.
Ablating the subpath map turns it red — but only through `because`: with the map
gone, `connect` and `probe` are still kept, by `entry-surface`, because an
`exports` target is an entry either way. An `[[alive]]` claim alone would have
passed. That is the previous tranche's `Derivation` earning its keep two days
later, on the first fixture written after it.

`kndo:js-ts` 14 → 15. The ABI moved (`path-alias` gained `alias-target`,
`package-entry` gained `subpaths`) and all four compat guests were re-pinned.
The contract fingerprint did not move — it is structural over the evidence
types, and an alias table is manifest evidence the engine assembles, not
evidence an extraction writes.

## 2026-09-09 — the two capabilities go to the floor a manifest can vary

Yesterday's entry kept `PublishedSurface` and `namespace_span` because ablating
each cost findings. That defence was wrong in the way the owner named: a number
proves a capability is doing work, never that it has the right shape. Both were
stickers — facts about the PROJECT (floor 3) bolted onto the LANGUAGE (floor 2)
— and the proof is in the code they patched.

**`namespace_span` patched a key the engine had already built.** `Scopes::build`
computes `(Compilation, segments)` for every file and every `Nesting` arm
produces both halves — `Nesting` already IS the key-shape type. Then
`span_nodes`/`cobuilt_nodes` threw the `Compilation` half away and re-grouped by
`segments` alone, and `namespace_span` existed to gate that re-derivation off
where it would be wrong. Not even a key fact in the end: a per-file boolean
choosing between THREE precomputed lists at every pool lookup.

**`PublishedSurface` said a second time, per language, what `Publication` says
per unit** — and the two could disagree into a state with no meaning: a manifest
saying `Published` under an adapter saying `Entries` published nothing at all.

**What replaces them, in the plan's own vocabulary.**

`UnitDep { unit, grants: Grant }`. The relation "unit A's files may name unit
B's namespace-private names" is a fact about a dependency EDGE, and `UnitDep` is
the type the plan already gives that edge. `friend: bool` — the same fact one
rung up — dissolves into it. `Grant` is `Exports | Namespace | Unit`, each state
implying the one before, because a build system that lets you INSIDE a unit has
already put you on its classpath.

`Grant` is deliberately NOT `Rung`, and the first attempt at this reused `Rung`
and was caught by the corpus: a rung says how far a declaration reaches OUT and
a grant says how far a dependent reaches IN, so the two orders run opposite ways.
Under `Rung`, granting a JVM dependency the namespace rung also granted it
Kotlin's `internal` — two findings on Exposed, and the reason a separate type
exists rather than a reused one.

`Publication { Unstated | Unpublished | ByName | ByEntry }`. `Published` was
under-specified exactly as `Nesting::Flat` was: it said a unit publishes without
saying how its consumers ADDRESS what they name. `ByName` is a jar, a Go module,
a Python distribution, a Rust crate — a consumer writes
`com.google.common.io.Files` and every export of every file is on the surface.
`ByEntry` is npm — a consumer writes a specifier the manifest maps to a FILE, so
an export no entry hands out is internal. `Unit::publishes_every_export` is the
one predicate, and `internal_only`'s two-clause disjunction became that one call.

**`Unstated` resolves to `ByName`, and that fixed a bias the flag had backwards.**
A silent manifest lands on the WIDER surface, because a wider surface accuses
less. Before, a js-ts unit the manifest reader failed to classify still got
`Entries` from the language flag — the ACCUSING direction. Absent evidence now
degrades toward silence, as the plan requires of every other absence.

**`Nesting::Flat` → `Nesting::ByUnit`.** Its doc said "the CLAUSE is the whole
key" and its arm produced `Compilation::Unit(u)`. It lied for the same reason
`Publication::Published` did — a variant that names a shape without naming what
else is in it.

**Measured. 131 conformance fixtures: findings byte-identical. Corpus: eight of
nine repos byte-identical.** The one row that moved is not this change's, and
that needs saying plainly.

**A correction to the entry before last.** The alias tranche reported "findings
byte-identical, all nine repos". That corpus run was taken BEFORE its final edit
— dropping `imports` from `entry_fields`, made while fixing the fixture — and I
committed the earlier number. The true figure for that tranche is **vite 701 →
712**, and this run is the first that shows it. Verified by ablation on a file
copy: restoring `imports` to `entry_fields` puts vite back at 701 exactly, and
nothing else moves.

Of those 12, one was a defect of that tranche and is fixed here: `landing()` took
the FIRST candidate of a rewriting, so `"#flag": { "module-sync": …, "default": …
}` kept whichever branch the manifest's key order put first — `serde_json` sorts
keys, so `default` won and `misc/true.js` died. The engine cannot know a runtime,
which is exactly why every branch is an edge; it now answers `Resolution::Files`
with all of them, and vite is 712.

The remaining **11 are true consequences of the entries/aliases split**, and they
decompose: `misc/true.d.ts` and `misc/false.d.ts` (a `.js` specifier resolves to
the implementation, and its declaration file is reached only by TypeScript's own
type resolution), `src/types/shims.d.ts` and `chokidar.d.ts#AwaitWriteFinishOptions`
(ambient declarations a `tsconfig` `include` picks up, which no `package.json`
states), and seven playground files behind the eight `#` specifiers a vite config
aliases rather than a manifest. All three families are tsconfig- and
config-shaped, which is M8.d's territory and not this one's; they go on the books
there rather than being chased here.

**Retired with them:** `Project::sees_into` (replaced by `granted`/`grants`),
`ProjectUnit::friend_of` (the grant carries it), `Scopes::spans` and
`Scopes::files` — three file lists per namespace node and a per-file toggle
became two lists and no toggle, because the data now says what the flag said.
`PluginRun.published_surface` leaves the report envelope: it answered per
language a question that is per unit, and a per-language answer to a per-unit
question is a wrong answer, not a partial one.

`CLAUDE.md` gains the test both capabilities failed: a capability a manifest
could state per unit or per edge is not the language's. Having a default, a named
consumer and a conformance case does not make a fact belong to the floor it sits
on — all three were satisfied, and both were still on the wrong floor.

## 2026-09-09 — M8.d: a manifest reader is graded by the build tool that owns the manifest

**The circle transcripts break.** Every manifest test in this tree was one hand
grading another: a manifest string I wrote and the answer I expected, authored
in the same sitting from the same reading of the same spec. Nothing that is not
us ever got to say what a manifest means. `kndo-testkit`'s `transcript` module
closes that: a `ToolTranscript` is one build tool's answers about one fixture,
captured from the tool by `cargo xtask capture` with the command and version
that produced them, replayed by the `captured_transcripts_hold` gate with no
toolchain present. A fixture now carries two claims side by side —
`expectations.toml` says what the RUN must report, `transcript.json` says what
the BUILD says the tree is.

**The shape.** `ToolClaim` is six variants, each a question `ManifestEvidence`
or `resolve` also answers, each keyed by a path or by a manifest's own spelling
— the coordinates two independent derivations of one tree cannot disagree about
by accident. `Compiles { file, kind }` carries a `RootKind` and not a
`UnitKind`, deliberately: a build's target granularity is its own (go links an
executable per `main` PACKAGE where its unit is the module), and demanding the
two agree on a target would grade our model of go rather than our reading of
it. `Enters { file, kind }` keeps `UnitKind`, because an entry names a target
and nothing else does. A unit's NAME is graded by nothing — cargo calls a bench
`throughput` where the engine calls it `bench:throughput`. `Reading` is
`Whole | Sampled`, stated once per transcript because one command is one
reading; a tool that answers two ways gets two transcripts.

**Five tools answer, and two ecosystems get a row instead.** cargo
(`cargo metadata`, whole), go (`go list -e -json ./...`, sampled), maven
(`help:evaluate` on the effective model, whole), node
(`createRequire().resolve`, sampled) and `packaging` (PEP 508, whole) are
captured. `ECOSYSTEMS_WITH_NO_TRANSCRIPT` carries the other two with the reason
each cannot answer here, and the gate fails if a row names an ecosystem that no
longer reads a manifest: swift ships no toolchain where transcripts are taken
and `Package.swift` is a program only swiftpm evaluates; kotlin's Gradle script
resolves its plugins from the network (measured: an offline `gradle -q projects`
on the `gradle-multi-module` fixture stops at `plugins { kotlin("jvm") }`) and
its Maven half answers `src/main/java` for a Kotlin tree until the toolkit reads
the `<sourceDirectory>` a Kotlin pom declares.

**go's `./...` is a SAMPLED reading, re-measured.** Against go 1.24.7 in a
scratch module: `go build ./...` succeeds and `go run .` prints from both when
`main.go` imports `example.com/sem/testdata/gen` and `example.com/sem/_scratch`,
while `go list ./...` lists only `example.com/sem`. The pattern reaches less
than the compiler compiles, so what the command omits is not a claim that go
skips it — the same measurement M8.b.12 made, taken again because the label
depended on it.

**What it caught, first run.** Two defects in `kndo:python`'s manifest reader,
both silent because the dependency family abstains for python
(`DependencyIdentity::Underivable`), so no finding moved and no test failed:
`[project] dependencies` declared `scope: None` — "the source could not
classify it" — for the one table PEP 621 defines as what an install pulls in;
and `[project.optional-dependencies]` declared `Dev`, when an extra is a feature
the CONSUMER opens (`pip install pkg[postgres]`), which is the fact cargo states
with `optional = true`. Now `Prod` and `Optional`; `[dependency-groups]` and
poetry groups stay `Dev`. The setup.cfg half said the same two things and gained
a third defect in the same edit — with `[options]` scoped, its
`is_requires` test admitted every key of the section, and `packages = find:`
became a dependency named `find`; the key decides now, not the value.
`kndo:python` moves 11 → 12. Nine repositories byte-identical.

**The ad-hoc capture is retired into the mechanism.** `tests/captured/tooling.json`
and `tests/tooling.rs` were the only real-producer grading in the tree, and a
sticker: an ad-hoc JSON shape, an ad-hoc reader in one test file, and a
"recapture with `scratchpad/capture-py.py`" pointing at a file that does not
exist. Its fourteen PEP 508 spellings are now a fixture's `[project]
dependencies` (`pep508-spellings`), and `packaging` — the parser pip and
setuptools both call — answers them through `xtask capture`. Not the wheel
metadata a build backend writes, deliberately: that file RENDERS a name
(`A.B_c-D` becomes `A.B-c-D`), and a rendering is a third spelling neither the
manifest nor the reader uses. `canonicalize` goes with it: PEP 503 folding had
exactly one caller, its own test, and the comparison it exists for does not
happen while python's specifier identity is underivable — it returns the day
that does, with its consumer.

**Eight of fourteen fixture poms were not poms.** Measured with maven 3.9.11
offline: `dead-code-same-package`, `dispatch-and-cross-package`,
`multi-release-variants`, `nested-type-qualifier`,
`package-private-across-modules`, `visibility-ladder-and-nested-members` (java)
and three kotlin siblings were REFUSED by maven — no `<modelVersion>`, no
`<groupId>`, no `<version>`, or a dependency with no version. A manifest fixture
the real tool refuses to read proves nothing about the real tool, so all
fourteen are now poms maven reads. Six pinned reports move by exactly one line
each — `dead-code-same-package`, `dispatch-and-cross-package` and
`visibility-ladder-and-nested-members` under kndo-adapter-java, and
`dead-code-same-package`, `dispatch-and-cross-package` and
`visibility-ladder-and-internal` under kndo-adapter-kotlin: the unit gains the
groupId it never had (`dead-code-same-package` →
`com.foo:dead-code-same-package`), which is how a real Maven coordinate is
named. `pep508-spellings` is the seventh pinned report, new. No other fixture
moves and no corpus repository moves.

**One gate was left red for a commit, and this says so.** `contract_changes_are_loud`
judges the DECISIONS text added over a RANGE of commits, so a commit's own entry
is only checked once that commit exists — running the suite before committing
judges the previous pair. The entry for `9c459d2` wrote "131 conformance fixtures:
findings byte-identical" where every one of their pinned reports moved (the
`extensions` block carries adapter versions), and the gate wants each fixture
named, `<crate> fixtures`, or `every conformance fixture`. It was red from that
commit until this one. DECISIONS is append-only, so the correction lives here:
that commit moved **every conformance fixture**, with findings byte-identical
and only the reported adapter versions changed.

**Two changes measured and NOT shipped, with their numbers.**

- *Reading `<build><sourceDirectory>` as the main unit's root.* The toolkit
  already walks `<testSourceDirectory>` through `<parent>` inheritance and
  ignores its twin; guava's root pom and `android/pom.xml` declare
  `<sourceDirectory>src</sourceDirectory>`. Narrowing the main unit to it:
  guava 8271 → 8235, **60 findings retire** (47 `untested`, 8 `unused`,
  5 `internal-only`, every one under `guava-gwt/src-super`,
  `guava-gwt/test-super` or `futures/failureaccess` — trees javac does not
  compile) and **24 appear** (15 `unused`, 9 `internal-only`), because those
  files then belong to NO unit and the graph reads a unit-less claimed file as
  second-class rather than as unstated. The eight other repositories are
  byte-identical. The narrowing waits on the unit-less file, not on the read.
- *A file no unit compiles publishes every export.* The obvious companion — an
  absence degrading toward keep-alive, the same law `Publication::Unstated`
  follows — is refuted by a fixture that already decided the opposite:
  `gradle-multi-module`'s `legacy/` is commented out of `settings.gradle.kts`,
  and under the generous default its `Scratch.kt` stops being an `unused` FILE
  and becomes "production-reachable, but no test reaches this file". Zero effect
  on guava (8235 either way): the two trees that produce a unit-less file want
  opposite answers, and the evidence cannot yet tell "shipped for another
  compiler" from "left out of the build". `GRAPH_SEMANTICS_VERSION` stays 38.

**Knobs.** `kndo:python` 11 → 12 (its manifest evidence changed);
`kndo:java` 21 and `kndo:kotlin` 16 were bumped for the `<sourceDirectory>`
read and stay bumped — the toolkit reads the tag through a walk generalised
over both source-directory tags, and the pom fixtures behind them are new
files. `GRAPH_SEMANTICS_VERSION` does not move; the contract fingerprint does
not move (`Ord` on `DependencyScope` and `RootKind` is a derive, not a shape).
`declared_roles` moves from `kndo-core` to `PluginSpec::roles_for` — file-role
globs are spec data and two crates asked for them.

**`default_extensions` becomes `default_plugins`.** The owner's term decision
was `plugin, con todo incluido`; the facade's one composition list, the gate
registry's invariant text, the README and one CLI diagnostic still said
`extension`.

## 2026-09-09 — M8.g gains a row: the corpus decomposes its own deltas

**Why it exists.** A corpus delta is a count, and the sentence explaining it is
written by hand from memory of how the engine works. That is how the transcript
tranche mis-explained guava's +24: the change was measured (60 retire, 24
appear) and the CAUSE of the 24 was a hunch about GWT shipping its super-source,
never a question put to the engine. The number was evidence; the sentence was
not.

**The row.** `xtask corpus` decomposes its own deltas: for every finding that
appeared or disappeared between the committed `corpus-findings/` and a fresh
run, it emits that subject's grounds — the keeper that changed, named by the
engine — so a tranche's COMPARISON decomposition is generated rather than
composed. Nothing new is invented for it: `Derivation`/`RuleId` landed with
extension 1/7 and `Snapshot::grounds` is already the reader.

**The hard half, named now rather than discovered late.** `grounds` answers
about a subject in ONE graph. A finding that disappeared has no accusation left
in the current graph, and one that appeared had none in the base, so the tool
needs both sides. The `--diff` composition already runs two full analyses and
pins the base by tree id; whether that hands back base-side grounds is the first
thing to check, before any new machinery is designed.

**What it retires.** The `Measure first` paragraph added to `CLAUDE.md` today —
"The explanation is part of the measurement…" — is deleted in the same commit
that lands this. A line in that file is a standing instruction only until a tool
makes it unnecessary, and this is the tool: a generated decomposition cannot be
written from memory, so the instruction has nothing left to govern. The second
paragraph added today, under `Scope belongs to the owner`, has no such
retirement and is not expected to grow one — no gate knows the plan's scope.

## 2026-09-09 — the pom's source directory is a root, and the 24 were never false positives

**Correcting today's earlier entry.** It decomposed this change as "60 findings
correctly retire and 24 appear for that reason" and held it back, calling the 24
a consequence of the graph reading a unit-less file as second-class. The second
half was a hunch about GWT, taken without asking the engine. Asked now, the
engine says something else, and the change ships.

**What the engine says.** With the root narrowed,
`kndo used-by guava/src/com/google/common/collect/ForwardingImmutableList.java#ForwardingImmutableList`
returns `kept_by: []`. Before it returned one keeper: a reference in
`guava-gwt/src-super/…/RegularImmutableList.java`. And guava's own tree explains
why that keeper was wrong — every one of these names is declared three to five
times across parallel variants of one library, each with the same package
clause:

| name | declared in |
|---|---|
| `ForwardingImmutableList` | `guava/src`, `android/guava/src`, `guava-gwt/src-super` |
| `ExtraObjectsMethodsForWeb` | `guava/src`, `android/guava/src`, `guava-gwt/src-super` |
| `LongAddables` | `guava/src` (cache), `android/guava/src` (cache + hash), `guava-gwt/src-super` |
| `TestPlatform` | `guava-tests/test` (×2), `guava-gwt/test-super` (×3) |

GWT super-source REPLACES a library file at compile time; it is never compiled
beside it. `guava-gwt/src-super/…/RegularImmutableList.java` extends the
`ForwardingImmutableList` that sits in its OWN tree, not the one in `guava/src`.
Holding the whole module directory in one unit put both variants in one
namespace pool, and the engine resolved a name across a boundary the build never
crosses. That is not a conservative over-inclusion — it is a false keep. It is
also the same fact an earlier entry recorded while refuting a `namespace_span`
fix: guava's super-source is "compiled INSTEAD of the library's file, never
beside it."

**So the 24 are the false keeps ending.** Three of them —
`ForwardingImmutable{List,Map,Set}` — are in `guava/src` itself and are named by
nothing in `guava/src`: in the Maven build of the `guava` module they are dead,
and reporting them is the point of this tool. The other twenty-one are files in
`guava-gwt/{src,test}-super` that this project has no evidence anyone reaches,
because nothing reads `.gwt.xml` and no manifest here says that tree is another
compiler's input. That is a missing READER, not a missing shape, and it goes on
the books below.

**The precondition, answered rather than accepted.** The test that pinned the
old behaviour carried its reason: "what the build adds to it is not enumerable".
It was right — the toolkit read the build helper's `add-test-source` and not its
`add-source` twin, so narrowing to `<sourceDirectory>` alone would have dropped
a generated tree the build really compiles. One walk now serves both goals, as
one walk already served both source-directory tags, and the main unit's roots
are `<sourceDirectory>` plus whatever `add-source` adds. A pom that states
neither still compiles its own directory, whole.

**Numbers.** guava 8271 → 8235. Alamofire 1484, Exposed 965, flask 19, gin 109,
lodash 20, ripgrep 143, vapor 732, vite 712 — byte-identical. No conformance
fixture moves: the one fixture that declares `<sourceDirectory>` roots it where
its files already lived.

**Knobs.** `kndo:java` 21 → 22 and `kndo:kotlin` 16 → 17: the same poms now
yield different roots, which is different evidence from the same source.
`GRAPH_SEMANTICS_VERSION` does not move — the assembly is unchanged, the
evidence entering it is not. The contract fingerprint does not move.

**And kotlin earns its transcript.** With `<sourceDirectory>` read, maven's
answer for a Kotlin tree stops being an empty `src/main/java` and becomes the
`src/main/kotlin` the pom declares, so `visibility-ladder-and-internal` captures
a real transcript and the `kndo:kotlin` row leaves
`ECOSYSTEMS_WITH_NO_TRANSCRIPT`. Only `kndo:swift` remains, for the reason its
row states.

**On the books, unstarted.** A `.gwt.xml` reader: the GWT module descriptor
names its source and super-source trees, which is a MANIFEST stating what
another compiler consumes. `ManifestEvidence::ignores` already has the slot —
"paths THIS manifest excludes from the project" — and its consumers already
leave such a file discovered, unclaimed, and casting no doubt. Twenty-one of
guava's findings are what it would answer for. Not started, not promised in a
release table, and named here so it is not rediscovered.
## 2026-09-09 — a unit name is not a unit identity, and a crate root speaks for its crate

**Two shapes, one sentence each.** A manifest naming a unit was naming a
`SmolStr`, and guava declares a module called `guava` twice — once per reactor —
so the word alone names one of two things. `UnitRef { name, declared_in }`
replaces it on `UnitDep`: the name as the manifest spells it, plus the manifest
that HAD to declare it where the ecosystem says so. And `MarkerTarget` had two
targets where a language has three: a crate root's `#![allow(dead_code)]` is
rustc's statement about the whole crate, and `MarkerTarget::Unit` is where it
now lands.

**The resolution rule is one function, and it grew one clause.** It lives on
`Aggregators::resolve` in `crates/kndo-core/src/project.rs`, the only place a
unit reference becomes a unit: a reference naming its declaring manifest is
answered THERE and nowhere else; everything else resolves by name, and the
NEAREST AGGREGATOR WINS — the naming manifest's own units first, then the first
aggregator up the chain listing a manifest that declares the name.
`ManifestEvidence::members` is the aggregation edge that makes the walk
possible, and it keeps that name. A `declared_in` that names nothing falls
THROUGH to the by-name walk rather than resolving to silence: a path that leads
nowhere is an absence, and an absence degrades toward reach, never toward
accusation. Three assertions in
`a_reference_that_names_its_declaring_manifest_is_answered_there` pin all three
readings.

**Who can honestly write `declared_in`, and who cannot.** Cargo's
`path = "../util"` names the directory whose `Cargo.toml` declares the crate:
`kndo:rust` now emits it, joined through `kndo_toolkit::join_relative` so `..`
climbs before `Cargo.toml` is appended, and every non-library target of a
manifest names ITS OWN manifest as the declarer of the library it compiles
against. Maven's reactor does NOT spell one — a `<dependency>` carries a
groupId and an artifactId and no path — so the JVM reader keeps writing
`UnitRef::named` and guava keeps resolving through the aggregator walk, which is
what that walk was built for. Reading the reactor's own resolution as a path
would have been inventing a coordinate the pom never wrote.

**`MarkerTarget::Unit` is claimed by the adapter and BOUNDED by the engine, and
the corpus is why.** `Plugin::extract` sees one file and no manifest — the
manifest read happens after extraction — so an adapter cannot tell a crate root
from any other module file. It therefore states the claim its grammar makes, and
`dispatch::UnitVoice` bounds it: `Entry` (the file the build enters the unit
through) makes the claim the unit's and broadcasts it to every file the unit
compiles under the SAME plugin — a marker is a sentence in one language, and a
file another plugin claims never read it; `Member` and `Unstated` read it as the
file's own, exactly as `MarkerTarget::File`. The report says which happened, per
file: "at unit level" or "at file level".

The measured alternative was to honor the claim unconditionally, and this tree
already refutes it. rustc scopes a lint attribute LEXICALLY: `#![allow(dead_code)]`
in `src/scratch.rs` covers module `scratch`, not the crate. The
`attribute-dispatch` fixture pins that exact tree — a file-top blanket in
`src/scratch.rs`, and `src/ffi.rs#truly_dead` expected dead — so an unbounded
reading would have silenced a true accusation to buy a numeric win. Its pinned
report is byte-identical after this change.

**ripgrep 143 → 141, and the other eight are byte-identical.**
`crates/index/src/lib.rs` opens `#![allow(warnings)]` over `mod index; pub mod
literal;`. `Handle::read_write` and `Handle::read_write_mut` in
`crates/index/src/index.rs` retire — both were accused under a blanket written
to cover them. Two diagnostics appear where they were: `index.rs` (23
declarations) and `literal.rs` (71) each report the exemption that now applies
to them. Alamofire 1484, Exposed 965, flask 19, gin 109, guava 8271, lodash 20,
vapor 732, vite 712 — unchanged. `path =` disambiguates nothing on this corpus:
no repository in it declares two crates of one name, which is precisely why the
guarantee had to come from the shape rather than from a number.

**Fixtures.** `crate-level-allow` moves: its `known_gap` on
`src/inner.rs#stale` — "M8.b units: a unit root's file-level markers dispatch
over the unit" — is closed and is now an `[[alive]]` claim, and both files
report the blanket at unit level. `attribute-dispatch`'s report does not move;
its `expectations.toml` gains the sentence naming why `truly_dead` stays dead.
The new `kmock` engine test
`a_unit_level_exemption_reaches_the_unit_and_only_from_its_entry` pins both
halves in one project; ablating the broadcast fails it and `crate-level-allow`
together, which is what makes it a gate.

**`src-layout-roots` was broken before this work, and this repairs it.** The
root `.gitignore`'s `dist/` line — `cargo xtask package` output — silently ate
`crates/kndo-adapter-python/tests/fixtures/src-layout-roots/project/src/dist/`
when the fixture landed: `git ls-files` holds its `pyproject.toml` and its test
and neither module, and the project as committed discovers 2 files where its
pinned report is of 5. That is the same "green locally, absent from CI" trap the
`coverage/` note three lines above it names, and it gets the same negation:
`!crates/*/tests/fixtures/**/dist/`. The two modules are rebuilt from the
fixture's own `expectations.toml`, which survived: every count in the pinned
report is reproduced exactly — 5 discovered, 4 claimed, 8 subjects, one `unused`
finding on `src/dist/helpers.py#_never_named`, and the same finding id — and the
one thing that could not be recovered is the BODY of `_never_named`, so its span
narrows from 26..118 to 26..58 and its report moves by those two numbers alone.
Fitting a docstring to the missing 60 bytes would have been fabricating fixture
content to match a fingerprint, and the fixture's claims are what test it.

**Knobs.** `kndo:rust` 16 → 17: the same source, different evidence — a file-top
`#![…]` now targets the unit, and a `path =` dependency carries its declarer.
`GRAPH_SEMANTICS_VERSION` 38 → 39: `ManifestEvidence` changed shape, which
manifest evidence moves rather than the fingerprint. The contract fingerprint
moves on its own, for `MarkerTarget`'s third variant. The report schema and the
gate registry do not move.

## 2026-09-09 — the two tranches compose, and two silent defects surfaced in the merge

**The composition.** Extension 5/7 (`UnitRef`, `MarkerTarget::Unit`, built in a
worktree off `8b51523`) and the `<sourceDirectory>` root (built on the branch)
touch different evidence and compose without interaction: guava 8271 → 8235 from
the source root, ripgrep 143 → 141 from the crate-level `allow`, and Alamofire
1484, Exposed 965, flask 19, gin 109, lodash 20, vapor 732, vite 712 unchanged.
Two conflicts, both in append-only records where each side appended at the end;
both resolved by keeping both blocks in order. No pinned report moved on the
merge and the regenerated fingerprint matches the one the branch already carried,
which is the evidence that the two contract changes are independent.

**`cargo fmt --all -- --check` was red at `e0913d3`, and CI would have caught
it.** It is a lint step in the generated workflow and one of the three commands
`CONTRIBUTING.md` names to run before pushing; the transcript tranche ran
`cargo test --workspace` and neither of the other two. Five files, all from that
tranche. Nothing to add anywhere — the rule is written, the gate exists, and it
was skipped.

**`src-layout-roots` had been broken since it was written.** `.gitignore`'s
`dist/` line — added for `cargo xtask package` output — silently swallowed
`crates/kndo-adapter-python/tests/fixtures/src-layout-roots/project/src/dist/`,
so `git ls-files` held four of the fixture's files and its pinned report was of
five. The fixture passed in every working tree that created it and would have
failed on a fresh clone. The negation now sits beside the `coverage/` one above
it (`!crates/*/tests/fixtures/**/dist/`), and the two modules were rebuilt from
the fixture's own surviving `expectations.toml` — every count and the finding id
reproduce, with one body span differing by 60 bytes that nobody fabricated a
docstring to close.

**What that second one says about the fixture gates.** They replay what the
working tree holds, not what the repository ships, so a fixture file an ignore
eats is invisible to every one of them. The check that would catch it is a clean
checkout, which is what CI is; it has not run since the billing hold (M0). Named
here, not fixed here.

## 2026-09-09 — `UnitVoice` dissolves: a blanket reaches what the file that wrote it mounts

**The owner's instinct, and what was under it.** `UnitVoice` was landed hours
earlier to bound a `MarkerTarget::Unit` marker — the entry of a unit speaks for
the unit, any other file speaks only for itself. The name read as borrowed, and
it was: the type reached for the MANIFEST's vocabulary (unit, entry) to state a
fact the language makes through its own module tree.

**Measured against rustc.** A scratch crate — `src/main.rs` mounting
`src/scratch.rs`, which opens `#![allow(dead_code)]` and mounts
`src/scratch/inner.rs` holding one unreferenced `fn`:

| | rustc | kndo with `UnitVoice` |
|---|---|---|
| `src/scratch.rs`'s own declarations | silenced | silenced |
| `src/scratch/inner.rs#buried` | **silenced** | **`unused`** |

A lint attribute reaches the module it is written in and everything under it.
`UnitVoice`'s rule — entry or nothing — has no third state for "a module with
children", and the fixture that pinned it (`attribute-dispatch`) had a
`scratch.rs` with no submodule, so nothing failed.

**The plan already had the shape.** Mounts. `mounted_by`, `mount_cap` and
`tree_root` were resolved before dispatch ran, and the rule that covers both
cases is one sentence: **a `MarkerTarget::Unit` marker reaches the file that
wrote it and everything mounted under it.** At a crate root — the file nothing
mounts — that is the whole unit, which is what the plan asked for. On a module
file it is that module's subtree, which is what the compiler does. The
entry/member/unstated trichotomy has nothing left to distinguish, so the type is
gone and `dispatch::apply` takes the markers a file INHERITS from the files
above it in its chain. The `enters` computation that fed it goes too, and with
it `dispatch_files`' `project` parameter.

**The note's word changed with the rule.** It said "at unit level" / "at file
level", where "unit" meant the entry wrote it. Under the subtree rule the honest
distinction is whether the blanket is the file's own or one it inherited, so the
words are `file` and `enclosing`. Two pinned reports move for it —
`crate-level-allow` (its root now reads `file`, its mounted file `enclosing`)
and `attribute-dispatch`, whose `src/scratch/inner.rs#buried` is new to the
fixture and silenced. `src/ffi.rs#truly_dead` stays accused: a sibling of the
writer is under neither chain, which is the true accusation the earlier
measurement protected and this rule still protects.

**Numbers.** All nine corpus repositories byte-identical to the run before it —
Alamofire 1484, Exposed 965, flask 19, gin 109, guava 8235, lodash 20,
ripgrep 141, vapor 732, vite 712. The rule reproduces every corpus verdict and
fixes a case no corpus repository happens to contain, which is exactly what a
fixture is for.

**Knobs.** The evidence is unchanged — rust emits the same marker — and the
contract is unchanged; what changed is how the assembly reads it, so
`GRAPH_SEMANTICS_VERSION` moves 39 → 40 and nothing else does. The fingerprint
stays put.

## 2026-09-09 — a reference that travels: `Performed::Elsewhere`, and the artifact's own shape corrected

**What the extension proposed, and why it could not be built as written.**
Item 6/7 was `Transparency { Opaque, Template }`, a field on `Declaration`:
"a use of this declaration is a use of what its body names". The rust adapter
has nothing to put it on — `macro_rules!` macros are DELIBERATELY undeclared
("textual scope… never accusing what the grammar alone cannot prove dead"), so
there is no declaration whose transparency could be stated. The shape was
designed from the metaphor rather than from what an adapter can say, which is
the same mistake `UnitVoice` made one layer down and on the same day.

**The `known_gap` already named the right shape.** `macro-template-names` asked
for "a reference that TRAVELS: the design has no coordinate for a use recorded
in one file and performed in another." So the coordinate goes on `Reference`:

```rust
pub enum Performed { Here, Elsewhere }
```

`Elsewhere` states a use and WITHHOLDS a site, which is the honest pair. The
reference still keeps the declaration alive — a use is a use — and it can no
longer be read as evidence that nobody outside this file names the declaration.
`EvidenceSink::template_reference` is how an adapter says it, bare by
construction: a template's mention qualifies nothing, because what it would
qualify against is not resolved here either.

**The consumer is `internal-only`, and only it.** A name a template in this file
spells is a name whose full set of use sites this file does not hold, so no
narrower rung is advisable for it. `unused` is untouched: it already read the
mention as a keeper, which was right.

**The measurement, taken from source before the design.** ripgrep's
`crates/core/messages.rs` declares `pub(crate) fn set_errored` (line 137) and
`pub(crate) fn ignore_messages` (line 113); the only mentions of either are at
lines 84 and 94, inside `macro_rules! err_message` and `macro_rules!
ignore_message`, and those macros are expanded from `main.rs` and `haystack.rs`.
Narrowing below `pub(crate)` on the strength of a template's own mention breaks
exactly those call sites.

**Numbers.** ripgrep 141 → 139, and the two that retire are exactly
`crates/core/messages.rs#set_errored` and `#ignore_messages` — zero added.
`crates/matcher/tests/util.rs#RegexCaptures`, the third `internal-only` on that
repository, is not a template case and stays. Alamofire 1484, Exposed 965,
flask 19, gin 109, guava 8235, lodash 20, vapor 732, vite 712 — byte-identical.

**Two known gaps close, and the precision holds.** `macro-template-names` and
`macro-use-mod` each carried a `known_gap` for this; both become `[[alive]]`.
And in the same file as the silenced `set_flag`, `local_only` — a `pub(crate)`
whose every use is ordinary code in its own file — is STILL accused. That pair
is the whole of the claim: the suppression is per NAME a template spells, never
per file.

**What is NOT built, and why.** The artifact's generalization — `type X = Y`,
re-export shims, generators, a C macro — has no measured case. The artifact
itself said so ("tiene un caso medido (ripgrep) y una generalización que todavía
no medí"), and a coordinate that travels is exactly the shape those would use
when one of them produces a number. `Performed` is `#[non_exhaustive]` and its
default reproduces every prior verdict, so nothing has to move for them to
arrive.

**Knobs.** `kndo:rust` 17 → 18 — the same source now yields a reference with a
different `performed`. The contract fingerprint moves (`Reference` grew a field,
`Performed` is new) and the ABI is re-pinned: `wit/vocab.wit` carries
`performed`, the SDK writes it and the host reads it back through
`template_reference`, so a WASM guest can state it and one that does not
defaults to `Here`. `GRAPH_SEMANTICS_VERSION` does not move — the assembly is
unchanged, the evidence entering it is not. Two pinned reports move
(`macro-template-names`, `macro-use-mod`), both losing the finding their
`known_gap` predicted.

## 2026-09-09 — the grammar states where names go, and every place has a verdict

The seventh and last item of the approved extension, and the one M8.f names
`P9`. An adapter reads a tree the grammar shapes; where the two disagree the
disagreement is SILENT — a name the grammar puts somewhere the extractor never
looks enters no stream, and the run reports one finding fewer with nothing
anywhere saying so. Six of the nine-adapter audit's findings were exactly that
and none of them had a gate.

**The unit is a name position, not a kind.** The plan's line was "a test walks
`node-types.json` and demands an entry for each kind": measured, that is 1,146
named kinds across the eight grammars, of which 741 no adapter mentions —
`block`, `binary_expression`, `argument_list`, kinds with no name to lose. A
reason written for each would be 741 sentences nobody could mean. What the
grammar actually states, and what an adapter can silently disagree with, is a
FIELD whose declared types include a name kind: the grammar saying "a name goes
here". That is 262 positions, and the run adds the ones a grammar hides behind
a supertype (`_type`, `_expression`) but a fixture writes anyway — 318 in all,
every one a place a name stands and a decision an adapter made.

**Three verdicts, and only one explains itself.** `Visited` is confirmed
positively — evidence came out of the position carrying the name — so it needs
no prose. `Seat` (the name binds and states nothing) and `Ignored` (the name
reaches the reference walk as a use) are absences of evidence, and an absence
is what every silence the audit found looked like from outside; each carries
its argument, and a reason two positions share is written once under
`[reasons]` and named `@key`, per this file's own promote-on-the-second-copy
rule — which the gate enforces from both sides, since a reason only one
position names belongs in that row.

**Three readers, not two.** `expectations.toml` says what the RUN must report,
`transcript.json` what the BUILD TOOL says a manifest means, and now
`grammar.toml` what the GRAMMAR says about a file's shape. The gate holds the
grammars itself — `kndo-gates` dev-depends on all nine — rather than asking the
adapter which one it reads: a second reader is only a second reader while it
reads for itself. It then runs each adapter over its own fixtures and grades
every claim the fixtures reach: `Visited` must produce evidence, `Seat` must
never appear in the reference stream, `Ignored` must appear in it. 156 of the
318 positions are graded that way today; the rest are listed, argued for, and
ungraded until a fixture writes one. `names` is authored per grammar and
checked from both ends — every entry must be a kind the grammar has, and every
kind a reference was actually read from must be listed.

**`kndo:html` has no grammar and says so.** It reads a stylesheet-and-tag
scanner, not a tree-sitter grammar with a node inventory, so it carries a row
in `ADAPTERS_WITH_NO_GRAMMAR` instead of a ledger — the same shape as swift's
row in the no-transcript ledger, and the gate fails the day the row outlives
its reason or an adapter has neither.

**What the first run found, and the number is zero.** `qualified_type.name` in
Go: the adapter seated the `name` field of EVERY kind, and `qualified_type`
puts the TYPE there and the package in `package`, so `fmt.Stringer` bound
`Stringer` and the use was never said — the one place a Go file names a type
another package exports was silent. It is the same shape as audit findings S5
and P3, and the same fix: `tk::Seats` names the kind beside the field, so only
the twelve kinds that really bind do. Measured: gin is byte-identical, and no
fixture shape moves either, because a Go namespace import keeps its target
package's whole surface alive and no analysis reads the reference yet. It ships
anyway and not as a findings claim: the ledger cannot record `Seat` — "this
name binds" — for a position that binds nothing, and the row would be a lie
where the fix is a sentence. `kndo:go` moves 13 → 14; an extraction test pins
both halves. Neither the graph semantics nor the fingerprint moves.

**A fixture the ledger asked for.** `type_item.name` is `Visited` — rust
declares a top-level `type` alias — but the fixtures only ever wrote the
associated form inside an `impl`, where the same kind is a MEMBER and declares
nothing, so the run could not bear the claim out. `type-alias-item` writes both
and pins the dead alias; the rust floor moves 26 → 27. That is the ledger
working the way it is meant to: a claim the corpus cannot reach is a fixture
that is missing.

**Swift's 72 ignored rows are an argument for a grammar patch.**
tree-sitter-swift declares no expression supertype, so every operand slot lists
`simple_identifier` outright among some ninety alternatives and each is a name
position by the same rule that makes `qualified_type.name` one. They share one
reason, and the count is the measurement M8.f's vendoring row can be judged
against: patch the grammar with an `_expression` supertype and swift's
inventory collapses to the slots that really reserve a name.
