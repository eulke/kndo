# RFC 0007 — Graph Navigation & Query Commands

**Status:** Draft · **Depends on:** RFC 0001, 0004, 0006 · **Normative schema:** [contracts/output-schema.md](../contracts/output-schema.md) §8

## 1. Motivation

`kondo check` already pays the cost of building a whole-project semantic graph and keeping it warm
in `.kondo/`. That graph answers questions agents (and humans) currently burn enormous effort on
with grep-and-read loops: *who uses this? what does this depend on? why does A end up depending
on B? is this safe to delete?*

Navigation commands expose the graph **read-only** through the CLI, so an LLM agent can ask a
precise question and get a precise, bounded answer in milliseconds — replacing multi-file context
stuffing with a handful of cheap, verifiable tool calls. This is the second half of the "for
humans and agents" mission: `check` prevents waste; navigation prevents the wasteful *exploration*
that produces it.

## 2. Design tenets

1. **Read-only, warm, fast.** Navigation verbs never mutate findings or baseline. They revalidate
   the cache exactly like `check` (patching changed files first, RFC 0004 §4), so answers reflect
   the working tree, within the same < 500 ms warm budget. Startup + revalidation dominate that
   budget, so batching (§4.7) amortizes them: many questions, one process, one graph load.
2. **Bounded by default.** Every listing is capped (default 50 entries, `--limit`) with an explicit
   `"elided": N` count and deterministic ordering — an agent always knows whether it saw
   everything, and output can never blow up a context window.
3. **Selectors are stable addresses.** Every answer contains selectors (§3) that can be fed back
   into the next query verbatim. Chaining queries requires no parsing heuristics.
4. **Answers carry evidence.** Edges come with kind, confidence, and source span — an agent can
   jump straight to the proving line instead of trusting a summary.
5. **Same output discipline as `check`.** Human format on TTY, JSON (schema §8 of the output
   contract) when piped or with `--format json`. No information exists in one format only.

## 3. Selectors

A selector uniquely addresses a graph node:

| Node | Syntax | Example |
|------|--------|---------|
| File | project-relative path | `src/billing/tax.ts` |
| Symbol | `path#name`, nested via `.` | `src/billing/tax.ts#TaxTable.lookup` |
| Package | `pkg:<name>` | `pkg:lodash` |
| Root set | `roots:production` \| `roots:test` \| `roots:tooling` | `roots:production` |

Ambiguity (e.g. overloads) is an error listing the concrete candidates — never a guess.
`kondo find` (§4.1) is the discovery verb that turns names into selectors.

## 4. Verbs

### 4.1 `kondo find <pattern>`
Search files and symbols by name (substring + qualified-suffix match, ranked exact > prefix >
substring). Filters: `--kind function|class|file|…`, `--color unreachable|test-only|…`,
`--lang <adapter-id>`. Returns selectors + kind + reachability color + declaring span.
The entry point of every agent workflow: name → selector.

### 4.2 `kondo describe <selector>`
Everything the graph knows about one node, in one call:
declaration (span, kind, visibility, exported), file flavor & reachability color, direct degree
(in/out, by edge kind), roots that reach it (nearest first), metrics (cyclomatic, CRAP, coverage
if ingested), duplication group membership, open findings attached to it, provenance
(adapter/plugins that produced its facts). For a `pkg:` selector: manifest scope, importing files
count, usage status. For files: declared symbols (capped).

### 4.3 `kondo uses <selector>`
Outgoing dependencies: what this node needs. `--depth N` (default 1), `--transitive`
(fixpoint, deduplicated, depth-annotated), `--edges imports|references|all` (default all).
Answers *"what am I pulling in?"* — e.g. before extracting a module.

### 4.4 `kondo used-by <selector>`
Incoming dependents: who needs this node. Same flags as `uses`, plus `--split-by-color` to
separate production, test-only, and tooling consumers. `used-by X` returning only test consumers
is the *"safe to delete (with its tests)"* signal — the query twin of the `test-only-code`
finding. Empty result ⇒ matches an `unused-code`/`unused-file` finding by construction.

### 4.5 `kondo trace <from> [<to>]`
Explain connectivity as concrete paths, every hop with edge kind, confidence, and span:

- `kondo trace A B` — directed path(s) from A to B. Default: one shortest path;
  `--all --max-paths K` for alternatives; exit code 1 (no-path) tells an agent decoupling
  succeeded. Answers *"why does A depend on B?"*.
- `kondo trace X` (single argument) — **liveness trace**: shortest path from the nearest root to
  X, `--roots production|test|tooling|all` (default production, falling back to test with a
  note). Answers *"why is this code alive?"* — the evidence chain behind reachability colors.
- Wildcard edges (dynamic constructs) appearing in a path are rendered explicitly
  (`—[dynamic import, possible]→`) so weak links are visible, not laundered.

### 4.6 `kondo impact <selector> [--if-deleted]`
Forward-looking blast radius, built on the same machinery as diff-mode derived effects
(RFC 0004 §6):

