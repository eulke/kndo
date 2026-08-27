# Graph navigation

Six read-only verbs answer questions about the project graph — "what references this?",
"why is this alive?", "what breaks if I delete it?" — without running a full report. They are
built for scripting and for [agents](agents.md): versioned JSON envelopes, stable selectors,
explicit elision, informative exit codes.

```console
$ kndo find 'calcTax'                              # search → selectors
$ kndo describe src/tax.ts#calcTax                 # everything about one node
$ kndo uses src/tax.ts#calcTax                     # what it depends on
$ kndo used-by src/tax.ts#calcTax                  # what depends on it
$ kndo trace src/tax.ts#calcTax                    # why is this alive?
$ kndo impact src/tax.ts#calcTax --if-deleted      # what dies with it
```

Every verb accepts **multiple selectors** in one invocation and answers each independently —
one bad selector never fails its siblings; it yields an inline
`{ "status": "not-found" | "error", … }` entry in `results` instead.

## Selectors

| Form | Names | Example |
|---|---|---|
| `path` | a file | `src/billing/tax.ts` |
| `path#Symbol` | a top-level symbol in that file | `src/billing/tax.ts#calcTax` |
| `path#Owner.member` | a member, qualified by its owner | `src/billing/tax.ts#TaxTable.lookup` |
| `dep:<name>` | a declared dependency | `dep:lodash` |
| `pkg:<name>` | a workspace package | `pkg:@acme/core` |
| `roots:production` \| `roots:test` \| `roots:tooling` | a whole root set (valid only as a `trace` endpoint) | `roots:test` |

A member also resolves by its bare name when unambiguous; if several owners in the file
declare the same member name, the response is an `error` listing the qualified selectors to
retry with. Paths are project-relative.

## The envelope

All verbs share one JSON envelope; `results` aligns 1:1 with `query.selectors`, in argument
order:

```json
{
  "schema_version": "1.1.0",
  "kndo_version": "0.1.0",
  "query": { "verb": "used-by", "selectors": ["src/billing/tax.ts#calcLegacyTax"] },
  "run": { "cache": "warm", "duration_ms": 74 },
  "status": "ok",
  "results": [ { "…": "verb-specific, below" } ],
  "diagnostics": []
}
```

Two building blocks appear everywhere:

```json
{ "selector": "src/billing/tax.ts#TaxTable.lookup", "kind": "method",
  "color": "test-only",
  "span": { "path": "src/billing/tax.ts", "start": [90,3], "end": [104,4] } }
```

— a **node reference**: its canonical selector (feed it straight back into another verb), its
kind, its reachability **color** (`production` | `test-only` | `tooling-only` | `unreachable`),
and its span. And:

```json
{ "edge": "references", "confidence": "certain",
  "site": { "path": "src/billing/index.ts", "start": [12,10], "end": [12,23] } }
```

— an **edge reference**: the edge kind, its confidence, and the concrete source site.

Listings are always capped (default 50, `--limit N` to change) and carry explicit `elided`
counts — `elided > 0` means "there is more", never "that's all".

**Formats**: the same `--format` rules as `check` — JSON when piped (shown here), a colored
human rendering on a terminal, `--format agent` for the token-frugal text form.

**Exit codes**: `0` ok · `1` selector/path not found (`find` with zero hits, `trace` with no
path — an empty `used-by` on a genuinely-unused symbol is a legitimate `ok`: that *is* the
answer) · `2` malformed request or error. Batches exit with the worst individual status.

## find — search for nodes

```console
$ kndo find calcTax --kind function
```

Matches the pattern against file basenames and symbol names (bare and `Owner.member`
qualified), ranked exact > prefix > substring. Filters: `--kind <symbol-kind|file>`,
`--color <production|test-only|tooling-only|unreachable>`, `--lang <language>`. `--limit N` caps
the listing.

```json
{ "matches": [
    { "selector": "src/billing/tax.ts#calcTax", "kind": "function", "color": "production",
      "span": { "path": "src/billing/tax.ts", "start": [10,1], "end": [38,2] } },
    { "selector": "src/billing/tax.ts#calcTaxLegacy", "kind": "function", "color": "unreachable",
      "span": { "path": "src/billing/tax.ts", "start": [41,1], "end": [78,2] } }
  ],
  "elided": 0 }
```

`--color unreachable` is the "show me the dead code here" query;
`--color test-only --kind function` is "what do only my tests keep alive".

## describe — one node, in full

```console
$ kndo describe src/billing/tax.ts#calcTax
```

```json
{ "node": { "selector": "src/billing/tax.ts#calcTax", "kind": "function", "color": "production",
            "span": { "path": "src/billing/tax.ts", "start": [10,1], "end": [38,2] } },
  "declaration": { "kind": "function", "exported": true, "visibility": 0,
                   "span": { "path": "src/billing/tax.ts", "start": [10,1], "end": [38,2] } },
  "degree": { "in_by_kind": { "references": 7 }, "out_by_kind": { "references": 3, "imports": 1 } },
  "reached_by_roots": [ { "selector": "src/index.ts", "kind": "file", "color": "production" } ],
  "findings": ["kndo-58ab12f0c3d4"],
  "sources": ["adapter:js-ts"],
  "elided": {} }
```

