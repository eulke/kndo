# Suppressions & baseline reference

Both mechanisms **acknowledge** findings — they never hide the truth. Suppressed and
baselined findings leave the report and `--fail-on`, but every run still counts them
(`suppressed N inline, M config` / `baseline N acknowledged` on the result line), and the
health score is computed before either applies. Suppressing never improves health.

| Situation | Tool |
|---|---|
| One intentional exception, with a reason a reviewer should see | inline `kndo:allow` |
| Bulk adoption on a legacy codebase; findings without a source location | baseline |
| A whole path that is not yours (generated, vendored, examples) | `kndo.toml` `[[rule]]` skip |
| Opting a plugin's findings into the CI gate | `kndo.toml` `[plugins.gate]` |

## Inline pragmas

A comment in the language's own comment syntax, on or directly above a declaration:

```ts
// kndo:allow unused legacy API kept for the mobile team until the v3 cutover
export function legacyExport() { /* … */ }
```

Syntax:

```text
kndo:allow      <category>[:<subject>] [free-text reason…]
kndo:allow-file <category>[:<subject>] [free-text reason…]
```

- `<category>` is any core category or a namespaced plugin category
  (`plugin:<coordinate>/<rule>`).
- `:<subject>` optionally narrows to one subject kind (`kndo:allow unused:enum-member`).
- Everything after the target is the **reason** — free text for reviewers.
  **Always write one**; a reasonless pragma is indistinguishable from noise.

Binding rules:

- `kndo:allow` attaches to the declaration starting on its own line (trailing comment) or on
  the line directly after (comment above). It covers the declaration and everything it
  declares — a class-level allow covers the members.
- `kndo:allow-file` covers the whole file, wherever it sits.
- A finding matches when file, category, and (if given) subject agree and, for declaration
  scope, the finding falls within the bound declaration's lines.
- A finding with no source location (some dependency-level findings) is never
  inline-suppressible — acknowledge it via the baseline.

No-flicker guarantee: analyses compute the complete finding set first; pragmas only mark
findings for filtering. So an actively-suppressing pragma is never reported stale, deleting
a stale pragma never resurrects a finding, and suppression never changes what other analyses
see.

Stale pragmas self-report as [`stale` findings](findings-playbook.md#stale) at the pragma's
own location — suppressions are self-cleaning; delete them when flagged. Two edge rules:
`kndo:allow stale` is rejected (meta-suppression; use the baseline); a `plugin:` pragma
whose plugin is not active this run is skipped entirely, neither suppressing nor stale.

## The baseline

```console
$ kndo baseline
kndo: baseline written — 412 findings acknowledged (.kndo/baseline.json)
```

Snapshots the complete current finding set: everything existing today is acknowledged;
everything new from now on reports (and gates) normally. The adopt-on-day-one mechanism.

- **Matching is by stable finding id** — reformatting un-acknowledges nothing; renaming or
  moving a symbol honestly produces a new finding.
- Every run reports `acknowledged` (still matching) and `stale` (fixed since the snapshot)
  counts — the file never lies silently.
- **It never grows on its own**: `kndo baseline` refuses to overwrite; every rewrite is an
  explicit `kndo baseline --update` in a commit a human reviews. As an agent, do not run
  `baseline` or `--update` on your own initiative — creating or refreshing a baseline is a
  policy decision; propose it instead.
- **Commit the file.** `kndo init` gitignores `.kndo/` for the cache's sake — force-add the
  baseline or carve out `!.kndo/baseline.json`.
- Applies in every mode: diff runs filter both sides symmetrically, so a baselined finding
  is neither new nor fixed.

## kndo.toml knobs

```toml
[analysis]
skip = ["unused:enum-member"]     # categories or category:subject, project-wide
min-confidence = "possible"       # report floor; "probable" hides the speculative tier

[[rule]]                          # per-path overrides
paths = ["examples/**"]
skip = ["unused"]

[plugins.gate]                    # opt plugin findings into the exit-code gate
"github.com/acme/plugin" = "warning"
```

## Choosing well

- Prefer **deleting** to suppressing — the finding is usually right.
- Prefer a **pragma with a reason** for individual, intentional exceptions: it lives next to
  the code, shows up in review, and cleans up after itself via `stale`.
- Prefer the **baseline** for bulk adoption and for findings without a source location.
- If the same suppression keeps recurring for a framework reason ("route handlers are not
  unused"), the durable fix is a kndo plugin that contributes the root — propose that to the
  human instead of scattering pragmas.
