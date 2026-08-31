# Experiments — the measure-first backlog

Every candidate feature starts here with a question and how to measure it; the number
decides. Killed entries stay, with the number that killed them, so nobody rediscovers
and rebuilds them. Prior data comes from v1's recorded measurements in this repository.

## Killed by measurement (do not rebuild without new data)

### kndo:vite / kndo:rollup plugins
Question: do bundler configs anchor enough roots to justify plugins?
Measurement (v1): only 11 of 61 vite configs in the wild declare an entry;
`kndo:rollup` had no case at all. The HTML adapter replaced the intent.
Reopen only if: config-declared entries become the norm in the ecosystem.

### Markdown/docs adapter (path references in prose)
Question: can prose path references be claimed without noise?
Measurement (v1): 15 of 74 repository Markdown files are legitimately linked from
nothing; claiming `**/*.md` breaks the dogfood gate; exempting needs a `FileRole::Docs`
that does not exist. Two real broken links were caught instead by the `doc_links` test —
links only, prose paths carry too many false positives.
Reopen only with: a `FileRole::Docs` design plus a measured false-positive rate near zero.

### kndo:guava-testlib plugin
Measurement (v1): three measurements said there is nothing to build (recorded to prevent
rediscovery).

### HTML adapter shipping its headline rule alone
Measurement (v1): first cut was net-negative on vite (−237 unused, +418 additions, 145
of them `untested` on the .html files themselves — a pre-existing gap multiplied ~35×).
Fixed only by adding `declares_units_of_testing`. Lesson generalized: a new adapter's
first corpus run gates its ship, not its fixtures.

## Deferred with data

### Incremental analysis (recolor on dirty regions)
Measurement (v1): full recolor at 50k files costs 13 ms — incrementality is a luxury,
not a lifeline. Revisit only if an LSP latency budget demands sub-parse response, as its
own experiment.

### deep-import external-provider half
Blocked (v1): evaluating it requires the provider's own manifest, which lives outside
the discovered tree (`node_modules/` is not walked). Needs a design for external
manifest visibility before any measurement.

## Open candidates (never decided in v1)

### Dependency-hygiene family (undeclared / unresolved / version-skew) — measured 2026-08-31
Full demand was 543 dependency-subject oracle findings (the 410 above plus 198
deps-unused and 14 deps-test-only hiding inside `unused`/`test-only`). The corpus
experiment decomposed every one (COMPARISON has the per-finding record):
**`unresolved` and `version-skew` shipped** (vite 10 + 3, ripgrep 10, everything
else 0 — zero measured noise), carried by `DependencyDeclaration` evidence
(js-ts and rust rich; JVM/go/python/swift name-only until their version models
exist) and three resolver/claim fixes worth more than the analyses.
**`undeclared` deferred with its number**: 2–3 honest true positives on this
corpus vs a 124-finding fixture cliff + 195 ancestor-declared (workspace
hoisting) + ambient modules + self-imports — the model needs those classes AND
a corpus repo shaped like an ordinary application before it can be judged.
**deps-unused/test-only deferred**: config-driven tooling is invisible to
structural evidence (zero-FP unreachable without per-tool knowledge); go.mod
`test-only` is unactionable by construction (no dev section). Also recorded:
discovery does not follow symlinks (three vite findings live on that boundary —
following them is its own experiment if a corpus repo ever hinges on it).

### cyclic (import cycles) — SHIPPED 2026-08-31
The definition question resolved as a language capability:
`ExtensionSpec::import_cycles` (default Tolerated = silence; js-ts and python
declare Hazard). Corpus: vite 31 (incl. the real 88-file node tangle), flask 3
(the package's own 20-file knot — never measured by v1), everything else 0;
v1's JVM file-cycle findings retired as vice (multi-pass compilation makes
them routine legal structure). Package-level cycles shipped with the aggregation epic
(2026-08-31): both oracle findings retired as vices against the real pins —
guava's pair is one-directional (v1 manufactured the loop) and Exposed's
closes only through a test-scoped project dependency, which never blocks a
publish. Corpus zero, mechanism pinned by an engine test (prod mutual fires,
dev pair is silence).

