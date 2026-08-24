# Graph navigation reference

Six read-only verbs — `find`, `describe`, `uses`, `used-by`, `trace`, `impact` — answer
graph questions without running a full report. All of them:

- accept **multiple selectors** per invocation and answer each independently; a bad selector
  yields an inline `{"status": "not-found" | "error", "selector": …, "message": …}` entry
  without failing its siblings;
- share one JSON envelope (`results` aligns 1:1 with the selectors, in argument order);
- cap listings (default 50, `--limit N`) with an explicit `elided` count — `elided > 0`
  means "there is more", never "that's all";
- render as `--format json` (default when piped), `human` (default on a tty), or `agent`.

Exit codes: `0` ok · `1` not found (zero-hit `find`, `trace` with no path; an empty
`used-by` is `0` — that IS the answer) · `2` malformed. Batches exit with the worst
individual status.

## Selectors

| Form | Names | Example |
|---|---|---|
| `path` | a file | `src/billing/tax.ts` |
| `path#Symbol` | a top-level symbol | `src/billing/tax.ts#calcTax` |
| `path#Owner.member` | a member, owner-qualified | `src/billing/tax.ts#TaxTable.lookup` |
| `dep:<name>` | a declared dependency | `dep:lodash` |
| `pkg:<name>` | a workspace package | `pkg:@acme/core` |
| `roots:production` \| `roots:test` \| `roots:tooling` | a root set (`trace` endpoints only) | `roots:test` |

Paths are project-relative. A member resolves by bare name when unambiguous; if several
owners declare the same member name, the response is an `error` listing the qualified
selectors to retry with — retry with one of those.

Two building blocks appear in every answer. A **node reference** — canonical selector (feed
it straight back into another verb), kind, reachability **color**
(`production | test-only | tooling-only | unreachable`), span. An **edge reference** — edge
kind (`imports`, `references`, …), confidence (`certain | probable | possible`), source site.

## find — search for nodes

```sh
kndo find calcTax --kind function
kndo find tax --color unreachable          # dead code matching "tax"
kndo find '' --color test-only --kind function   # what only tests keep alive
```

Matches against file basenames and symbol names (bare and `Owner.member`), ranked
exact > prefix > substring. Flags: `--kind <symbol-kind|file>`,
`--color <production|test-only|tooling-only|unreachable>`, `--lang <language>`, `--limit N`.
Returns `matches[]` of node references plus `elided`.

## describe — one node, in full

```sh
kndo describe src/billing/tax.ts#calcTax
```

Returns declaration facts (`exported`; `visibility` as an index into the language's ladder,
0 = most private), in/out degree by edge kind, which roots reach it, attached finding ids
(current, suppression-applied — feed them into the check loop), and provenance (`sources`).
Describing a file adds `file` (role, origin) and `declared_symbols`; `dep:`/`pkg:` selectors
add `dependency`/`package` blocks. For the neighbor lists themselves use `uses`/`used-by`;
`describe` gives counts.

## uses / used-by — the neighbor verbs

```sh
kndo used-by src/billing/tax.ts#calcLegacyTax --depth 2
kndo uses src/billing/tax.ts#calcTax --edges imports
```

`uses` walks outgoing edges (what this needs); `used-by` walks incoming ones (what needs
this — the deletion question). Flags:

- `--depth N` — traversal depth (default 1); each entry reports its depth.
- `--transitive` — the full closure instead of a fixed depth.
- `--edges imports|references|all` — restrict edge kinds (default `all`).
- `--limit N` — listing cap.

Each entry is a node reference plus the edge (`via`) it was reached through. The `by_color`
summary answers the deletion question at a glance: on `used-by`, `"production": 0` means
nothing in production depends on it.

## trace — why is this alive?

```sh
kndo trace src/billing/tax.ts#calcTax                 # liveness: root → node
kndo trace src/a.ts#f src/b.ts#g                      # directed: does f reach g, and how?
kndo trace --pair a.ts#f,b.ts#g --pair a.ts#f,c.ts#h  # batched directed pairs
```

