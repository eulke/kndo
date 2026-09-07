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
