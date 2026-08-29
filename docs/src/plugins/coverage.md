# `kndo:coverage-lcov` · `kndo:coverage-cobertura` · `kndo:coverage-jacoco` · `kndo:coverage-go`

**Status:** Shipped, built-in, all four always-on ·
**Implements:** `ingest_coverage` ·
**Crate:** `crates/kndo-plugin-coverage` (one plugin type per format, one crate)

One document for four plugins because they are one design decided once and spelled four times
— and reading them apart is how the shared rules below get re-litigated per format.

## The gap

`crap` is complexity × untestedness. Without a coverage measurement the untestedness factor
would be a guess, so the analysis **abstains** rather than scoring — and zero `crap` findings
then means *unmeasured*, not clean. Resolving that confusion is the whole job of these four:
each turns "unknown" into a measurement, and none of them puts a single fact in the graph.

## Why they are plugins and not core

The ignorance rule covers report formats as much as languages: core owns the
format-neutral model (`kndo_core::coverage`) and the `ingest_coverage` hook, and knows lcov no
better than it knows Go. Formats kndo does not ship arrive through the same hook as external
WASM components — these four are simply the ones compiled in.

## The rules, shared by all four

1. **Always-on** (`activation: vec![]`). A `FileExists` gate would deactivate the plugin at
   composition time exactly when `kndo.toml` points `[plugins.<id>] report` at a path that is
   not well-known — composition is structural and never sees config. The cost on a
   report-less project is a handful of `stat` calls.
2. **`mutates_graph() == false`**, without exception. An ingester contributes no roots, edges
   or annotations; declaring otherwise would silently disable the graph-snapshot cache and
   incremental patching for every project kndo runs on, since these four are registered
   unconditionally.
3. **The subset that matters, and no more.** Line → hit-count facts only. Branch, function and
   method records are ignored in every format: kndo maps lines to functions itself, through
   symbol spans it already has.
4. **A parser normalizes only as far as it can know.** Separators and `./` prefixes, yes; the
   project root, no — the host alone knows that (`CoverageMap::rebase`). Where the true source
   root is ambiguous the parser emits *candidate* keys (JaCoCo's package/sourcefile split is
   the case) and lets exact-path lookup decide. A key matching no graph path matches nothing:
   silence, never a wrong file. Even a post-rebase collision only accumulates hit counts,
   which every consumer reads as `hits > 0`.

## Well-known paths

Config **overrides** these, it does not extend them: `[plugins.<id>] report = "…"` replaces
the list for that ingester.

| Plugin | Paths |
|---|---|
| `kndo:coverage-lcov` | `coverage/lcov.info`, `lcov.info` |
| `kndo:coverage-cobertura` | `coverage.xml`, `cobertura.xml`, `coverage/cobertura-coverage.xml` |
| `kndo:coverage-jacoco` | `build/reports/jacoco/test/jacocoTestReport.xml`, `target/site/jacoco/jacoco.xml`, `jacoco.xml` |
| `kndo:coverage-go` | `coverage.out`, `cover.out` |

lcov is the lingua franca (jest/vitest/nyc, llvm-cov, gcov, and Go through converters), which
is why it is first and why a project with any other toolchain usually needs no config.

## Proven by

`crates/kndo/tests/builtin_plugin_proofs.rs`, one test per format over one shared fixture:
without a report `crap` **abstains**; with a report at that format's own well-known path it
judges. The abstention is the assertion — for an ingester it is the only observable delta,
since the contribution record is empty by design (rule 2), and the test asserts that too.

## What they do not do

- **No branch coverage**, in any format. Nothing in kndo consumes it today, and a parser that
  reads a field no analysis uses is a maintenance cost with no signal.
- **No merging across formats by preference.** All four ingest; the map accumulates. A project
  that emits both lcov and Cobertura for the same lines gets the sum, which every consumer
  reads as covered.
- **No inference of a missing report.** A project with no report is unmeasured and says so.