### Python ancestor-package import edges
`import a.b.c` executes `a/__init__.py` and `a/b/__init__.py` on the way down —
the language's rule, but kndo:python emits no edges to the ancestors, only to
`a/b/c.py` itself (plus the submodule probes M6.b.4 added for from-import
bindings). No flask finding hinged on it — every ancestor `__init__.py` there is
reached by direct imports anyway — so the edges are not built. Build only when a
corpus repo shows a finding this changes; the emission point is one loop in
`imports()` next to the binding probes.

### deep-import — DEFERRED with its number 2026-08-31
The last oracle category: 6 findings, and every one is the monorepo's own
test or tooling package deep-importing vite's internals (`@vitejs/unit-ssr`
at 99 sites IS vite's unit-test suite doing its job; `create-vite`'s tsdown
config reaching for a type is tooling). v1's design was already careful
(contract-gated on a declared surface) and still produced only this. A
consumer-role gate — test/tooling consumers exempt, which any honest v2 build
would need — makes the corpus demand exactly zero, and unlike `unresolved`
(whose zero floor guards future renames everywhere) this rule's regression
value exists only in monorepos with declared surfaces and production-role
cross-package consumers, which the corpus lacks. Cost it would need anyway:
`declares_surface` package knowledge, package-pair subjects, consumer-role
gating. Build only when a corpus repo shows a production-role deep-import.

### crap (complexity × uncoverage)
Oracle demand 0 — corpus runs carry no coverage, so v1's own analysis never
fired there. v2 has both inputs (metrics winnowing, lcov ingestion); build
only with a measurement from a coverage-bearing corpus repo, as its own
experiment.

### hollow-test rule
A test that asserts nothing / covers nothing real. No decision recorded; needs a
definition that can reach zero FP and a corpus count.

### speculative-abstraction rule
Abstractions with a single implementer and no external consumers. Same bar: zero-FP
definition first, corpus count second.

### `test-only` default severity
v1 left it at info with the raise "available but with no decision behind it". Measure:
how many test-only findings on the corpus are actionable?

### Churn × complexity (git history plugin)
Roadmap idea. Needs: determinism story for history-derived data (history changes run to
run — likely advisory-only), and a corpus measurement of signal quality.

### Visibility-ladder shape (module-and-descendants scope)
v1's linear rung ladder could not express Rust's module-and-descendants privacy — a
recorded incident (the adapter "had to stop lying about private"; equal rungs anchored
in different files compared as equal regions). Candidate: rungs gain a scope-shape
dimension.

Measured at M4.d, when the Rust and Go adapters landed (the deferred-until moment).
Demand: 8,026 oracle findings across the corpus depend on ladder knowledge —
internal-only 7,983 (guava 7,014, vapor 383, Alamofire 216, Exposed 163, vite 147,
ripgrep 59, gin 1) plus private-type-leak 43 (SHIPPED 2026-08-31: vite 30 real, the
other 13 retired as vices — rung folding, test-support, Scoped comparisons) — the largest unbuilt category, bigger
than everything v2 reports today combined. Supply: three languages shipped on binary
`Reach` alone, and NONE of M4's false-positive fixes wanted a rung between private and
exported — every one wanted scope SHAPE: Go's package scope landed as what is now
`sees` (née `sees` — a capability, not a rung; it retired the interim `ReferenceScope`), and
Rust's module-tree privacy landed as bindings-keep-whatever-the-reach. That is the hypothesis confirmed early and
partially absorbed: the linear part of the ladder is what remains, its consumer is the
internal-only analysis, and it does not exist yet — so the ladder waits for it
(consumer rule; DECISIONS 2026-08-30 M4.d has the verdict).

The same design absorbs `Declaration.exported_as` (2026-08-30, M2): the export alias
belongs inside the exported side of the visibility type — today it rides beside
`Reach` as a parallel `Option` whose `Private`+`Some` combination is representable but
inert, a recorded shape debt by the repo's own "Reach for the type" bar. Folding it in
is a deliberate contract change: fingerprint moves, the pinned conformance reports
diff, DECISIONS gets the entry.

