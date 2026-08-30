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
ripgrep 59, gin 1) plus private-type-leak 43 — the largest unbuilt category, bigger
than everything v2 reports today combined. Supply: three languages shipped on binary
`Reach` alone, and NONE of M4's false-positive fixes wanted a rung between private and
exported — every one wanted scope SHAPE: Go's package scope landed as
`ReferenceScope::Directory` (a capability, not a rung), and Rust's module-tree privacy
landed as bindings-keep-whatever-the-reach. That is the hypothesis confirmed early and
partially absorbed: the linear part of the ladder is what remains, its consumer is the
internal-only analysis, and it does not exist yet — so the ladder waits for it
(consumer rule; DECISIONS 2026-08-30 M4.d has the verdict).

The same design absorbs `Declaration.exported_as` (2026-08-30, M2): the export alias
belongs inside the exported side of the visibility type — today it rides beside
`Reach` as a parallel `Option` whose `Private`+`Some` combination is representable but
inert, a recorded shape debt by the repo's own "Reach for the type" bar. Folding it in
is a deliberate contract change: fingerprint moves, the pinned conformance reports
diff, DECISIONS gets the entry.

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

## Standing experiment infrastructure

- The corpus (`corpus/corpus.toml`) is the measuring instrument; keep it pinned.
- The oracle (`oracle/`) is the baseline; every emitting milestone diffs against it.
- An experiment's result — kill, defer, or build — lands in `DECISIONS.md` with its
  number, and killed entries move up into this file's killed section.