Everything kndo knows about the node in one shot: declaration facts (`visibility` is the
index into the language's visibility ladder — 0 is the most private rung, higher is wider —
alongside the language-independent `exported` bit), in/out degree by edge
kind, which roots reach it (capped, with the cap in `elided`), the ids of findings attached
to it (the current, suppression-applied set — feed them to the [baseline](suppressions.md)
loop or your own tracking), and the provenance of the facts. Describing a **file** adds a
`file` block (role, origin) and its `declared_symbols`; `dep:`/`pkg:` selectors add
`dependency`/`package` blocks. For the neighbor lists themselves, use `uses`/`used-by` —
`describe` gives you the counts.

## uses / used-by — the neighbor verbs

```console
$ kndo used-by src/billing/tax.ts#calcLegacyTax --depth 2
$ kndo uses src/billing/tax.ts#calcTax --edges imports
```

`uses` walks outgoing edges (what this needs); `used-by` walks incoming ones (what needs
this — **the deletion question**). Flags:

- `--depth N` — traversal depth (default 1); each entry reports its depth.
- `--transitive` — the full closure instead of a fixed depth.
- `--edges imports|references|all` — restrict edge kinds (default `all`).
- `--limit N` — listing cap.

```json
{ "node": { "selector": "src/billing/tax.ts#calcLegacyTax", "kind": "function", "color": "test-only" },
  "entries": [
    { "node": { "selector": "src/billing/tax.test.ts", "kind": "file", "color": "test-only" },
      "via": { "edge": "references", "confidence": "certain",
               "site": { "path": "src/billing/tax.test.ts", "start": [8,3], "end": [8,20] } },
      "depth": 1 }
  ],
  "by_color": { "production": 0, "test-only": 1, "tooling-only": 0, "unreachable": 0 },
  "elided": 0 }
```

`by_color` is the summary that answers the deletion question at a glance:
`"production": 0` on a `used-by` means nothing in production depends on it.

## trace — why is this alive?

Two shapes:

```console
$ kndo trace src/billing/tax.ts#calcTax                  # liveness: root → node
$ kndo trace src/a.ts#f src/b.ts#g                       # directed: does f reach g, and how?
$ kndo trace --pair a.ts#f,b.ts#g --pair a.ts#f,c.ts#h   # batched directed pairs
```

With **one** selector, `trace` finds a concrete path from a root to the node — the answer to
"why does kndo think this is alive?". `--roots production|test|tooling` picks which root set
to trace from (default: production, falling back to any). With **two** selectors (or
`--pair A,B` repeated), it traces from A to B. `--max-paths N` (default 10) and `--all`
control how many distinct paths come back.

```json
{ "from": { "selector": "src/index.ts", "kind": "file", "color": "production" },
  "to":   { "selector": "src/billing/tax.ts#calcTax", "kind": "function", "color": "production" },
  "paths": [
    { "hops": [
        { "node": { "selector": "src/billing/index.ts" }, "via": { "edge": "imports",    "confidence": "certain" } },
        { "node": { "selector": "src/billing/tax.ts#calcTax" }, "via": { "edge": "references", "confidence": "probable" } }
      ],
      "weakest_confidence": "probable" }
  ],
  "paths_elided": 0 }
```

Every path reports its `weakest_confidence` — a chain is only as trustworthy as its weakest
edge. No path at all is exit `1`: the node is *not* reachable that way.

## impact — what does a change touch?

```console
$ kndo impact src/billing/rates.ts#rateFor
$ kndo impact src/billing/tax.ts#calcLegacyTax --if-deleted
```

`impact` walks the reverse closure — everything that transitively depends on the node
(`--depth`/`--edges`/`--limit` as in the neighbor verbs) — and reports which **roots** are
affected:

```json
{ "node": { "selector": "src/billing/rates.ts#rateFor", "kind": "function", "color": "production" },
  "affected": [
    { "node": { "selector": "src/billing/tax.ts#calcTax" }, "via": { "edge": "references", "confidence": "certain" }, "depth": 1 },
    { "node": { "selector": "src/api/checkout.ts" },        "via": { "edge": "imports",    "confidence": "certain" }, "depth": 2 }
  ],
  "by_color": { "production": 2, "test-only": 1, "tooling-only": 0, "unreachable": 0 },
  "elided": 0,
  "affected_roots": [ { "kind": "production", "node": { "selector": "src/index.ts" } } ],
  "affected_roots_elided": 0 }
```

`--if-deleted` additionally **simulates the removal** and reports the typed reachability
flips — the pre-flight for a deletion:

```json
  "if_deleted": {
    "newly_unreachable": [ { "selector": "src/billing/tax-tables.ts", "kind": "file" } ],
    "newly_unreachable_elided": 0,
    "newly_test_only": [],
    "newly_test_only_elided": 0,
    "freed_dependencies": ["big-decimal"]
  }
```

"Delete this and `tax-tables.ts` becomes unreachable too, and nothing will import
`big-decimal` anymore" — the whole cleanup, before you edit. The simulation reports graph
flips, not fabricated finding objects: those findings don't exist until you actually make the
change.

## Batching: kndo query

Every verb is also available as a JSON Lines batch over **one** graph load — see
[For agents](agents.md#batching-kndo-query) for the request grammar. The single-shot verbs
above and `kndo query` produce the same envelopes; `query` just amortizes the graph.
