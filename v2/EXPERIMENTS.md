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
dimension. Measure when the v2 Rust adapter lands, against the harvested fixtures and
the oracle — not before.

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
