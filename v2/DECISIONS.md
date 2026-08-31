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
