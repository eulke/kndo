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
          fail-on: warning    # default; "none" = report-only
          comment: true       # sticky PR comment (default)
          sarif: false        # upload to code scanning (needs security-events: write)
```

## What a run does

1. Installs kndo (a pinned release, `latest`, or `version: source` to build from the checkout).
2. Restores `.kndo/cache` via `actions/cache` — content-addressed entries make stale restores
   safe (worst case a colder run, never a wrong one).
3. Runs `kndo check --diff <base>` against the PR's merge-base — identical semantics to local.
4. Publishes: **one** sticky comment per PR, edited in place on every push (never a pile of
   stale reports); inline annotations for new findings ≥ warning; the full report in the job
   summary always. Fixed findings always render — deleting dead code should feel like a win in
   review too.

## Semantics worth knowing

- **Exit codes are kndo's own**: `1` = findings at/above `fail-on`; `2` = kndo failed to run,
  annotated as infrastructure — never as "code has findings". A typo'd base ref or a missing
  repository is exit 2, loudly; it can never pass vacuously.
- **Fork PRs degrade, never fail**: without a write token the comment is skipped with a notice
  in the summary; annotations and the summary still publish.
- Any CI system can build the same integration: the Action is a thin frontend over
  `kndo check --diff <base> --format json` — the versioned JSON is the contract.