**Worked design (2026-08-31; SHIPPED as M6.c first half — see DECISIONS):** the
region, not the rung. New demand since the entry above: kotlin measured 19 unused vs
the oracle's 277 (public-by-default meets library-mode roots — only `private` is
individually judgeable), rust folds `pub(crate)` to Exported, and swift (M6.b.3) has
`internal` as its DEFAULT. A linear rung cannot compare "module" (kotlin) with
"package" (go) with "crate" (rust) without core learning what those words mean — the
coupling the ignorance rule exists to prevent. A REGION can: visibility for judgment
is not an ordering but the SET of files that could legally name the declaration.

Shape: `Reach` grows `Scoped { scope: SmolStr }` — the token is the adapter's own
word ("package", "module", "crate", "in:a::b"), core never parses it (the
`SymbolKind::Other` posture). The adapter grows one capability:
`scope_of(path, scope, cx) -> Option<files>` — path- and manifest-computable, never
content-dependent (the `sees` stability class, so the persisted graph can trust
it), `None` = unboundable, treated exactly as Exported (keep-alive). Engine judgment
becomes evidence-shaped, ignorant of every language word: Private pools over
file+unit; Scoped pools over its region, is NOT part of the `entry_surface` handed-out
surface (`unused.rs`), and namespace/glob importers keep it only from inside the
region; Exported unchanged. The same region machinery is what `internal-only` (the
7,983) needs to classify uses as inside/outside — one mechanism, two named consumers,
and it absorbs the recorded `exported_as` shape debt (the alias moves into the
non-Private variants, killing the representable-but-inert `Private`+`Some`). Cost:
`Reach` is documented closed-by-design, so this is a deliberate semantic contract
change — fingerprint moves, WIT variant + pin-abi toll, adapters bump when they start
emitting it. Conformance case: kotlin `internal` fixtures (unused internal accused;
cross-package-same-module use keeps it; `protected` stays Unknown→Exported).

