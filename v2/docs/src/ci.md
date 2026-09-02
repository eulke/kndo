# Continuous integration

## The GitHub Action

```yaml
name: kndo
on:
  pull_request:
  push:
    branches: [main]

permissions:
  contents: read
  pull-requests: write      # the sticky comment
  security-events: write    # only with sarif: 'true'

jobs:
  kndo:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: eulke/kondo/action@main
        with:
          fail-on: warning
```

On a pull request the run is `kndo check --diff <base branch>`: the report is
what the pull request introduces and what it fixes, never the whole tree's
backlog. On a push it is a full run. The Action installs the release binary for
the runner's platform, restores `.kndo/cache` between runs, and publishes the
result where reviewers look: a sticky comment upserted in place (one living
report per pull request), file annotations, and the job summary. The step
itself fails on the same rule as the local pre-commit — findings at or above
`fail-on`.

| Input | Default | Meaning |
|---|---|---|
| `fail-on` | `warning` | `error`, `warning`, `info`, or `never` (report only) |
| `comment` | `true` | maintain the sticky pull-request comment |
| `sarif` | `false` | also upload a SARIF rendering to GitHub code scanning |
| `version` | `latest` | a release tag, `latest`, or `source` (build `kndo-cli` from the checked-out repository) |
| `working-directory` | `.` | the project root to analyze |
| `github-token` | the workflow token | the token used for the comment |

| Output | Meaning |
|---|---|
| `exit-code` | kndo's own exit code: 0 clean, 1 findings at or above `fail-on`, 2 kndo failed to run |
| `report` | path to the JSON report |

The SARIF rendering is produced with `--fail-on never`, so code scanning
receives every finding whatever the gate says.

## Any other CI

kndo is one binary and one exit code, so any runner works:

```sh
curl -fsSL https://raw.githubusercontent.com/eulke/kondo/main/install.sh | sh
kndo check --format json > kndo-report.json
```

Cache `.kndo/cache` between runs keyed on the kndo version; the cached and
uncached runs produce byte-identical reports, so a stale or missing cache
costs time, never correctness. For a merge request against a base branch,
`kndo check --diff origin/<base>` gives the same change-scoped report the
Action produces.

## Locally, before the commit

`kndo init --hook` installs a pre-commit hook running `kndo check --staged`:
the index, against `HEAD`. It is the same composition the Action runs on a
pull request, one commit earlier.
