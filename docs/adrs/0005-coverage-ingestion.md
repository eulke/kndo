# ADR 0005 — Coverage is ingested, never measured

**Status:** Accepted · **Date:** 2026-08-18

## Context
CRAP (RFC 0005 §10) needs per-function coverage. Measuring coverage requires *executing* tests —
incompatible with a < 500 ms static tool and with kndo's non-goal of being a coverage tool.

## Decision
kndo consumes existing coverage reports via `ingest_coverage` plugins (RFC 0003 §2). Launch
formats: **lcov** (lingua franca: jest/vitest/nyc, llvm-cov, gcov, Go via converters) and
**JaCoCo XML** (Java/Kotlin); **Cobertura XML** next. Reports are located by config or
well-known paths, matched to files by path, mapped to functions by line ranges.

Freshness policy: a report older than `max-age` (default 7 days) or referencing missing files is
ignored **with a diagnostic** — stale certainty is worse than declared uncertainty. Without
usable coverage, CRAP degrades as specified in RFC 0005 §10 (cov = 0, flagged `coverage: none`).

## Consequences
- kndo stays static and fast; teams get CRAP "for free" if any coverage already runs in CI.
- Coverage staleness/precision is inherited from the producer (line-level lcov ⇒ statement-level
  approximation of `cov(m)`); we report the source + age so consumers can judge.
- New formats are plugins — no core changes.

## Alternatives
Running tests to measure coverage (breaks budget and scope); requiring no coverage and using
complexity alone (loses the entire point of CRAP — the risk is complexity *times* untestedness).
