# Roadmap

**Status:** Draft · Milestones ship in order; each has explicit exit criteria. Docs are updated in
the same PR as the code they describe — a milestone isn't done if its documents lie.

## M0 — Definition ✅ (completed 2026-08-18)
All RFCs/ADRs/contracts reached `Accepted` after debate; open questions resolved or explicitly
deferred with reasons of record.

**Exit (met):** contracts signed off (`core-traits.md`, `output-schema.md`); candidate-rule
table (RFC 0005 §13) fully triaged — no candidate left undecided; warm-budget feasibility
validated by [spike 0001](spikes/0001-performance.md) (73 ms measured vs 500 ms budget);
product name decided (ADR 0007).

## M1 — Skeleton + first language (JS/TS), full-scan only
Workspace layout (`kndo-cli`, `kndo-core`, `kndo-adapter-toolkit`, `kndo-adapter-js`),
graph model per contracts, discovery, extraction, resolution driver, conformance harness.
Package model per RFC 0011 with npm/pnpm/yarn workspaces (the single-package repo is the n = 1
case, so this is foundation, not a feature).
Analyses: `unused` (symbols, files, dependencies — with file/directory rollup), `undeclared`,
`version-skew` (manifest-only, rides the RFC 0011 package model), and `duplicate` on
byte-identical files (subject `file` — free on content hashes).
Output: human (RFC 0009 visual language, incl. degradation tiers), JSON v1.0.0, and agent
format v1 (output-schema §9) — all emitted through the `Engine` facade (contracts §5 — the CLI
is a frontend from day one, not a shortcut). No cache yet (cold runs only).

**Exit:** correct findings on fixture corpus + 3 real OSS TS repos; `kndo check` on kndo's own
JS-free repo returns cleanly; JSON validates against generated schema.

## M2 — Cache, incrementality, diff modes, pre-commit ✅ (completed 2026-08-20)
`.kndo/` cache (ADR 0004), warm-run algorithm, `--staged`/`--diff` with derived-effects delta
(RFC 0004 §6) reading git trees in memory (no tempfiles — content-addressed blob-hash sidecar
makes a warm diff's cost proportional to what changed, not repo size), baseline + suppressions,
exit codes, `kndo init` hook installer, `kndo doctor`. First navigation verbs over the warm graph
(RFC 0007): `find`, `describe`, `uses`, `used-by`, `trace` with query JSON envelopes,
multi-selector support and `kndo query` (composite JSONL queries, single graph load).
`--threads` + one global rayon pool (RFC 0008 §5).

**Exit (met):** warm p95 measured well under the 500 ms budget on the 5k-file benchmark repo
(full ~40 ms, `--staged` ~70 ms, `--diff` ~14 ms, a single changed file ~84 ms — all warm,
release build); `--threads 1` ≡ `--threads N` byte-identical output (test-enforced);
`--no-cache` ≡ cached results verified on the fixture matrix and the 5k benchmark; kndo runs in
kndo's own pre-commit hook (dogfooding is live on this repo's real commits, not simulated).

**Deferred out of M2** (RFC 0004 §4 kept as the accepted target design; see its "Implementation
status" note): the patch/dirty-region incremental algorithm (§4 step 4, §5's incremental
reachability-color BFS) — the current all-or-nothing rebuild already meets the warm budget at
benchmark scale, so building the patch algorithm wasn't required to close this milestone; revisit
if a larger real repo's rebuild cost grows past budget. Also deferred: wiring the above gates into
an actual CI workflow (measured manually this session — no CI/multi-core infra was available to
automate it) and the ≥ 3× scaling-at-8-cores check, which needs that same infra.

## M3 — Reachability semantics complete + second language (Go)
`test-only` (three-color reachability), `untested` (its inverse — same coloring passes),
tooling roots, wildcard-edge conservatism, confidence surfacing, library mode, `internal-only`
and `private-type-leak` (both directions of the visibility-mismatch comparison, RFC 0005 §7),
`deep-import` (contract-gated boundary erosion with pair-level rollup, RFC 0011 §4),
`delta_origin` introduced/derived on diff findings (RFC 0004 §6). Go adapter proves the contract fits a second language
without core changes — any needed contract change happens *here*, cheaply. Navigation completes:
liveness traces (`trace X` from roots), `used-by --split-by-color`, and `kndo impact`
(incl. `--if-deleted` simulation, reusing the derived-effects machinery).

**Exit:** Go conformance corpus passes; a deliberately-injected "only tests call this" fixture is
caught in both languages; the RFC 0007 §5 agent workflow (find → used-by → impact → check)
runs end to end on a fixture; contract diffs (if any) documented in updated contracts + ADR.

**Progress so far:** `untested` and `internal-only` implemented and dogfooded (RFC 0005 §7, §9).
Go adapter landed (`kndo-adapter-go`) — claiming, extraction, manifest, and resolution modules,
conformance fixtures, real-CLI-dogfooded against hand-built multi-file/multi-package projects
(both `unused` and `test-only` correctly cross the language boundary — the exit criterion's own
"only tests call this" case verified working for Go, not just JS). The contract genuinely needed
extending twice, both documented (contracts §2, RFC 0005 §1, RFC 0011 §4): `FileFacts::unit` for
package-scoped (not file-scoped) reference resolution, and a `ResolveCtx::files_in_dir` query for
languages whose import unit is a directory rather than a single file — plus one real, previously-
latent core bug the second language's shape exposed and fixed (`analysis/reachability.rs`: a
symbol-only root, the common case in Go, never propagated reachability to the file-attributed
references its own file makes). `deep-import`'s verdict and `kndo impact` remain — each needs
its own real design pass (subpath+surface tracking; a new navigation verb), not attempted as a
rushed add-on to this pass.

