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