- Default: the reverse closure of the node (who is affected if it changes), grouped by depth and
  color, plus affected roots — *"what do I retest if I touch this?"*.
- `--if-deleted`: simulate removal on the patched graph and report the finding flips it would
  cause — symbols that become unreachable or test-only, dependencies that become unused, files
  orphaned. Simulation only: nothing is written. This lets an agent *plan* a deletion and know
  the full cleanup set before editing a single line.

### 4.7 `kondo batch` — many questions, one process

Per-invocation cost (process start + cache revalidation, ~120 ms warm) dwarfs per-query cost
(~a few ms on the loaded graph). An agent exploring a subsystem asks dozens of questions;
paying startup dozens of times wastes both wall-clock and the 500 ms mental budget. Two
amortization levels:

1. **Multi-selector verbs.** Every verb accepts multiple selectors/patterns:
   `kondo used-by selA selB selC`. The `result` becomes an array of per-selector results in
   argument order (schema §8). `trace` takes repeated `--pair A,B` for multiple traces.
2. **`kondo batch`** — heterogeneous queries in one process: reads JSON Lines from stdin
   (one request per line: `{ "verb", "selectors": […], "flags": {…}, "id"? }`), revalidates the
   cache **once**, answers in input order as JSON Lines on stdout, one envelope per request,
   echoing the optional caller-supplied `id` for correlation.

```
$ kondo batch <<'EOF'
{"id":"q1","verb":"used-by","selectors":["src/billing/tax.ts#calcLegacyTax"],"flags":{"split_by_color":true}}
{"id":"q2","verb":"trace","flags":{"pairs":[["src/api/routes.ts","pkg:decimal.js"]]}}
{"id":"q3","verb":"impact","selectors":["src/billing/tax.ts#TaxTable"],"flags":{"if_deleted":true}}
EOF
```

Batch semantics:

- **Isolation:** a failing request (bad selector, no path) yields an error/status envelope on its
  line; the batch continues. The batch never partially mutates anything — all requests see the
  same graph snapshot, so answers are mutually consistent (no torn reads across lines).
- **Streaming:** responses are flushed per line as computed — an agent can pipeline.
- **Bounds still apply** per request (caps + `elided`); a batch is limited to 1000 requests
  (diagnostic + truncation status beyond that, guarding against runaway generation).
- Batch mode is JSON-only (no human format) and is the intended transport for a future
  `kondo serve`/MCP wrapper (§7): one MCP tool call ⇒ one batch line, same envelopes.

## 5. Agent workflow (worked example)

Goal: "remove the legacy tax path".

```
kondo find calcLegacyTax                        → selector src/billing/tax.ts#calcLegacyTax
kondo used-by src/billing/tax.ts#calcLegacyTax --split-by-color
                                                → 0 production, 2 test consumers
kondo impact src/billing/tax.ts#calcLegacyTax --if-deleted
                                                → also orphans TaxTable + frees pkg:decimal.js
<agent edits: deletes function, tests, TaxTable, dependency>
kondo check --staged                            → verifies: 4 fixed findings, 0 new, health +1
```

Four bounded calls replace reading five files into context, and the final `check` is the
machine-verifiable proof the cleanup is complete — the anti-slop loop closed end to end.
After `find`, the middle queries are independent — an agent that already knows its questions
collapses them into one `kondo batch` invocation (§4.7), paying startup once.

## 6. Exit codes & failure semantics

| Code | Meaning |
|------|---------|
| 0 | query answered (even if the answer is an empty list) |
| 1 | selector/path not found (`find` with zero hits, `trace` with no path) |
| 2 | kondo failed (bad selector syntax, ambiguous selector, no cache and cold build failed) |

The 0-vs-1 distinction is load-bearing for agents scripting checks like "assert nothing uses X
anymore" (`kondo trace roots:production X` → expect 1).

Multi-selector and batch runs report per-request status inside each envelope (`"status":
"ok" | "not-found" | "error"`); the process exit code is the *worst* individual status
(0 < 1 < 2), so single-question scripting semantics survive batching unchanged.

## 7. Non-goals (1.0)

- No arbitrary graph query language (Datalog/Cypher-style) — the fixed verbs cover the known
  workflows; a query API is the same post-1.0 item as custom analyses (RFC 0003 §6).
- No mutation verbs (`kondo clean` stays in the post-1.0 parking lot).
- No long-running server/MCP mode — but the verbs are deliberately shaped so a future
  `kondo serve` can expose them 1:1 as MCP tools without redesign (parking lot, ROADMAP).

## 8. Open questions

1. Flat verbs (`kondo uses`) vs. namespaced (`kondo graph uses`) — flat reads better and the verb
   set is small and closed; namespacing frees verb names for future features. Current draft: flat.
2. Should `describe` inline the first level of `uses`/`used-by` (saves a round-trip, grows
   payload)? Current draft: yes, capped at 10 per direction with `elided` counts.
3. `trace --all` path explosion policy: cap by `--max-paths` only, or also by path length?
