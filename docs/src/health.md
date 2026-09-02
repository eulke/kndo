# Health and coverage

## Health

Health is the fraction of the judged graph that findings do **not** implicate,
printed with one decimal:

```text
health 90.0 · implicated 1 of 10 · unused 1
```

The denominator is every claimed file plus every declaration, plus every
dependency declaration the usage judgment counted. A subject counts once
however many findings land on it — a function that is both unused and
duplicated is one problem, not two penalties. Only first-party findings of
severity `warning` or worse count; `info` is the advisory tier, and extension
findings are advisory by construction.

What health ignores, on purpose: the baseline (acknowledged debt is still
debt), the clock (no history, no "previous run"), and the machine. What it
honors: `kndo:allow`, because a suppression is a human verdict overriding the
analysis.

`kndo health` prints the block alone and exits 0; `--by-package` splits the two
integers by the package that owns each subject, so a monorepo can see which
package carries the debt. The JSON envelope always carries both.

## Coverage

Coverage is run output — usually ignored by git — so the discovery walk never
sees it; the built-in ingesters read it from conventional paths, the first
report that parses *and* maps onto the project wins:

| Format | Producer examples | Paths read |
|---|---|---|
| lcov | vitest, jest, pytest-cov `--cov-report=lcov`, `cargo llvm-cov --lcov` | `lcov.info`, `coverage/lcov.info` |
| Cobertura XML | coverage.py, pytest-cov `--cov-report=xml` | `coverage.xml`, `cobertura.xml`, `coverage/cobertura-coverage.xml` |
| JaCoCo XML | the jacoco Maven and Gradle plugins | `target/site/jacoco/jacoco.xml`, `build/reports/jacoco/test/jacocoTestReport.xml`, `jacoco.xml` |
| Go coverprofile | `go test -coverprofile` | `coverage.out`, `cover.out`, `coverage.txt` |

A report speaks in its producer's path spelling — a Go profile keys by import
path, JaCoCo by package and source name, coverage.py by the path under a
source root — and kndo maps each entry onto the one project file it names; an
ambiguous spelling maps onto nothing rather than credit the wrong file.
Repeated entries for one file accumulate, so concatenated shards of a sharded
run mean more executions, never a replaced earlier shard.

With a report ingested:

- `untested` judges per **function**, `certain`, from function records where
  the format has them (lcov's `FN`/`FNDA`, JaCoCo's method counters) — a
  declaration line executes when its module loads, so line hits alone would
  call every loaded function tested — and from body lines where it does not.
- `crap` turns on: `cc² × (1 − coverage)³ + cc` per function, a finding at or
  above the threshold (`[analysis.crap] threshold`, 30 by default). Complex
  and barely tested is the change-risk signal; complex and fully tested scores
  its complexity alone.

Without one, `untested` falls back to the graph per file (`probable`) and
`crap` abstains — and says so.
