# Suppressions & baseline

Two mechanisms exist to *acknowledge* findings — never to hide the truth. Suppressed and
baselined findings disappear from the report and from `--fail-on`, but every run still counts
them (`suppressed: N inline · baseline: M acknowledged` in the header, `suppressed` and
`baseline` objects in the JSON), and the [health score](health.md) is computed before either
is applied.

| Situation | Tool |
|---|---|
| Adopting kndo on a legacy codebase | [baseline](#the-baseline) |
| One intentional exception, with a reason a reviewer should see | inline [`kndo:allow`](#inline-suppressions) |
| Opting a plugin's findings into the CI gate | [`[plugins.gate]`](configuration.md#pluginsgate) |

## Inline suppressions

A comment in the language's own comment syntax, on or directly above a declaration:

```ts
// kndo:allow unused legacy API kept for the mobile team until the v3 cutover
export function legacyExport() { /* … */ }
```

```rust
// kndo:allow test-only exercised by the fuzz harness, which lives out of tree
pub fn arbitrary_input() -> Input { /* … */ }
```

```go
//kndo:allow-file duplicate generated twin of the v1 handler, kept during migration
```

### Syntax

```text
kndo:allow      <category>[:<subject>] [free-text reason…]
kndo:allow-file <category>[:<subject>] [free-text reason…]
```

- **`<category>`** is any core category from the
  [registry](rules.md#the-category-registry), or a namespaced plugin category
  (`plugin:<coordinate>/<rule>`).
- **`:<subject>`** optionally narrows to one subject kind: `kndo:allow unused:enum-member`
  suppresses unused-enum-member findings and leaves everything else alone.
- Everything after the target is a **reason** — free text for your reviewers. Write one.

### Binding rules

- **`kndo:allow`** (declaration scope) attaches to the declaration that *starts on the
  pragma's own line* (a trailing same-line comment) or *on the line directly after it* (a
  comment directly above). It covers the declaration and everything it declares — a
  class-level allow covers its members.
- **`kndo:allow-file`** covers the whole file, wherever the comment sits.
- A finding matches a pragma when file, category, and (if given) subject agree, and — for
  declaration scope — the finding's location falls within the bound declaration's lines.
- A finding with no source location (some dependency-level findings) is never
  inline-suppressible; acknowledge it via the baseline.

### The no-flicker guarantee

Analyses always compute the **complete** finding set first, as if no pragmas existed;
pragmas then only *mark* findings for filtering. Consequences:

- an actively-suppressing pragma can never be reported as stale;
- deleting a stale pragma can never resurrect a finding;
- suppression never changes what other analyses see — only what the report shows.

### Stale pragmas

A pragma that isn't doing its job becomes a [`stale` finding](rules.md#stale) at the pragma's
own location — unknown category (with a did-you-mean hint), attached to no declaration,
targeting `stale` itself, or simply matching nothing anymore because the issue it
acknowledged is gone. The fix is always the same: delete the pragma. Suppressions are
self-cleaning by construction — they cannot silently accumulate.

Two edge rules:

- `kndo:allow stale` is rejected as meta-suppression (and is itself stale): stale findings
  are not inline-suppressible. If you must, acknowledge them via the baseline.
- A `plugin:` pragma whose category no **active** plugin declares this run is skipped
  entirely — neither suppressing nor stale. The plugin may simply not be activated in this
  checkout, and flagging the pragma would flicker with activation state.

## The baseline

```console
$ kndo baseline
kndo: baseline written — 412 findings acknowledged (.kndo/baseline.json)
```

The baseline snapshots the complete current finding set into `.kndo/baseline.json` — the
adopt-on-day-one mechanism: everything that exists today is acknowledged; everything new from
now on is reported (and gated) normally.

- **Matching is by stable finding id.** Ids hash the code object — category, subject kind,
  path, symbol path, and a category-specific discriminator — never line numbers. Reformatting
  and moving lines around un-acknowledges nothing; renaming or moving a symbol honestly
  produces a new finding.
- **Reports stay honest.** Every run shows `acknowledged` (entries still matching a current
  finding) and `stale` (entries whose finding is gone — fixed since the snapshot).
- **The baseline never grows on its own.** `kndo baseline` refuses to overwrite an existing
  file; every rewrite is an explicit `kndo baseline --update`, which re-snapshots — keeping
  what still reproduces, dropping what was fixed — in a commit a human reviews.
- **Commit the file.** It is meant to be shared and reviewed. (`kndo init` gitignores
  `.kndo/` for the cache's sake — force-add the baseline or carve out
  `!.kndo/baseline.json`.)
- Baselines apply in every mode: in diff runs both sides are filtered symmetrically, so a
  baselined finding neither counts as new nor as fixed.

## Choosing well

- Prefer **deleting** to suppressing — the finding is usually right.
- Prefer a **pragma with a reason** to a baseline entry for individual, intentional
  exceptions: the pragma lives next to the code, shows up in review, and cleans up after
  itself via `stale`.
- Prefer the **baseline** for bulk adoption and for findings without a source location.
- If the same suppression keeps recurring for a *framework* reason ("this is a route handler,
  it is not unused"), the right fix is a [plugin](plugin-authoring.md) that contributes the
  root or annotation — then nobody needs the pragma.
