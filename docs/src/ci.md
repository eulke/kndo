# CI: the GitHub Action

The first-party Action runs kndo on pull requests and reports where reviewers already look —
a sticky PR comment, file annotations on the diff, and the job summary — gating the merge on
the same rules as the local pre-commit hook.

```yaml
name: kndo
on: pull_request
permissions:
  contents: read
  pull-requests: write        # the sticky comment; drop it to run comment-less
jobs:
  kndo:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0      # kndo needs the merge-base resolvable
      - uses: eulke/kondo/action@v1
        with:
          fail-on: warning
```

## Inputs

| Input | Default | Meaning |
|---|---|---|
| `fail-on` | `warning` | fail the check on findings at/above this severity: `error` \| `warning` \| `info` \| `none` (report-only) |
| `comment` | `true` | maintain the sticky PR comment (one living report per PR, edited in place) |
| `sarif` | `false` | also upload a SARIF rendering to GitHub code scanning (needs `security-events: write`) |
| `version` | `latest` | kndo version to install: a release tag like `v0.1.0`, `latest`, or `source` to build from the checked-out repository |
| `working-directory` | `.` | project root to analyze |
| `github-token` | the workflow token | token used for the sticky comment |

Outputs: `exit-code` (kndo's own: `0` clean, `1` findings at/above `fail-on`, `2` kndo failed
to run) and `report` (path to the JSON report file).

## What a run does

1. **Installs kndo** — downloads the release binary for the runner platform (Linux
   x86_64/aarch64, macOS x86_64/arm64), or builds from the checkout with `version: source`.
2. **Restores the analysis cache** (`.kndo/cache`) via `actions/cache`. Cache entries are
   content-addressed, so a stale restore is safe by construction — worst case a colder run,
   never a wrong one.
3. **Runs the diff analysis** — on a PR, `kndo check --diff origin/<base> --fail-on <fail-on>
   --format json`: the PR's merge-base against the base branch, identical semantics to a
   local `kndo check --diff`. Outside a PR context it falls back to a full check.
4. **Publishes**:
   - **one** sticky comment per PR, upserted in place on every push — never a pile of stale
     reports;
   - inline file annotations for new findings at warning and above;
   - the full report in the job summary, always;
   - fixed findings always render — deleting dead code should feel like a win in review too.
5. **Gates.** Exit `1` fails the check with "findings at or above the fail-on threshold";
   exit `2` fails it as **infrastructure** ("kndo failed to run"), never as "code has
   findings" — a typo'd base ref or a download failure can never pass vacuously *or*
   masquerade as a code problem.

Fork PRs degrade, never fail: without a write token the comment is skipped with a notice in
the summary; annotations and the summary still publish.

## SARIF / code scanning

```yaml
      - uses: eulke/kondo/action@v1
        with:
          sarif: true
permissions:
  security-events: write
```

With `sarif: true` the Action renders the same analysis as SARIF (a second render over the
warm cache — no re-analysis) and uploads it under the `kndo` category. Findings then appear
in the repository's code-scanning UI with kndo categories as rules, severities mapped to
SARIF levels, and evidence chains as related locations. You can produce the same file
anywhere with `kndo check --format sarif`.

## Budgets for the change

`--fail-on` judges each new finding by severity. Budgets judge the change *in aggregate* —
how far health moved, how many more findings there are than before — and the two compose
with OR: the run exits `1` when either gives way.

```toml
[delta]
max-health-drop = 0.0     # this PR may not lower the score
max-net-findings = 0      # pay for what you dirty: new − fixed ≤ 0

[delta.budget]
defect = 0                # and never a new defect, whatever else it fixes
```

Budgets only exist in diff modes (`--staged`, `--diff`), and only when the section is
written — see [Configuration](configuration.md#delta) for the full semantics. Every run
that evaluated them reports what it measured, so a red build says how much work is left
rather than only that it failed:

```
budget: fail (2/3) | health-drop<=0 ok -1.7 | net<=0 FAIL 1 over-by 1 | defect<=0 ok 0
```

The same block is `"budget"` in the JSON envelope, with `over_by` on the rules that broke.

## The local half: the pre-commit hook

CI is the second line. The first is the hook `kndo init --hook` installs:

```sh
#!/bin/sh
exec kndo check --staged --fail-on warning
```

It runs on what `git commit` would actually commit (the index, not the working tree), in
well under a second warm, and fails the commit on new warnings — so the PR comment is
usually already clean. The hook and the Action gate on the same rules by design; there is
nothing CI knows that the hook doesn't.

## Any other CI system

The Action is a thin frontend. The whole integration is:

```console
$ kndo check --diff "origin/${BASE_BRANCH}" --fail-on warning --format json > report.json
```

plus whatever your platform does with a JSON file and an exit code. The versioned JSON
envelope is the contract (`schema_version` semver: additive = minor, breaking = major);
`--format sarif` covers platforms that speak SARIF natively. Cache `.kndo/cache` between
runs if you can — it is content-addressed and safe to restore stale.
