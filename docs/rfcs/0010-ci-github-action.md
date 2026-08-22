# RFC 0010 — kndo in CI: GitHub Action & PR Reporting

**Status:** Implemented (M6 — `action/`, composite; dogfooded by `ci.yml`'s test job on kndo's own PRs) · **Depends on:** RFC 0004 (diff modes, cache), RFC 0006 (formats, exit codes),
contracts §5 (Engine boundary) · **Ships:** M6 (ROADMAP)

## 1. Goal

`kndo-action`: a first-party GitHub Action that runs kndo on pull requests and reports where
reviewers already look — a PR comment, file annotations, the job summary — gating the merge on
the same rules as the local pre-commit. Setup is one workflow block, zero kndo config required:

```yaml
- uses: kndo-dev/kndo-action@v1
  with:
    fail-on: warning          # default; "none" = report-only
    comment: true             # sticky PR comment (default true)
    sarif: false              # upload to GitHub code scanning
```

## 2. Architectural position

The Action is a **frontend** (contracts §5): it downloads the pinned kndo binary, invokes
`kndo check --diff <base> --format json`, and renders/publishes the result. It contains zero
analysis logic and reads only the JSON contract — meaning any CI system (GitLab CI, Buildkite,
Jenkins) can build the same integration against the same JSON without core changes; the GitHub
Action is simply the one we ship and dogfood. Comment markdown is presentation, so it is
**frontend-owned** (RFC 0009 principle applied to a different frontend), built from the JSON
delta envelope.

## 3. What a run does

1. Resolve the diff base: the PR's merge-base against the base branch (not the branch tip —
   identical semantics to local `--diff`, RFC 0004 §6).
2. Restore `.kndo/cache` via actions/cache (key: kndo version + graph schema version + OS;
   content-addressed entries make stale restores safe — worst case is a colder run, never a
   wrong one, RFC 0004 §3).
3. `kndo check --diff <base> --format json` → typed delta: new findings, fixed findings,
   health movement, all including derived effects far from the touched files.
4. Publish (§4) and exit with kndo's own exit code semantics (RFC 0006 §5): findings at/above
   `fail-on` fail the check; kndo failures (exit 2) fail it *differently* — annotated as
   infrastructure, never as "code has findings".

## 4. Publishing surfaces

**Sticky comment (primary).** One comment per PR, **upserted in place** on every push —
identified by a hidden HTML marker (`<!-- kndo-report -->`) — never appended: a PR with thirty
pushes gets one living report, not thirty stale ones. Layout mirrors the terminal report in
markdown:

```markdown
### kndo · 3 new · 2 fixed · health 82.4 → 84.1 (B) ↑ · budget 2/3 ✗

- [x] health-drop ≤ 0.0 — +1.7
- [x] new defects = 0 — 0
- [ ] net findings ≤ 0 — +1 (**over by 1**)

**New**
| | finding | where | why |
|-|---------|-------|-----|
| ✗ | `unresolved` import | `src/api/client.ts:3` | `./transpor` resolves to nothing |
| ◦ | `unused` function | `src/billing/tax.ts:41` | last production ref removed by this PR |

**Fixed** ✓ `unused` dependency `date-fns`

<details><summary>47 baseline findings unchanged</summary>…</details>
```

The budget checklist (one checkbox per configured `[delta]` rule, RFC 0006 §5) makes the sticky
comment a *living budget marker*: every push updates how much of the tolerance is consumed.
Group order and glyph vocabulary follow RFC 0009 §3 (rendered as text/emoji-safe equivalents);
long sections collapse under `<details>`; hard cap per section with a link to the workflow run
for the full report. Fixed findings always render — the reward loop (RFC 0004 §6) applies to
reviewers too.

**File annotations.** New findings of severity ≥ warning are emitted as GitHub annotations on
the diff (`::warning file=,line=`) so they appear inline in Files Changed. Capped (GitHub limit
~10/step honored explicitly, overflow noted in the comment).

**Job summary.** The full report is always written to `GITHUB_STEP_SUMMARY` — it works in every
context, including where comments are impossible.

**SARIF (opt-in).** `sarif: true` uploads the SARIF rendering to code scanning; findings then
also appear in the Security tab and as native PR review comments. Redundant with the sticky
comment by design — teams pick one or both.

## 5. Fork PRs & permissions

Declared permissions: `contents: read`, `pull-requests: write` (comment), `security-events:
write` (only with `sarif: true`). On fork PRs the default token cannot write comments: the
Action **degrades, never fails** — annotations + job summary still publish, and the comment is
skipped with a notice in the summary. No `pull_request_target` gymnastics in v1: kndo runs
static analysis on untrusted code safely (it never executes the analyzed project), but
write-token workflows on forks are a security decision we don't make for users.

## 6. Non-goals

- No auto-fix commits or suggested-change batches from CI (post-1.0, alongside `kndo clean`).
- No trend dashboards — the JSON is stable; external systems archive it (vision non-goal).
- No GitHub App in 1.0: the Action + SARIF cover the surface without hosting anything. A
  richer App (checks UI, org dashboards) is a parking-lot item that would consume the same
  JSON contract.
