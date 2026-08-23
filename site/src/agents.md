# For agents

kndo treats LLM coding agents as a first-class audience: after generating or deleting code, an
agent runs kndo and gets machine-checkable feedback that its edit is complete — nothing new is
orphaned, nothing it targeted survives.

## The agent format

```console
$ kndo check --format agent
```

Deterministic, token-frugal text that keeps every machine anchor — stable finding ids,
selectors, explicit elision markers — at a fraction of JSON's token cost. The same engine and
guarantees as `--format json`; only the rendering differs.

## Stable finding ids

`id = "kndo-" + hash(category, subject_kind, path, symbol path, discriminator)` — line numbers
never participate. The loop this enables: read finding `kndo-3f2a…` → fix it → re-run → assert
that id is absent. Reformatting can't break the assertion; a rename honestly produces a new id.

## Graph navigation verbs

Six read-only verbs answer "can I delete this?" *before* editing:

```console
$ kndo find 'calcTax*'              # fuzzy/glob symbol search → selectors
$ kndo describe src/tax.ts#calcTax  # declaration, visibility, reachability, metrics
$ kndo uses src/tax.ts#calcTax      # what it references
$ kndo used-by src/tax.ts#calcTax   # what references it (the deletion question)
$ kndo trace src/tax.ts#calcTax     # a concrete root→symbol path: WHY is this alive?
$ kndo impact --if-deleted src/tax.ts#calcTax   # what would become unreachable with it
```

All emit versioned JSON envelopes. Exit codes are informative, not failures: `0` found,
`1` not found (with suggestions in the envelope), `2` malformed request.

## Batching

`kndo query` reads one JSON request per line from stdin and answers them over a **single**
graph load — the cheap way to ask fifty questions:

```console
$ printf '%s\n' \
    '{"verb":"used-by","selectors":["src/tax.ts#calcTax"]}' \
    '{"verb":"impact","selectors":["src/tax.ts#calcTax"],"if_deleted":true}' \
  | kndo query
```

## The check loop

1. `kndo check --format agent` before editing → the worklist, with ids.
2. Navigate (`used-by`, `impact --if-deleted`) to plan a safe deletion.
3. Edit.
4. `kndo check --diff <base> --format agent` → assert: targeted ids gone, nothing `new`.
