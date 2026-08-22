# kndo-action

Runs [kndo](../README.md) on pull requests and reports where reviewers already look — a sticky
PR comment, file annotations on the diff, and the job summary — gating the merge on the same
rules as the local pre-commit hook. The Action is a **frontend** (RFC 0010 §2): it installs the
kndo binary, runs `kndo check --diff <base> --format json`, and publishes the JSON contract; it
contains zero analysis logic.

## Usage

```yaml
name: kndo
on: pull_request
permissions:
  contents: read
  pull-requests: write        # sticky comment; drop it to run comment-less
jobs:
  kndo:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0      # kndo needs the merge-base resolvable (RFC 0004 §6)
      - uses: eulke/kondo/action@v1
        with:
          fail-on: warning    # default; "none" = report-only
          comment: true       # sticky PR comment (default true)
          sarif: false        # upload to GitHub code scanning (needs security-events: write)
```

## Inputs

| Input | Default | Meaning |
|-------|---------|---------|
| `fail-on` | `warning` | Fail on findings at/above this severity: `error` \| `warning` \| `info` \| `none` |
| `comment` | `true` | Maintain the sticky PR comment (one living report, upserted in place) |
| `sarif` | `false` | Also upload SARIF to code scanning (add `security-events: write`) |
| `version` | `latest` | Release tag to install, `latest`, or `source` (build `kndo-cli` from the checkout — dogfood/pre-release) |
| `working-directory` | `.` | Project root to analyze |
| `github-token` | workflow token | Token for the sticky comment |

## Outputs

`exit-code` — kndo's own exit semantics (RFC 0006 §5): `0` clean, `1` findings at/above
`fail-on`, `2` kndo itself failed (annotated as infrastructure, never as "code has findings").

## Behavior notes

- **Sticky comment**: one comment per PR, identified by a hidden `<!-- kndo-report -->` marker
  and edited in place on every push — never appended.
- **Fork PRs**: the default token cannot write comments; the Action degrades, never fails —
  annotations and the job summary still publish, and the skip is noted in the summary (§5).
- **Annotations**: new findings ≥ warning, capped at GitHub's ~10-per-step render limit with the
  overflow noted; advisory (plugin) findings are never annotated.
- **Cache**: `.kndo/cache` is restored via `actions/cache` keyed on OS + kndo version;
  content-addressed entries make stale restores safe — worst case is a colder run.
- **Non-PR events**: runs `kndo check` in full mode; comment is skipped, summary still publishes.