With one selector: a concrete path from a root to the node — the answer to "why does kndo
think this is alive?". `--roots production|test|tooling` picks the root set (default
production, falling back to any). With two selectors (or repeated `--pair A,B`): traces A→B.
`--max-paths N` (default 10) and `--all` control how many distinct paths return.

Every path reports `weakest_confidence` — a chain is only as trustworthy as its weakest
edge. No path at all exits `1`: the node is NOT reachable that way.

## impact — what does a change touch?

```sh
kndo impact src/billing/rates.ts#rateFor
kndo impact src/billing/tax.ts#calcLegacyTax --if-deleted
```

Walks the reverse closure — everything that transitively depends on the node
(`--depth`/`--edges`/`--limit` as in the neighbor verbs) — with `by_color` totals and the
affected **roots**. `--if-deleted` additionally simulates the removal and reports the typed
reachability flips:

- `newly_unreachable` — nodes that die with it;
- `newly_test_only` — nodes that would survive only through tests;
- `freed_dependencies` — dependencies nothing would import anymore.

That is the pre-flight for a deletion: the whole cleanup, before the edit. The simulation
reports graph flips, not fabricated findings — those findings do not exist until the change
is made.

## Batching: kndo query

`kndo query` reads one JSON request per line from stdin and answers them all over a
**single** graph load — the cheap way to ask fifty questions. Request grammar:

```json
{ "id": "q1",
  "verb": "find | describe | uses | used-by | trace | impact",
  "selectors": ["src/tax.ts#calcTax"],
  "flags": { "kind": null, "color": null, "lang": null,
             "depth": 1, "transitive": false, "edges": "all",
             "all": false, "max_paths": 10, "roots": null,
             "pairs": [["a.ts#f","b.ts#g"]], "limit": 50, "if_deleted": false } }
```

- `id` is optional and echoed back — correlate answers however you like.
- `flags` mirror the CLI flags; irrelevant flags for a verb are ignored, so one superset
  request shape works everywhere. Omitted flags take their defaults.
- Answers come back as one JSON Line per request, **in input order**; the shared `run` block
  appears only on the first line.
- Malformed lines and unknown verbs are reported per line on stderr without dropping the
  rest; empty lines are skipped.
- A batch is capped at 1000 requests; excess lines are counted and reported, never silently
  dropped.
- Exit code is the worst individual status.
- JSON-only by design (it IS the machine transport); it refuses to run on a tty without
  piped stdin.

## Recipes

**Orient in an unfamiliar repo** (one batch):

```sh
printf '%s\n' \
  '{"id":"pkgs","verb":"find","selectors":[""],"flags":{"kind":"file","limit":30}}' \
  '{"id":"dead","verb":"find","selectors":[""],"flags":{"color":"unreachable","limit":30}}' \
  | kndo query
```

Then `kndo describe` the entry files it surfaces, and `kndo uses <entry> --depth 2` to map
the core module structure. `kndo health` gives the overall shape in one number per category.

**Prove a deletion safe:**

1. `kndo used-by <sel>` → require `by_color.production == 0`.
2. If anything non-production remains, `kndo trace <sel> --roots test` shows what keeps it
   alive and which edge to cut (delete the tests together with the code).
3. `kndo impact <sel> --if-deleted` → the complete removal set (`newly_unreachable` +
   `freed_dependencies`) — delete all of it in one edit.
4. After editing: `kndo check --diff <base> --format agent` → targeted ids under `fixed:`,
   nothing unexpected under `new:`.

**Scope a refactor:**

1. `kndo impact <sel>` → who is affected, at what depth, and which production roots.
2. `kndo describe` each depth-1 caller to understand the call sites (`degree`, spans).
3. `kndo trace --pair <caller>,<sel>` when you need the concrete path between two nodes.
