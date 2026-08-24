# ADR 0005 — Coverage is ingested, never measured

**Status:** Accepted · **Date:** 2026-08-18

## Context
CRAP (RFC 0005 §10) needs per-function coverage. Measuring coverage requires *executing* tests —
incompatible with a < 500 ms static tool and with kndo's non-goal of being a coverage tool.

## Decision
kndo consumes existing coverage reports via `ingest_coverage` plugins (RFC 0003 §2). Shipped
formats (the `kndo-plugin-coverage` built-ins): **lcov** (lingua franca: jest/vitest/nyc,
llvm-cov, gcov), **Cobertura XML** (coverage.py, .NET, istanbul's cobertura reporter),
**JaCoCo XML** (Java/Kotlin), and **Go coverprofile** (`go test -coverprofile`); other
formats load as external WASM components through the `coverage-ingester` world. Reports are
located by config (`[plugins.<id>] report`, globs included for monorepos) or well-known
paths, matched to files by path (host-side root and package-table rebasing lands absolute
and module-qualified report paths), mapped to functions by line ranges.

Freshness policy: a report older than `max-age` (default 7 days; `[plugins.<id>] max-age`
overrides per plugin) is ignored **with a diagnostic** — stale certainty is worse than
declared uncertainty. (A report referencing missing files simply matches nothing — silence,
never a wrong file.) Without usable coverage, CRAP degrades as specified in RFC 0005 §10
(cov = 0, flagged `coverage: none`).

## Consequences
- kndo stays static and fast; teams get CRAP "for free" if any coverage already runs in CI.
- Coverage staleness/precision is inherited from the producer (line-level lcov ⇒ statement-level
  approximation of `cov(m)`); we report the source + age so consumers can judge.
- New formats are plugins — no core changes.

## Alternatives
Running tests to measure coverage (breaks budget and scope); requiring no coverage and using
complexity alone (loses the entire point of CRAP — the risk is complexity *times* untestedness).