RFC 0012 (Precise Reference Semantics & Visibility) then landed its first six stages here:
`member_of` + the duck-typed member-call fallback (killing the unused:method false positive),
the Go `dir#package` unit key, symbol-granular reference attribution (`within`) in the core
and **both** adapters — transitive death is now visible in Go and JS/TS alike, pinned by
conformance fixtures in each — §5's `RefKind` tagging + callable `signature_span`s feeding
the `private-type-leak` analysis (RFC 0005 §7's second direction), likewise fixture-pinned in
both languages, and §6's ladder-as-data: adapters declare `(scope, label)` visibility rungs,
assembly carries them onto the graph, and three consumers read them — `internal-only`'s
tightest-sufficient rung (closing go.md's documented same-package under-reporting, fixture
`internal-only-unit/`), `private-type-leak`'s scope comparison, and the member fallback's
visibility-scoped candidacy. §7 (`detected_origin`) closed go.md's other documented trait
limitation: extraction now corrects the origin axis from content (Go's `DO NOT EDIT.` banner,
JS `@generated` markers, via the toolkit's bounded `ContentMarkers` scanner), so generated
files are exempt from findings even when their paths look authored — fixture `generated-file/`
in both languages, negative-control dogfooded. §9 (qualified-reference resolution) moved the
system's last string-guess into assembly: Go emits qualified accesses as structured
`{ name, scope_context }` facts (plus `RawImport.local_alias` and `FileFacts.unit_name`), and
the core resolves the qualifier against the import's alias or the *target's* declared package
name — fixing the dir≠package case (`gopkg.in/yaml.v3` → `yaml`, fixture
`qualified-package-name/`) and keeping receiver accesses (`t.helper()`) from ever capturing a
same-named free function. §10 closed the queue: `go.work` is claimed and parsed (`use`
directives → `workspace_members`), and sibling-module imports resolve as `WorkspaceMember` —
reachability plus the `require` contract go.work does not waive — pinned by fixtures
`go-work-multi-module/` (declared sibling: precise cross-module liveness, one deliberate dead
helper found) and `go-work-phantom-dep/` (undeclared sibling: a real phantom dependency). The
path-renaming `replace` directive stands as RFC 0012 §10's one recorded divergence. **All RFC
0012 stages (§§3–10) have now landed.**

`deep-import` then landed for **internal providers** (RFC 0011 §4, all three design rules:
contract gate on `declares_surface`, one finding per consumer→provider pair with capped site
evidence, computed remediation splitting also-public from genuinely-internal symbols via
surface-reachability) — conformance fixture `deep-import/`, gate verified both ways with the
real CLI. The **external-provider half remains**: it needs the provider's own manifest, which
`node_modules/` discovery doesn't reach — a provider-manifest peek at resolution time is its
own discovery/cache/purity design pass (the gate stays closed meanwhile, so it degrades to
silence, never noise). `kndo impact` landed next (RFC 0007 §4.6): the reverse-closure blast
radius on the same adjacency `uses`/`used-by` navigate (depth-annotated, color-summarized,
affected roots listed), and `--if-deleted` simulating removal on a patched graph copy through
the *real* reachability engine — reporting typed flips (newly unreachable, newly test-only,
freed dependencies) rather than synthesized findings; wired through the single verb, `kndo
query` JSONL, human/agent/JSON renderings, and the regenerated committed schemas.

**M3 exit: met.** The four criteria, each with its standing evidence: (1) the Go conformance
corpus passes — seven fixtures through the real engine; (2) the "only tests call this" case is
caught in both languages — the JS `test-only/` fixture plus the Go dogfood the progress notes
above record; (3) the RFC 0007 §5 agent workflow runs end to end —
`crates/kndo/tests/agent_workflow.rs` drives `find` → `used-by` → `impact --if-deleted` →
edit → `check` against the real engine on the RFC's own legacy-tax fixture, asserting the
final check comes back clean; (4) every contract change M3 needed is documented in the
contracts + RFCs it landed with (`unit`, `files_in_dir`, the RFC 0012 series, the RFC 0011 §4
surface fields). One consciously carried item, recorded above and in RFC 0011 §4: the
external-provider half of `deep-import` awaits a provider-manifest peek design — gated to
silence until then.

## M4 — Duplication, CRAP, health
`duplicate` (winnowing index, incremental), `cyclic` (SCCs with per-language tolerance),
`crap` + lcov/JaCoCo ingestion plugins (ADR 0005), `health` score + `kndo health`, SARIF output.

**Progress so far:** `cyclic` landed first (RFC 0005 §8, all of it): iterative Tarjan over the
file-import graph and the derived real-package graph, one finding per cycle anchored at its
most-referenced node, tolerance as adapter-declared data (`CyclePolicy` on the descriptor,
carried onto the graph like the ladders — JS/TS Hazard at both levels, Go Impossible so the
compiler-forbidden case is skipped, mock Hazard), cross-package file cycles rolled up into the
package-level finding, and the shortest cycle path as the finding's evidence chain — which
made `related` real: `Finding.related` now exists end-to-end (JSON schema regenerated, human
renderer shows `└` evidence lines, agent format `evidence:` lines), first populated by
`cyclic`, adoptable by every other analysis as their evidence models land. The
`npm-workspace-monorepo` fixture's expected set gained the package-cycle finding its own
deliberate phantom-dependency loop always implied.

Structural `duplicate` landed next (RFC 0005 §6): the toolkit gained a data-driven
function-shape module (`metrics.rs` — one walk yields cyclomatic, LOC, and winnowing
fingerprints over the normalized token stream; K=10/W=8, blake3-derived gram hashes so cached
facts never change meaning under a hasher upgrade), both adapters emit `FileFacts::functions`
for callables (Go functions + methods; JS function declarations + arrow/function-bound
consts), assembly resolves them onto `ProjectGraph::function_metrics` keyed by SymbolId, and
the analysis groups Type-1/Type-2 clones transitively (shared-fingerprint index, same
language, Jaccard ≥ 0.8, boilerplate postings capped) into one `info` finding per group with
every instance in `related`. The M4 exit criterion held on the real CLI: reformatting one
clone (whitespace, comments, line collapsing) leaves the finding id byte-identical. Perf note
of record: the pushed `cyclic` commit carried a quadratic cycle-anchor scan (933 ms warm on
the pathological 4996-file-SCC synthetic corpus, vs a 352 ms pre-cyclic base) — fixed here
with a precomputed in-degree pass (~490 ms warm; the remaining ~140 ms is Tarjan + the
duplicate index on 15k deliberately-identical functions, linear costs on a corpus built to be
worst-case; bench5k stays comfortably in budget).

SARIF output landed next (`--format sarif`, contracts §7's mapping exactly): SARIF 2.1.0
rendered core-side like every machine format — category → rule.id with one rule per distinct
category, severity → level (info → note), the `related` evidence chain → relatedLocations
with role-prefixed messages, confidence and subject/symbol/package under properties, and the
stable kndo finding id as partialFingerprints (SARIF's result-matching mechanism, which
kndo's line-number-free ids are already built for). Diff modes emit current findings only —
SARIF models "the results of this run"; the JSON envelope remains the delta carrier.

`crap` + coverage ingestion landed next (RFC 0005 §10, ADR 0005): the `Plugin` trait gained
its `ingest_coverage` hook (sink-based, same discipline as the contribute hooks — the contract
was updated from its sketched `Option<CoverageData>` shape to match) with the lcov built-in as
the first ingester; the engine locates reports at the plugins' `requested_file_access`
well-known paths (`coverage/lcov.info`, `lcov.info`), enforces the 7-day freshness gate (stale
→ one diagnostic, report ignored), and passes the resulting `CoverageMap` to `run_all` as a
separate per-run input — never on the graph or its snapshot, because report freshness varies
independently of content hashes. The analysis is the RFC's formula verbatim
(`comp² × (1−cov)³ + comp`, findings above 30), per-function coverage as the covered fraction
of instrumented lines inside the symbol's span, no-report functions scored pessimistically at
cov = 0 and flagged `coverage: none`, test/generated/vendored code exempt. Dogfooded all four
paths on the real CLI: fresh report (covered function silent, uncovered one fires with
`coverage 0%`), full coverage silencing, stale-mtime report (diagnostic + pessimistic
scoring), and no report at all. Perf note: the uncovered pathological corpus now legitimately
emits 75k crap findings (every one of its 15k-clone families is complex and uncovered), which
moves synth-repo warm to ~750 ms — linear finding-construction/serialization volume, not an
algorithmic regression; bench5k stays at ~60 ms warm.

The health score closed the milestone's feature list (RFC 0005 §11, all of it): the composite
computed from the same primitives the analyses use (reachability colors, metrics, coverage,
aux stats returned by `cyclic`/`duplicate` — never recomputed), with the saturation curve and
its per-category constants now documented in §11 as the contract. `SymbolMetrics` gained
`token_count` (graph schema 9, facts Go 9/JS 8) so duplication's ratio basis is real tokens,
not a proxy. `RunResult.health` carries the §4 object in every mode: full mode reads/writes a
`.kndo/health.json` snapshot for the trend line, diff modes compute `previous` from the
"before" side — the exit criterion's delta, rendered in the human diff header
(`health 81.3 ──▶ 60.9 −20.4 ↓ D (10.9 from F)`, with the grade-boundary distance on drops),
the agent result line (`| health 81.3 -> 60.9 (D)`), and the JSON envelope. `kndo health`
renders the full category table (`--by-package` adds RFC 0011 §6's grouping of the same
penalties; always exit 0 — gating stays with budgets). One test adjusted with cause:
`threads_determinism` now strips `health.previous` before comparing, because the trend
snapshot makes consecutive full runs legitimately differ in that one field — the score and
breakdown remain in the byte-identical comparison.

**Exit:** duplication findings stable under reformatting (Type-2) — **met** (byte-identical id
on the real CLI); health deltas shown in diff modes — **met** (see above); CRAP hotlist
matches manual audit on a real repo — **met**: kndo over expressjs/express (real clone, no
coverage report) produced a four-function hotlist (`lib/response.js#sendfile` CRAP 210 / comp
14, `lib/response.js#stringify` 72/8, `lib/view.js#View` 72/8, `lib/utils.js#acceptParams`
56/7); manual decision-point counts of three of the four (sendfile 13+1 including its nested
closures, View 7+1, acceptParams 6+1) matched kndo's cyclomatic numbers exactly, and sendfile
is exactly the function an express maintainer would name first. Precision is exact; the
audit's one recall note is an adapter scope limit that predates `crap`: express's
`res.send = function send()` prototype-assignment idiom is outside the JS adapter's
documented declaration extraction, so such callables never enter the metrics table — now
called out explicitly in docs/adapters/js-ts.md §2 with the RFC 0012 §3 frame for lifting it.

**M4 exit criteria are all met.**

## M4.5 — Performance: re-measure, re-arm, re-earn the budget

Inserted before M5 deliberately: three milestones of features grew latency silently, because
the RFC 0008 §7 enforcement (benchmark suite as a gate) was deferred out of M2 and never
built. Measured drift (release, warm, medianas): bench5k no-op ~40 ms (M2) → ~60 ms; bench5k
one-file change ~84 ms (M2) → ~155 ms — the pre-commit case nearly doubled, still inside the
500 ms budget but trending wrong. Phase breakdown (one-off instrumented build, now
productized as `--verbose`): the one-file change is ~122 ms of assembly — the all-or-nothing
rebuild re-resolves all 5k files and writes the snapshot **inside** the critical path; the
pathological synth corpus adds analysis costs on `HashMap<NodeRef>`/SipHash (reachability
93 ms, internal-only 70 ms), 78 ms sorting ~90k findings, and a render that clones every
finding before serializing (172 ms).

Stages, in order — measure first, then optimize against locked numbers:
**E0** instrumentation (`--verbose` per-phase timings, RFC 0009 §6) + the 50k fixture and
RFC 0008 §7 scenario suite with a recorded baseline and regression gate;
**E1** measured quick wins (FxHash, render without cloning, snapshot persist off the
critical path per §2);
**E2** bounded structural work: reachability over shared CSR adjacency + per-tier bitsets
(§3, applied to the hottest algorithm only), inter-analysis parallelism with deterministic
reduce;
**E3** the RFC 0004 §4 patch/dirty-region decision **with 50k data**: if the one-file change
at 50k breaks 500 ms after E1–E2 (extrapolation says it will), the incremental algorithm
lands before M5; if not, its trigger gets real numbers instead of guesses.

**Progress:** E0a landed — the engine now times every phase (`RunResult::timings`: assembly,
coverage ingest, each analysis, sort, health; diff modes carry the after side plus a
before-side rollup) and `--verbose` renders the block with cache state. Timings stay out of
the JSON envelope by design: wall times are run metadata, and the §4 determinism matrix
compares envelopes byte-for-byte.

E0b landed — `cargo xtask bench`: deterministic generated fixtures at 1k/5k/50k (import
chains from the manifest root, one dead file per decade, branchy function bodies, a clone
family every 500 files, git-initialized so `--staged` runs the real diff path), the five
RFC 0008 §7 scenarios measured end-to-end (wall time of the release binary, min-of-N),
baseline recorded in `docs/perf-baseline.json`, >10%-and->10ms regression gate behind
`--gate`. First full reading (this machine): 1k warm-noop 20 ms / 1-file 37 ms; 5k
warm-noop 86 ms / 1-file 217 ms; **50k warm-noop 903 ms / 1-file 1 962 ms** — at 50k even
the no-change case is over the 500 ms mark and the one-file change is 4× over, which
settles E3's question in advance: the all-or-nothing rebuild does not scale to 50k, and the
gap between no-op and 1-file (~1 s of pure re-resolve/re-link/persist) is exactly what
RFC 0004 §4's patch algorithm removes. E1/E2 must first pull the no-op cost down; then the
incremental lands.

E1 landed as a measured series (each commit carries its own before/after table): FxHash for
every internal map (RFC 0008 §3's "no default SipHash" — −17…−42% across scenarios), the
JSON envelope borrowing the run's collections instead of deep-cloning them to serialize, and
the graph-snapshot persist moved off the critical path (§2 verbatim: a detachable writer on
a background thread, joined after render via Engine::drop — ~400 ms off the 50k one-file
change alone).

E2 landed: reachability keeps its literal tiered algorithm but on §3's data layout — dense
node indices, one CSR adjacency (offsets + targets + confidences), per-(kind, tier) bitsets,
the module-load rule as an implicit `symbol → owner` CSR edge at `Certain` (same semantics,
no special case) — 57 → 13 ms at 50k; and the independent analyses now run concurrently
through an explicit `rayon::join` tree (§2's inter-analysis parallelism, §4's parallel
compute / deterministic reduce: named slots, fixed extend order, id-sort regardless).
Cumulative vs the E0b baseline: 5k warm-noop 86 → 61 ms, 5k one-file 217 → 127 ms, 50k
warm-noop 903 → 590 ms, 50k one-file 1 962 → 1 312 ms, 50k staged 1 533 → 917 ms. The 50k
no-op is now dominated by assembly (~220 ms: discovery + hashing 50k files and
validating + deserializing a 29 MB snapshot) — E3's territory, together with the ~700 ms
rebuild gap the one-file change still pays.

E3 closed with its decision made **and** two more spec'd mechanisms landed. The data: even
after E1+E2, the 50k one-file change sat at ~1.4 s — so RFC 0004 §4's patch algorithm is
required and scheduled as the next unit of work before M5 (design notes recorded in that
RFC's implementation-status section: a body-only v1 guard — file set unchanged, surface and
imports identical, no manifest — makes the dirty set exactly the changed files, with full
rebuild as the fallback everywhere else; §5's incremental recoloring is deprioritized *with
data* — full recoloring costs 13 ms at 50k after the CSR rewrite). Landed en route, both from
the accepted designs: RFC 0004 §4 step 2's **stat-scan** (a `(mtime, size) → blake3` sidecar
with git's racy-write guard — unchanged files are never read, contract-tested with a
poisoned-hash entry) and RFC 0008 §2's **parallel resolution** (phase 3b's per-file loop is
embarrassingly parallel once the symbol tables are frozen; per-file contributions merge in
FileId order, DependencyIds still assigned first-appearance-in-file-order in the sequential
reduce — 476 → 265 ms at 50k).

Final M4.5 table vs the E0b baseline (same machine, min-of-N; the container measurably
slowed over the session — cold rows especially are noisy, noted in the re-recorded baseline):
1k noop 20 → 14 ms · 5k noop 86 → 55 ms · 5k 1-file 217 → 117 ms · 50k noop 903 → 580 ms ·
50k 1-file 1 962 → 1 134 ms · 50k staged 1 533 → 1 052 ms. On the original M2 bench5k corpus,
apples-to-apples: warm no-op 44 ms and one-file 97 ms against M2's ~40/~84 recorded on a
faster machine state — at the marks, now carrying four milestones more of analyses.

**Exit:** the RFC 0008 §7 scenarios run as a suite against a recorded baseline with a >10%
regression gate — **met** (`cargo xtask bench [--gate]`); bench5k warm no-op and one-file
change at or under their M2 marks — **met within measurement noise** (44/97 ms vs ~40/~90 on
a machine now measurably slower than at M2's recording); the 50k fixture answers the
incremental question with data — **met**: it fires, the patch precedes M5.

**M4.5 exit criteria are met.**

The carried item closed immediately after: **the incremental patch landed as RFC 0013**
(designed, documented, then implemented in three commits — hardening, schema 10, patch).
Ownership is explicit (`Edge.owner`), the dirty set is derived from surface signatures rather
than guessed, canonical order makes graph equality checkable, barrel aliasing resolves to an
order-independent fixpoint (multi-hop barrels now work), and the patch's single correctness
statement — a patched graph is byte-identical to the full rebuild of the same tree — is
enforced by a dedicated equivalence suite over both adapters' conformance corpora plus
mock-level unit cases. Measured at 50k: the one-file change fell 1 134 → ~575 ms, equal to a
no-op — RFC 0004 §4's target. The dirty-fraction threshold is 5%, measured (30% regressed the
1k/100-file scenario by +22%). Deliberately out, with reasons in RFC 0013 §7: fine-grained
dirty propagation, incremental recoloring (13 ms at 50k), file-set changes.

## M5 — Remaining languages + plugin system GA
Adapters: Java, Kotlin, Swift, Rust, JSON, CSS (Rust moved first — dogfooding on kndo itself is
the highest-signal corpus; then Java → Kotlin share infra; Swift; CSS+JSON close cross-language
edges). First-party ecosystem plugins for detected frameworks (initial set per RFC 0003 §3).
**WASM plugin/adapter ABI** (`kndo-plugin-api`) published with a sample external plugin +
compliance suite.

### M5 progress — Rust adapter ✅ (landed 2026-08-20)

`kndo-adapter-rust` per docs/adapters/rust.md: module tree as the file graph (`mod foo;` →
`self::foo`, `#[path]` → `file:` specifiers), two-step tail rule, `crate`/`self`/`super`
anchors, bare-segment precedence (stdlib → workspace member with `_`↔`-` normalization and
self-crate detection → declared dep → local-module retry → `Dependency` probable fallback),
Cargo.toml extraction with the `workspace = true` inheritance sentinel, bins rooting
unconditionally and lib entries only when publishable, `#[cfg(test)]` subtrees as inline test
roots, trait-impl methods rooted for dispatch, and entry-liveness imports keeping crate
entries alive under deep paths. Assembly grew the **library-surface fixpoint** (phase 2.7,
shared semantics with JS): `pub mod` chains and bare `export * from` re-exports extend a
published package's surface transitively, with patch-side parity in `try_patch` — this lifted
the JS star-reexport limitation recorded in docs/adapters/js-ts.md §5.

The dogfood loop (kndo on kndo, ~70 files, ~290 ms cold) drove findings 1100 → 324 through
adapter/core precision fixes rather than suppression — primitives-as-deps, aliased-qualifier
re-probing, attribute-argument path reconstruction (item and field level, lint attributes
excluded), trait-visibility inheritance, struct-literal initializer reads, macro token-tree
path reconstruction, and Rust 2021 inline format captures; core gained the `test_root_symbols`
exemption (a test being test-reachable is a test, not a finding). Every surviving category was
audited: the remaining `unused`/`test-only` entries are true statements about the code.
Bench gate note: the recorded perf baseline predates this container's current load — HEAD
itself misses it by +30% — so parity was verified by interleaved A/B against HEAD (overlapping
ranges, medians favor the adapter build); warm scenarios are unchanged or better.

### M5 progress — Java adapter ✅ (landed 2026-08-21)

`kndo-adapter-java` per docs/adapters/java.md: package identity is declared *and* directory-
checked by javac (a hybrid of Rust's declared-tree model and Go's directory model) — the
adapter sidesteps source-root detection entirely by setting `FileFacts::unit` to the
**declared** package name, never the directory. No dogfood corpus exists for this one (kndo is
written in Rust) — precision rests entirely on four real Maven/Gradle conformance fixtures run
through the real `Engine`, no mock.

Two structural findings changed core, not just this adapter: **`ResolveCtx::files_under`**
(the recursive counterpart to `files_in_dir` — a publishable Java module's root promotion needs
every `.java` file under `src/main/java/**` at arbitrary package depth, not one representative
file) and **`ResolveCtx::with_units`/`unit_files`** (a package-name → declaring-files reverse
index, because a Java import specifier *is* a `unit` value directly — unlike Go, which turns a
specifier into a directory and lists it). Root promotion itself is a new shape: neither Go's
blanket per-file `RawRoot` emission (no manifest-visible privacy signal) nor Rust's single-
entry-file mechanism (no single entry point — every public class is API) fit, so a publishable
module's manifest emits one `ManifestRoot` per non-test source file, reusing the existing
per-file declaration-promotion path with zero new mechanism beyond the two `ResolveCtx`
additions above.

**`resolves_dependency_usage: false`** (new `AdapterDescriptor` field, carried onto
`PackageNode`): Java's import namespace has no reliable mapping to Maven/Gradle coordinates
without resolving the classpath, which kndo — a static source analyzer — structurally never
does. Rather than flood every declared dependency with a false `unused` verdict,
`dependency_hygiene` skips languages where this is `false`, with one diagnostic instead of a
finding per dependency; `version-skew` is unaffected (pure manifest-fact comparison, no usage
edge needed) and stays fully precise for Java from day one.

A real, load-bearing bug the conformance fixtures caught before it shipped: the four-rung
visibility ladder's `package-private` rung was first mapped to kndo's `VisibilityScope::Package`
— which means "same manifest/workspace-member" (RFC 0011's `PackageId`, JS's granularity), a
**different thing** from a Java `package` (a `com.foo` namespace; one Maven module routinely
holds several). The fix maps `package-private` to `VisibilityScope::Unit` instead — exactly
mirroring Go's own choice, since `FileFacts::unit` already carries the declared Java package —
caught by a fixture exercising genuine cross-Java-package, same-Maven-module visibility, which
misfired as a false `internal-only` before the fix and passed clean after.

### M5 progress — Kotlin adapter ✅ (landed 2026-08-21)

`kndo-adapter-kotlin` per docs/adapters/kotlin.md: the first adapter that shares its manifest
layer wholesale with a sibling — `kndo-adapter-toolkit::jvm_manifest`, extracted from the Java
adapter's `manifest.rs` in the same session (a `pom.xml`/`build.gradle`'s shape has zero
dependency on the module's source language), parameterized by a `JvmSourceLayout`
(`src/main/java` + `.java` for Java, `src/main/kotlin` + `.kt` for Kotlin). Same no-dogfood-
corpus situation as Java — precision rests on four conformance fixtures.

The real language-fit work was the visibility ladder, and it runs the *opposite* direction from
Java's own bug: Kotlin's `package` carries no visibility meaning at all (default with no
modifier is `public`, not package-scoped), so there is no `Unit`-scoped rung anywhere — instead
`internal` maps directly to `VisibilityScope::Package` (kndo's "same manifest" granularity, a
Kotlin compilation module) with no widening needed, the one rung Java's ladder has no
equivalent of. `protected` (members only) still widens to `Public`, mirroring Java's own
two-rungs-share-a-scope pattern. Getting this backwards — reusing Java's `Unit`-for-package-
scope reflex — would have been the *same class* of bug Java's own fixture caught, inverted:
narrowing suggestions onto declared-public code Kotlin's compiler would reject narrowing.
`member_of` follows RFC 0012 §3 literally (top-level functions, including extension functions,
get `None`) and companion-object members attribute to the *enclosing class* rather than the
companion itself — `Widget.factory()` is how real code addresses them, not `Widget.Companion.
factory()`.

Declaration dispatch (9 shapes: class/object/companion/function/secondary-constructor/property/
type-alias/init-block/enum-entry) is table-driven from the start this time — a lookup-and-call
over `(&str, fn(...))` pairs, not a `match` with one arm per kind — after the Java session's
own CRAP gate taught that a flat match past ~4 arms already crosses the zero-coverage
threshold. Two independently-verified upstream tree-sitter-kotlin-ng 1.1.0 grammar edge cases
(meta-annotated parameterless `annotation class`; any `class`/`interface`/`object` body written
entirely on one line) are documented, not worked around — third-party grammar issues, and every
fixture/test in this adapter simply uses realistic multi-line formatting.

**A second infra fix, found while building this adapter's own `GENERATED_MARKERS`
constant**: `kndo_adapter_toolkit::classify::LineMarker::Contains` had no guard against matching
its *own declaration* — `kndo-adapter-java/src/extraction.rs`'s `Contains("@generated")` string
literal was itself a textual match for the Rust adapter's identical marker scanning kndo's own
repo, silently exempting that file from `crap`/`unused`/`untested`/`internal-only` via
`FileOrigin::Generated`. Fixed by requiring the matched line to look like a comment (`//`, `/*`,
`*` after trimming) — `PrefixSuffix` (Go's) was already self-safe by column-anchoring, by
design; `Contains` wasn't. Effect on kndo's own dogfood: health score 59.3→69.3 (D→C), not from
any code change but from ~200 previously-hidden findings (mostly `crap`/`untested`) becoming
visible — real pre-existing debt, now visible, not something this fix set out to remediate.

### M5 progress — Swift adapter ✅ (landed 2026-08-21)

`kndo-adapter-swift` per docs/adapters/swift.md: the first adapter whose manifest
(`Package.swift`) is genuine Swift source rather than a data format — `manifest.rs` parses it
with the same tree-sitter-swift grammar `extraction` uses and reads the `Package(…)` call's
labeled arguments, the SwiftPM analogue of Rust's structured `Cargo.toml` parse. That shape
forced one new engine-wide guarantee to be enforced *explicitly* for the first time:
`claim()` had to exclude `Package.swift` from the ordinary source glob it otherwise matches
(it genuinely ends in `.swift`), or "manifests are not claimed" — load-bearing everywhere
else in the engine because no other manifest format doubles as valid source in its own
adapter's language — would have silently broken. Caught by a fixture, not by inspection.

The other real language-fit finding was structural, not a bug: Swift's default visibility
(no modifier at all) is `internal` ≈ `Package` scope — a *third* distinct default among the
launch languages (Java ≈ `Unit`, Kotlin = `Public`), and it happens to be exactly the
"same-unit usage" scope `internal-only`'s tightest-sufficient check asks for. So the common
case — an un-annotated declaration used only within its own SPM target — never fires
`internal-only` for Swift at all, unlike the analogous Java/Kotlin fixtures, which do (their
defaults are wider than same-package usage requires). Documented in the conformance fixtures
rather than worked around, since it's correct: the language's own default already is the
tightest rung that covers the common case.

A second, unplanned finding surfaced while writing the conformance fixtures: `main.swift`
(and any `.swift` file, structurally) permits bare top-level *statements*, not just
declarations — `source_file`'s children can be an `if_statement`, a `call_expression`, any
expression, the same shape a function body allows. The initial extraction only dispatched
declarations at top level, so a `helper.live()` sitting next to a top-level `let` in
`main.swift` left no reference behind at all — `helper` and `Helper.live` both read `unused`
in the first conformance run. Fixed by falling back to the same body-walker a function uses
for anything `DECL_HANDLERS` doesn't recognize at top level. That same investigation surfaced
RFC 0012 §4's already-documented "Swift lazy globals" `within` case (Swift globals are always
lazily-initialized outside a script file — an ordinary file's top-level `let x = f()` only
runs `f()` on `x`'s first access, so the reference belongs to `x`, not to "module load")
which the adapter hadn't implemented yet; `main.swift` itself is the one exception, since its
top-level code executes procedurally like a script rather than lazily.

Declaration dispatch is table-driven from the start (same CRAP-gate reasoning Kotlin's session
established). One grammar-shape gap the dispatch table needed a second entry for: a protocol's
method requirement is its own node kind, `protocol_function_declaration` (no body field at
all), not `function_declaration` — missed on the first pass, caught immediately by the
`declarations_cover_the_type_zoo` conformance-style unit test.

### M5 progress — JSON adapter ✅ (landed 2026-08-21)

`kndo-adapter-json` per docs/adapters/json.md: RFC 0002 §3's "non-source language" — no
declarations, no imports, no references, no roots, no visibility ladder, no manifest of its
own. The smallest adapter yet, both in code and in what it needed to get right, since its
entire value is one sentence from the RFC: "JSON participates as import targets so file-level
`unused` findings cover config/data files." Confirmed by tracing the actual resolution code
rather than assuming: a JSON import already resolves to a real graph edge with zero JSON
adapter in existence (`ResolveCtx`'s known-files index is built from every *discovered* file,
claimed or not), so resolution was never the gap. Every analysis in `kndo-core/src/analysis/`
opens with `let Some(class) = file.class else { continue; }` — an orphaned `.json` config file
was structurally *invisible* to `unused`, not merely reachable, because nothing ever gave it a
`FileClass`. Claiming is the entire fix.

That framing also settled two things the RFC's own prose left ambiguous: `resolve()` turned out
to be genuinely unreachable in normal operation, not just trivial — `graph.rs`'s `resolve_file`
only ever calls the *claiming* adapter's own `resolve()` over *that file's own* `facts.imports`,
never a fan-out to every registered adapter the trait doc comment's wording suggests. Since
JSON's `extract()` never populates `imports` (JSON has no import syntax), its `resolve()` is
dead code that exists only to satisfy the trait — worth recording as the mechanical
confirmation of how cross-language target resolution actually works (the *importing* language's
own resolver finds the target file directly, e.g. JS-TS's `resolve_relative` matching `.json`
paths against the discovered-file index itself), since this is the first adapter where the
distinction is observable.

The one piece of real design work was `claim()`'s cross-adapter exclusion list: `package.json`
is genuinely, unambiguously JSON syntax, so `**/*.json` would otherwise double-claim JS-TS's
own manifest — a different shape of the "manifests are not claimed" bug from Swift's
`Package.swift` (same adapter claiming its own manifest as source) since here it's a *different*
adapter's manifest a generic glob would swallow. `tsconfig.json` is excluded pre-emptively too,
per RFC 0002 §3 naming it as JS-TS's eventual manifest even though JS-TS hasn't implemented
`tsconfig.json` parsing yet — confirmed by checking `kndo-adapter-js`'s own manifest_globs and
resolution.rs doc comments directly rather than assuming the RFC prose was current. No tree-
sitter grammar (ADR 0002's explicit escape hatch): `serde_json` validates in one call, with
nothing else worth walking a syntax tree for. Two conformance fixtures, run with `JsonAdapter`
*and* `JsTsAdapter` together — the first genuinely mixed-language fixture in the corpus, since
a JSON-only fixture could never demonstrate the cross-language claim this adapter exists for.

**Effect on kndo's own dogfood**: two `.json` files newly become claimed, hence newly visible
to `unused` — `docs/perf-baseline.json` and `schemas/*.json`, both reached only through a Rust
`PathBuf::join("...")` call at runtime (`xtask`/`kndo`'s own test suite), invisible to static
analysis by construction (RFC 0002 §5). Real pre-existing debt, now visible, same as the
Kotlin session's own `comment_openers` self-reference fix — not something this adapter set out
to remediate, and (confirmed while investigating) not something `kndo.toml`'s documented
`[[rule]]` path-override mechanism can silence yet either, since no parser for it exists
(docs/adapters/json.md §2). Left as-is rather than papering over with a change that does
nothing.

### M5 progress — CSS adapter ✅ (landed 2026-08-21)

`kndo-adapter-css` per docs/adapters/css.md: RFC 0002 §3's other "non-source language," but a
genuinely narrower slice of what that section's prose promises than JSON turned out to need —
"symbols are selectors/mixins/variables... class-name usage from JS/TS/HTML... enables 'unused
CSS rule' as a normal finding" describes a mechanism that structurally doesn't exist yet. Traced
directly through `reachability.rs` before writing any extraction code: (1) no adapter extracts
`className`/CSS-Modules references today (checked `kndo-adapter-js/src/extraction.rs`
directly — that's plugin territory, RFC 0003, not this adapter's), so a class selector's real
consumers are invisible; (2) the tempting compensation — root every selector, since kndo can't
see its real usage — was checked against the CSR construction and rejected: every symbol
carries an implicit symbol→file edge (the "module-load rule"), so rooting a selector would make
its *owning file* permanently reachable, silently disabling the one CSS finding this adapter
*can* deliver honestly (an orphaned `.css` file nothing imports). Selector/class extraction
stays out of v1 entirely rather than shipping either a false-positive flood or a broken
file-level check — recorded as the adapter's own central open question, not a gap discovered
by a fixture and patched around.

What v1 does cover, fully verified: file claiming (same mechanism as JSON — `ResolveCtx`'s
known-files index already resolves a `.css`/`.scss` import without any adapter existing;
claiming is what makes the resulting `FileNode` visible to `unused` at all), the `@import`
graph between CSS files (entirely CSS-internal, no cross-language blindness), and custom
properties/`var()` (`SymbolKind::CssVariable`, already present in the shared vocab — the one
symbol kind whose real consumers are, in the common case, other CSS in the same project, not
JS/HTML). Mid-spec, on request, SCSS support was folded in as a "flavor" rather than deferred to
a separate crate: `tree-sitter-scss` turned out to be a strict grammar *superset* of
`tree-sitter-css` (verified directly — `declaration`/`property_name`/`import_statement`/
`call_expression` all parse identically in both), so one shared extraction walker, dispatched
purely on node *kind*, handles both grammars — `$variable` declarations/references (same
`SymbolKind::CssVariable`), `@mixin`/`@function` (`SymbolKind::Other("mixin")`/`SymbolKind::
Function` — the latter's *invocations* resolve for free through the same generic
`call_expression`-to-`Call`-reference handling `var()`/`url()` already needed, no SCSS-specific
resolution code required), and `@use`/`@forward` (Sass's module system, alongside plain
`@import`, resolved through one unified candidate-list algorithm rather than two — extraction
never tags which at-rule produced a specifier, so resolution doesn't need to branch on it
either).

Two real bugs surfaced by writing the extraction tests, not by inspection — both variants of
the same root cause: `tree-sitter`'s `Node::children()` walks *every* child, anonymous
punctuation tokens included, not just named ones (`to_sexp()`'s dump hides anonymous nodes
entirely, which is what made the ground-truth probes look deceptively simple). `var(--brand)`'s
first-argument lookup grabbed the literal `(` token instead of the argument, silently emitting
no reference at all — fixed by filtering for `.is_named()`. Separately, `tree-sitter-css`'s
`string_value` wraps a `string_content` child excluding the quotes, but `tree-sitter-scss`'s
`string_value` has *no* such child — its own text *is* the quoted string — so `@use`/`@forward`
specifiers extracted as empty/missing until the string-value reader learned to fall back to
trimming quotes off the node's own text when `string_content` is absent. Both caught by
conformance-style unit tests failing loudly (empty reference/import lists), not silently wrong
output. Two upstream `tree-sitter-scss` 1.0.0 grammar bugs were also found and documented rather
than worked around, same posture as the Kotlin session's tree-sitter-kotlin-ng issues:
`@use "x" as y;` and `@extend %placeholder;` both produce `ERROR` nodes (`has_error()` verified
directly for each) — the former still recovers its specifier correctly from the surviving
partial tree, the latter is moot since `@extend` was already out of scope alongside `composes`.

Four conformance fixtures, one carrying the whole SCSS increment — every one of them needed a
JS-TS `import` to root the graph at all, since CSS declares no roots of its own (RFC 0002 §7).
Designing them surfaced the same-file-only resolution scope's real implication twice: an
initial draft had `var(--used)`/`$brand`/`@include flex-center`/`double(4px)` referenced from a
*different* file than their declaration (mirroring how a human would naturally write cross-file
CSS), which — correctly, per the adapter's own documented scope — never resolves; both fixtures
were redesigned to reference from within the declaring file itself, the same constraint the
spec's §2/§7 already named as a documented non-goal, now confirmed by the harness rather than
just asserted in prose.

### M5 progress — WASM plugin/adapter ABI ✅ (landed 2026-08-21)

`kndo-plugin-api` per `docs/contracts/wasm-abi.md`: the WASM component-model tier ADR 0003
promised, shipped for `LanguageAdapter` — a deliberately scoped-down **v1** (adapter-only, no
`Plugin` hooks; no manifest/resolve, `ResolveCtx` host-imports, visibility ladder, or byte
content — the full list, and why each is a real cut rather than an oversight, is
`wasm-abi.md` §2). The scoping question that mattered most: whether v1 needed to be
bidirectional (host-import callbacks for `resolve()`'s `ResolveCtx` queries) or could stay
one-directional (guest exports only). Cutting `resolve()`/manifests entirely — the host
answers all three trivially without ever calling the guest, the same posture JSON/CSS already
document for their own non-applicable trait methods — kept v1 one-directional, which is what
let the reference guest target plain `wasm32-unknown-unknown` with zero WASI: the sandbox
("no ambient fs/net", ADR 0003) becomes a property of the compilation target itself, not a
policy the host has to enforce and hope holds.

Toolchain-wise: `wit-bindgen` (guest codegen) and `wasmtime::component::bindgen!` (host
codegen) both read the same `.wit` file at compile time, no `cargo-component` install
required; componentizing a plain core `wasm32-unknown-unknown` module into an actual
component binary uses the `wit-component` crate as a library
(`ComponentEncoder::default().module(bytes)?.encode()?`) — also no external CLI, which
matters because a v1-conformant guest (no WASI imports to satisfy) needs no adapter shim
either. `wasmtime` is pinned to `27.0.0` (the latest version this workspace's Rust toolchain
can build; `48.0.0` requires a newer rustc) — ADR 0003's own "versioned and conservative from
day one" discipline applied to its runtime dependency, not just the WIT surface.

`examples/kndo-plugin-demo` is the reference/compliance adapter: a deliberately invented toy
language ("kdemo"), hand-scanned with a small lexer rather than a real grammar — pulling
tree-sitter's C sources across the `wasm32-unknown-unknown` boundary would have been a much
bigger yak than this demo exists to shave, and nothing about the ABI requires a WASM adapter
to use tree-sitter at all (that's an ADR 0002 choice for *native* adapters). It lives outside
the cargo workspace (`exclude`d, the same convention `spikes/perf` already uses) so "not
in-tree" is structural, not a promise: it cannot be statically linked into `kndo` by accident.
`kndo::open` auto-discovers `.kndo/plugins/*.wasm` (RFC 0003 §3's stated convention, no config
parser needed — zero-config by default like every other discovery mechanism in the product),
feature-gated (`external-adapters`, on by default, droppable for a minimal static build per
ADR 0006).

Compliance is two real, always-fresh (nothing checked in as a binary) end-to-end tests:
`kndo-plugin-api/tests/compliance.rs` drives a loaded `WasmAdapter` against a hand-built
`Engine`; `kndo/tests/external_adapter.rs` goes through the *full* product composition —
`kndo::open`, `.kndo/plugins/` discovery included — the exact call every `kndo-cli` command
makes, closing the exit criterion literally rather than by analogy. Both build the demo guest
from source and componentize it in-process on every run, then assert a real `unused` finding
(a genuinely dead function) comes back correctly through the real reachability engine while a
called one doesn't — proof the ABI carries real graph facts, not just that it links.

**Exit:** all eight launch languages pass conformance — **met** (§ per-language progress
notes above); a third-party demo adapter (not in-tree) runs against the released binary —
**met**, `kndo/tests/external_adapter.rs`; budget still holds with all adapters active — **met
by construction**: `.kndo/plugins/` discovery is a single `read_dir` that's a no-op when the
directory doesn't exist (every existing benchmark/fixture project), and no per-adapter
overhead was touched.

**Carried out of M5, not required by its stated Exit criteria:** "First-party ecosystem
plugins for detected frameworks" (M5's own header names it, but it isn't one of the three
bullets Exit actually checks) and the `Plugin` trait's own WASM bridge (`wasm-abi.md` §2) —
both real M5-scoped ideas, neither built. Parking lot until a first ecosystem plugin (or real
external-plugin demand) picks a concrete shape to build toward, same "don't build the
mechanism before the demand" call this session made for `resolve()`'s host-imports.

## M6 — 1.0 hardening
False-positive hunt across dogfood corpus (target < 2%, vision §6), schema/ABI freeze, docs site,
install channels (brew/cargo/npm shim/curl), **`kndo-action` GA** (RFC 0010: sticky PR comment,
annotations, SARIF opt-in — dogfooded on kndo's own PRs from M2 via a pre-GA workflow),
`stale` (suppressions) rule, error-message polish.

**Exit:** semver 1.0 commitments declared for the three contract surfaces; two external repos
adopt kndo in pre-commit and stay enabled for 2 weeks.

## Post-1.0 parking lot
`kndo clean` (guided auto-removal), watch mode / LSP, custom analyses over a stable graph API,
`kndo serve` exposing the navigation verbs 1:1 as MCP tools (RFC 0007 §7), an arbitrary graph
query language, deep mode (compiler-grade resolvers), historical trend service, HTML report,
tsconfig project-references deep integration, deferred rules from the RFC 0005 §13 triage
(`layer-violation`, `oversized-unit`, `redundant-export-binding`) and ecosystem-plugin rules (`barrel-abuse`,
`dead-feature-flag`, churn×complexity hotspots via a git plugin, overlapping-dependency
knowledge base, config-key drift), and **divergent clones** — near-identical clones where one
copy got a fix the other didn't ("the bug you fixed here still lives there"), the natural
Type-3 extension of the winnowing index and the headline candidate for the first post-1.0
release.

## Standing rules

- A language lands only via the adapter contract; a core `if language == X` reverts the PR.
- Every milestone keeps the benchmark green from M2 onward — performance regressions block merge.
- Contracts change only with the corresponding doc updated in the same PR.