**Post-ship residue (2026-08-31, M6.d):** the shipped regions serve every Scoped
rung (guava 3,305, Alamofire 268, vapor 85, Exposed 31, ripgrep 1 measured), which
leaves the demand's two-rung slice structurally unserved: vite's 147 are
`export`ed TypeScript symbols whose every use sits in their own file, where
removing the `export` is the language-checked narrowing — `internal-only` judges
`Scoped` declarations only, and TS has no Scoped rung. That is a DIFFERENT
analysis (export-narrowing over `Exported` declarations, likely also the residual
slice of rust's `pub`-vs-`pub(crate)`), with its own false-positive surface:
an Exported name is nameable from anywhere, so "all uses in own file" is a weaker
fact than region-enumerated absence. Zero-FP definition first, corpus count
second — the demand number is pinned here.

### Import-shape debt: the `use`-leaf pair (recorded 2026-08-30, M4)
A plain Rust `use` leaf emits TWO import records over the same target: a
`Bindings` (per-item precision — what keeps a private a child module legally
imports) and a `Namespace` (the whole-surface keep that survives alias hops the
resolver cannot follow). Two records to express one statement is the "two fields
that must agree" smell by this repo's own bar — the honest shape is one variant
carrying both meanings (a named binding that also keeps the surface). Folding it
is a deliberate contract change (fingerprint moves, adapters and the pinned
conformance reports diff), so it rides the next planned import-shape change
rather than moving the contract twice in one milestone. Two smaller ledger notes
from the same audit, each already stated at its code site: Go declares no methods
(structural interfaces), so `duplicate` cannot see Go method-body clones — an
under-report to measure if a corpus ever suggests it matters; and the Rust
adapter's unimportable-crate-root and single-file-crate checks are path-segment
conventions (`src/bin`, `tests/`, `examples/`), the only knowledge available at
their layer — a module literally named `tests` inside `src/` would be misread,
in the fewer-keeps direction.

### Type-3 clones (divergent copies)
Winnowing covers Type-1/2 today. Measure recall/precision of a Type-3 extension on the
corpus before designing anything.

### LESS / JPMS / KMP / Windows-target support
Each is an adapter-capability or platform experiment; Windows enters the release table
only after the CI matrix compiles it (the v1 lesson: it shipped in the target table with
nothing ever building it, and died on a grammar's build script).

### RootKind::Docs (a fourth reachability color)

Candidate: documentation is neither production, test nor tooling. A `docs` root
kind would let doc-example code contribute liveness WITHOUT hiding dead code:
today references inside doctests/KDoc/DocC examples are invisible (symmetric
with v1 — ripgrep parity holds), and if an adapter ever parsed them as plain
references, an export used only by its own examples would go silently alive.
Under a docs color it becomes a `docs-only` finding instead — test-only's
sibling. The same color is what a docs adapter would root, if one ever earns
revival (the Markdown adapter above is killed with data; that verdict stands).

Demand today: none measured — no oracle category is docs-shaped and no corpus
false positive asks for the color. Consumer rule, the ladder's own precedent:
the color waits for its analysis, and a RootKind variant nothing emits and
nothing reads is the "commented-out config key" the release law forbids.

Why the entry exists anyway — a timing constraint: `root-kind` is a WIT enum
and the ABI freezes at the first release. Adding an enum case is cheap before
the freeze and a versioned evolution after. At the freeze (M6.e) this becomes
a forced decision: either an experiment by then justifies the variant, or the
set {production, test, tooling} is declared closed and docs-liveness, if it
ever comes, arrives by another mechanism.

## The v1 surface ledger (owner directive, 2026-08-31)

v1's shipped surface is a floor: every capability it offers is either present in v2,
carried here with a disposition, or dead with its vice named in `DECISIONS.md`. Nothing
falls off quietly. The renders row landed first (the directive's own example); the rest
is inventory awaiting its turn — an `open` disposition is a claim of value, not a
commitment to v1's design: each item is rebuilt v2-native when it lands.

| v1 surface | v2 state | disposition |
|---|---|---|
| formats: human, json, agent, sarif + flag > `KNDO_FORMAT` > tty selection | shipped | the render toll (2026-08-31): `--format`, agent format 1, SARIF with byte regions |
| line/column in findings and output | lines shipped (2026-08-31): `Finding.lines`, `path:line` in human/agent, SARIF `startLine`/`endLine` | resolved per run from in-memory contents — no cache or graph format learned about lines, and identity never includes them; columns stay deliberately absent (SARIF counts them in UTF-16 units — a slightly-wrong column is worse than none) |
| `--staged` / `--diff <ref>` change-scoped runs | shipped (2026-08-31): two full analyses over two pinned trees, composed | the engine never learned git — the CLI materializes the base (and, for staged, the index) via `git archive`, and `Snapshot::against` rides the baseline mechanism; envelope carries `run.mode` + `base_health` (a pure function of the pinned base tree, not v1's cross-run `previous`) |
| health (score + per-category breakdown) | model + verb shipped (2026-08-31) | derived ratio, no weights (see DECISIONS); `kndo health` prints the block alone (line on a tty, JSON piped, always exit 0 — measurement, not a gate); `--by-package` shipped 2026-08-31 — the same two integers partitioned by directory-truth ownership, parallel same-named trees told apart by manifest |
| navigation verbs (`find`/`describe`/`uses`/`used-by`/`trace`/`impact`, batched `query`) | all seven verbs shipped (2026-08-31) | one contract (`kndo-query/1`, `Snapshot::query`) answered from the SAME index `unused` judged on — gate-certified: used-by empties exactly where unused accused; `trace` proves liveness (rooted file → import hops with recorded confidence → in-file keeper), `impact --if-deleted` simulates the removal as typed reachability flips, never fabricated findings; serve carries one MCP tool per verb over the same `Request`, answering in the agent grammar from a held session; the directed pair ships as `trace <from> --to <target>` (v1's positional pair would break 1:1 inputs→results; its `--all --max-paths` enumeration is dead — speculative surface, no consumer or measurement); JSONL batch stays out (serve IS the batch amortizer, and it ships) |
| `explain <id>` (evidence chain behind one finding) | shipped (2026-08-31) | finding brief + the full description of its subject — for `unused` the empty keeper preview IS the why; deepens per category as analyses grow evidence |
| `doctor` (what kndo sees: extensions, cache, config) | shipped (2026-08-31) | the real composition (WASM load failures included), config as parsed (a broken kndo.toml is doctor's diagnosis, never its crash), cache and baseline as filesystem facts; two facade doors opened for it (`Session::extensions`, `Session::composition_diagnostics`) |
| `init` + `kndo.toml` + pre-commit hook | shipped (2026-08-31) | `[check]` fail-on/format/only/skip — only keys with living consumers; a typo REFUSES the run (`deny_unknown_fields`); one merge site (flag > `KNDO_FORMAT` > file > default); `init --hook` writes a pre-commit running `kndo check --staged`; template↔struct held one-list by a test |
| `plugin` manager verb (install/list/new/build/wit/verify) | absent — `.kndo/plugins/` loads, nothing manages | open — authoring exists (`docs`), management waits for demand |
| `--only` / `--skip` / `--strict` category filters | `--only`/`--skip` shipped (2026-08-31) as judgment scope — the unselected analysis never runs, `judged` shrinks, health follows | `--strict` dead: it promoted `undeclared`, a category v2 has not built — the dependency family carries it (DECISIONS) |
| `--quiet` / `--verbose` / `--color` | shipped (2026-08-31) | human-render options only (elsewhere they warn and change nothing): quiet = the one verdict line, verbose = the phases line from beside-the-report timings, color = flag > `NO_COLOR` > tty with a semantic palette (severity, clean line, arrow DIRECTION — never an absolute score); v1's verbose reveal-of-hidden-tiers has no successor because v2 hides no tier |
| SIGPIPE default disposition (`kndo \| head` exits like a filter) | shipped (2026-08-31): SIG_DFL restored in the CLI binary, asserted-overflow test pair | v1's reasoning adopted as v2's own judgment — piped-by-design output makes the filter convention correct; serve is a protocol conversation, not a filter, and keeps clean error-propagation |

## Standing experiment infrastructure

- The corpus (`corpus/corpus.toml`) is the measuring instrument; keep it pinned.
- The oracle (`oracle/`) is the baseline; every emitting milestone diffs against it.
- An experiment's result — kill, defer, or build — lands in `DECISIONS.md` with its
  number, and killed entries move up into this file's killed section.

### Honesty-channel gaps recorded by the 2026-08-31 audit round

Conduct traps now land on the contribution (`ConductSink::note` → `dropped`), but two
sibling calls still degrade silently: a trapped `ingest` returns `None`
(indistinguishable from "report unparseable"), and a trapped `manifest_dependencies`
returns no names (activation stays off — safe direction, wrong silence). Both want the
same shape: a bridge note on something report-visible. The ingest one also wants the
winning ingester's coordinate on the run. Waiting on a channel design, not on demand.

### Declared-but-unjudged evidence (consumer rule, recorded 2026-08-31)

`Reference.kind` (six variants every adapter computes), `Import.confidence` (always
`Certain`), and root confidence (read only by dedup ordering) currently have no
analysis consumer. Kept, not cut: `RefKind::TypeUse` is `private-type-leak`'s input
and `Extend` is dispatch analysis's, both censused; cutting and re-adding would churn
the fingerprint twice for nothing. The rule stands: the next analysis that wants
confidence-carried-through starts by naming one of these as its input (the
`test_only.rs` comment about carried uncertainty becomes true then, not before).

### Reflection-driven dispatch conventions (recorded 2026-08-31, M6.c)

Caliper benchmarks, NullPointerTester fixtures, JUnit-3-style `testXxx`: methods
whose only caller is a framework that finds them reflectively by naming
convention. Statically they are dead, and kndo reports exactly that (guava's
+251 package-private members after the member-surface rule). The missing fact is
FRAMEWORK knowledge — which conventions a runner dispatches on — which is
conduct-plugin territory (`contribute_roots` with `ManifestDependency`
activation), never adapter or core territory: a `kndo:caliper` or `kndo:junit`
plugin would anchor declaration roots on the convention and those findings
disappear for projects that actually depend on the framework. Same family:
UIKit storyboards instantiate view-controller TYPES by name at runtime
(Alamofire's `Example/` app) — a `kndo:uikit` conduct plugin's FileExists
activation on `*.storyboard` could root the named classes. Demand: the guava
slice above. No plugin is built until an experiment shows the roots land on real
corpus findings — and no finding is suppressed core-side in the meantime just
because v1 happened to ship the same false positives.
