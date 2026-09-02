# Getting started

Run it at the root of a repository:

```text
$ kndo
warning unused scripts/orphan.ts: no root anchors this file and no reachable file imports it
1 finding
health 90.0 · implicated 1 of 10 · unused 1
abstained: test-only — no test root anchors any file in this graph
abstained: untested — no test root anchors any file in this graph
abstained: crap — no coverage report ingested this run
```

Each line is one finding: severity, category, subject (with a line where the
subject has one), and the message that states the evidence. The verdict line
counts them; the health line is the ratio the findings implicate; the
abstentions are what kndo could not judge and why — here, a project with no
tests and no coverage report.

The exit code is the gate: 0 when nothing reached `--fail-on` (default
`warning`), 1 when something did, 2 when kndo could not run. Pipe it and you
get the JSON envelope instead of the human render:

```sh
kndo check --format json > report.json
```

## Ask why

Every finding is a claim about the graph, and the graph answers questions:

```text
$ kndo trace scripts/releaseUtils.ts
[scripts/releaseUtils.ts] file · tooling-only
  from tooling root scripts/detect-release.ts
  → scripts/releaseUtils.ts via import (certain)

$ kndo used-by scripts/orphan.ts
[scripts/orphan.ts] file · unreachable
  kept by: nothing

$ kndo explain kndo-ea8c083aa403
[kndo-ea8c083aa403] warning unused · scripts/orphan.ts · certain
  no root anchors this file and no reachable file imports it
[scripts/orphan.ts] file · unreachable
next: kndo used-by scripts/orphan.ts · kndo trace scripts/orphan.ts
```

`trace` is the liveness proof: a root, then the hops, each with the confidence
of the edge. `used-by` is the deletion question. `explain` takes a finding id
from the report. All seven verbs are in [Navigation](navigation.md).

## Settle in

```sh
kndo init            # writes kndo.toml with every default shown
kndo init --hook     # …and a pre-commit hook running `kndo check --staged`
kndo baseline        # accept today's findings; from now on, only what is new
```

Add `.kndo/cache/` to `.gitignore` and commit `.kndo/baseline.json`. The cache
makes warm runs cheap and never changes the output; the baseline is the team's
record of accepted debt, and [health](health.md) ignores it on purpose.

## Sharpen it with coverage

Leave a coverage report from your test run at one of the paths kndo reads —
`lcov.info`, `coverage/lcov.info`, `coverage.xml`, `target/site/jacoco/jacoco.xml`,
`coverage.out` — and the next run judges `untested` per function from what
executed, and turns on `crap`, the complexity-times-untestedness signal. See
[Health and coverage](health.md).

## In a pull request

The [GitHub Action](ci.md) runs `kndo check --diff <base>` and publishes the
change-scoped report — what the pull request introduces and what it fixes — as
a sticky comment, annotations and the job summary, failing on the same rule as
the hook.
