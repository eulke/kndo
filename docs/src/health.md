# Health & coverage

`kndo health` condenses the whole analysis into one deterministic 0–100 score, so a number
you improved last month still means the same thing today.

```console
$ kndo health
health   82.4  B   +0.4 ↑ from 82.0
  unused-symbols       ▃        −6.2  (47)
  unused-dependencies           −0.0
  unused-files         ▁        −1.1  (3)
  test-only                     −0.0
  duplication          ▄        −7.1  (8412 tokens)
  crap                 ▂        −4.0  (12, load 1912.4, coverage coverage-lcov coverage/lcov.info (2d old))
  cycles               ▁        −1.2  (2)
  internal-only                 −0.0
  untested             ▁        −2.0  (9)
```

## The formula

```text
health = 100 − Σ  weight(category) × min(ratio(category) / saturation(category), 1)
```

Each category contributes a penalty: its **ratio** (how much of the codebase carries the
verdict), pushed through a saturation curve — linear near zero, so small improvements always
move the score, and capped at the category's full weight, so a single terrible corner can't
zero the whole score.

| Category | Weight | Saturates at ratio | Ratio measures |
|---|---:|---:|---|
| `unused-symbols` | 25 | 0.25 | dead symbols / all symbols |
| `unused-dependencies` | 15 | 0.5 | unused deps / declared deps |
| `unused-files` | 10 | 0.25 | dead files / claimed files |
| `test-only` | 10 | 0.25 | test-only nodes / production-candidate nodes |
| `duplication` | 20 | 0.3 | duplicated tokens / all counted tokens |
| `crap` | 20 | 0.5 | functions above the CRAP threshold / all measured functions |
| `cycles` | 5 | 0.25 | files participating in reported cycles / claimed files |
| `internal-only` | 5 | 0.5 | over-visible symbols / visibility-bearing symbols |
| `untested` | 5 | 0.5 | test-blind production nodes / production nodes |

Grades: **A** ≥ 90 · **B** ≥ 80 · **C** ≥ 65 · **D** ≥ 50 · **F** below.

Honesty rules baked into the score:

- Health is computed from the codebase's state **before** baseline and suppressions are
  applied — the score measures the code, not how much of it you've acknowledged away.
  (Findings you baselined disappear from reports; they still weigh on health.)
- The `untested` category is skipped entirely when the project has no test roots — a repo
  without tests gets a diagnostic, not a guaranteed-saturated penalty.
- Cycle participation counts only cycles the language actually reports — a cycle the
  language's compiler forbids or the ecosystem treats as idiomatic isn't a penalty.
- Plugin findings never affect health — the score can't be moved by installing a plugin.

## Trend

Full runs store their score in `.kndo/health.json`; the next run shows the movement
(`+0.4 ↑ from 82.0`). In diff modes the "previous" side is computed from the before-tree, so
`kndo check --diff main` shows exactly what your change does to the score
(`health 82.0 ──▶ 84.1  +2.1 ↑`), with the distance to the next grade boundary called out on
drops.

## Per-package breakdown

```console
$ kndo health --by-package
...
by package:
  @demo/web                 91.0  A
  @demo/api                 74.2  C
```

`--by-package` groups the **same penalties** by owning package — never a different metric —
so the two views always agree. It appears when more than one package owns claimed files
(monorepos); a single-package project omits it.

`kndo health --format json` emits just the health object — `score`, `grade`, `previous`,
`categories[]` (each with `ratio`, `penalty`, and its count/tokens/crapload extras), and
`packages[]` — the same object embedded in `kndo check --format json` under `health`.

## Coverage ingestion

Coverage is **ingested, never measured**: kndo never runs your tests, it reads the reports
your test runner already produces. The built-in lcov plugin (`kndo:coverage-lcov`) activates
when a report exists at a well-known path:

```text
coverage/lcov.info
lcov.info
```

and feeds per-file, line-granular hit counts into the analysis. Rules:

- **Freshness**: a report modified more than **7 days** ago is ignored, with a diagnostic
  telling you to regenerate it — stale certainty is worse than honest absence. The report's
  provenance and age are echoed in the health output
  (`coverage coverage-lcov coverage/lcov.info (2d old)`).
- **Accumulation**: multiple records for one line (across test suites) accumulate — a line
  any suite ran is covered.
- Only the essential lcov records are read (`SF:` file sections and `DA:` line hits);
  function/branch records are ignored because kndo maps lines to functions itself, via each
  function's own span.

## From lines to functions: cov(m)

A function's coverage is the covered fraction of the **instrumented** lines inside its span.
Two situations are "coverage unknown", never "coverage zero":

- the file appears in no report;
- the span contains no instrumented lines (the instrumenter skipped the function).

What "unknown" means is each consumer's call — [`crap`](rules.md#crap) applies the
pessimistic reading (`cov = 0`, flagged `coverage: none` in the message), because an
unmeasured complex function is exactly the one you want surfaced.

## CRAP and the crapload

`CRAP(m) = comp(m)² × (1 − cov(m))³ + comp(m)` — see the
[`crap` finding](rules.md#crap) for semantics and fixes. The health table's `crap` row also
carries the **crapload**: the sum of scores above the threshold, a magnitude ("how much
risky, untested complexity is there") that the per-function count alone doesn't convey.

The practical playbook for a low score dominated by `crap`: wire your test runner to emit
lcov (`--coverage --coverageReporters=lcov`, `cargo llvm-cov --lcov`,
`go test -coverprofile` converted to lcov, …), drop the file at `coverage/lcov.info`, and
re-run — functions your tests already execute stop counting against you, and what remains is
the real risk list.
