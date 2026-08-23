# Configuration & suppression

kndo is zero-config by design: manifests are the configuration. The mechanisms below exist to
*acknowledge* findings — never to hide the truth (suppressed findings are still counted and
reported in the `suppressed` summary).

## kndo.toml

`kndo init` writes a commented template. The one section the engine reads today:

```toml
[plugins.gate]                    # RFC 0018: opt plugin findings into the exit-code gate
"github.com/acme/conventions" = "warning"          # whole plugin, capped at warning
"kndo:nextjs/orphan-page" = "error"                # one rule; per-rule beats per-plugin
```

Without an entry, plugin findings render but are advisory — they never move the exit code.
Config can *lower* a rule's declared severity, never raise it.

## Baseline

`.kndo/baseline.json` (created by `kndo baseline`) acknowledges the findings that existed at
adoption time. It matches by stable finding id — ids hash the code object, not line numbers,
so reformatting never un-acknowledges anything, while a rename is honestly a new finding.

## Inline suppressions

A comment on (or directly above) a declaration, in the language's own comment syntax:

```ts
// kndo:allow unused legacy API kept for the mobile team until Q3
export function legacyExport() { … }
```

- `kndo:allow <category>[:<subject>] [reason…]` covers the declaration and everything it
  declares (a class-level allow covers its members).
- `kndo:allow-file …` covers the whole file.
- The subject facet narrows: `kndo:allow unused:enum-member` leaves other findings alone.
- Analyses always compute the full finding set first; pragmas only *mark* findings. So an
  actively-suppressing pragma can never be `stale`, and deleting a stale pragma can never
  resurrect a finding.
- A pragma that stops matching anything becomes a `stale` finding telling you to delete it.
  `stale` itself is not inline-suppressible — acknowledge it via baseline if you must.

## Choosing between them

| Situation | Tool |
|---|---|
| Adopting kndo on a legacy codebase | baseline |
| One intentional exception, with a reason a reviewer should see | inline `kndo:allow` |
| Opting a plugin's findings into the CI gate | `[plugins.gate]` in kndo.toml |
