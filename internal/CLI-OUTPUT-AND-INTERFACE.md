# CLI, Output & Interface

The user-facing surface: commands and configuration (RFC 0006), read-only graph navigation
(RFC 0007), the terminal's visual language (RFC 0009), and CI integration via the GitHub Action
(RFC 0010). Each section below was originally its own RFC document; they are merged here per
the consolidation recorded in `.wayfinder/tickets/33-consolidation-decision.md`.

## RFC 0006: CLI and output

**Status:** Accepted · **Depends on:** RFC 0001, 0004, 0005 · **Normative schema:** [contracts/output-schema.md](CONTRACTS.md)

### 1. Design tenets

- **Zero-config first run.** `kndo check` in any repo produces a useful report with no setup
  (ADR 0006). Config only *adjusts*; it is never required.
- **One mental model.** Every mode is "compute findings, show the relevant slice". Flags select
  the slice, not different engines.
- **Two audiences, one engine.** Human output optimizes for scanning; `--format json|sarif` is
  the same data, schema-versioned for agents/CI. No information exists in one and not the other.

### 2. Commands

```
kndo check [PATHS…]           # full analysis (default command: `kndo` = `kndo check`)
    --staged                   # scope report to effects of the staged changes
    --diff <ref>               # scope report to effects of changes vs merge-base(ref)
    --format human|json|sarif|agent  # default: human on TTY, json when piped; KNDO_FORMAT env overrides the default
    --fail-on <severity>       # exit-code threshold (default: warning in diff modes, none in full)
    --only <cats> / --skip <cats>
    --strict                   # promote severities (see RFC 0005) — see the note below
    --no-cache                 # bypass cache (CI correctness check, debugging)
kndo explain <finding-id>     # full evidence chain for one finding, human or --format json
kndo health                   # health score + category breakdown + trend vs previous snapshots

# graph navigation (read-only, warm-cache; full spec in RFC 0007)
kndo find <pattern>           # name → selector (files, symbols, packages)
kndo describe <selector>      # everything the graph knows about one node
kndo uses <selector>          # outgoing dependencies (--depth, --transitive)
kndo used-by <selector>       # incoming dependents (--split-by-color: safe-to-delete signal)
kndo trace <from> [<to>]      # concrete path A→B, or root→X liveness trace (why is X alive?)
kndo impact <selector>        # blast radius; --if-deleted simulates removal → finding flips
kndo query                    # composite queries: JSONL on stdin, one graph load, JSONL answers

kndo init                     # write minimal kndo.toml, .gitignore entry, offer pre-commit hook
kndo baseline [--update]      # create/refresh baseline from current findings (RFC 0006 §6)
kndo doctor                   # what was detected: adapters, plugins active & why, cache state, timings
kndo plugin install <coord>   # install a plugin (+ deps) into the global directory (RFC 0015 §4)
kndo plugin list              # installed plugins (plugins.lock) + hand-dropped files
kndo plugin remove <coord>    # remove a managed plugin; doctor reports any dependency gap left
```

**`--strict`, as landed:** severity promotion only, and today exactly one category promotes —
`undeclared` warning → error, which is the only promotion RFC 0005 actually specifies. The
"stricter confidence floor" half of the line above was cut when the flag was implemented: the
confidence floor already has two knobs pointing in both directions (`[analysis]
min-confidence` raises it, `--verbose` drops it to `possible`), and a third that moves it
again would be a second way to say what one of those already says. Which direction "stricter"
even meant was never settled — fewer findings you are surer of, or every tier including the
speculative one — and a flag whose meaning has two defensible readings is worse than no flag.

`--only`/`--skip` landed with a distinction this section did not draw: `--skip` is suppression
(the `[analysis] skip` policy from another source — it unions with the file, counts as
suppressed, and cannot silence `stale`), while `--only` is a lens whose narrowing is *counted
and reported* rather than exempted. See [cli.md](../docs/src/cli.md#--only-and---skip).

`--staged`/`--diff` report the **findings delta** (new + fixed, including derived effects far from
the touched files — RFC 0004 §6), not "findings inside touched files".

### 3. Human output

Compact, grouped, colored; every finding shows its id (for `explain`/suppression) and confidence
when below `certain`. This section defines *what* is shown; the visual language — colors, glyphs,
layout grammar, capability degradation, stream discipline — is RFC 0009, and it binds only the
CLI frontend: rendering lives outside the core (Engine boundary, contracts §5). Diff mode leads
with the delta and the health movement:

```
kndo · staged · 3 new · 2 fixed · net +1

  health   82.4 ──▶ 84.1   +1.7 ↑   B
  budget   health-drop ≤ 0.0   +1.7  ✓
           net findings ≤ 0      +1  ✗   over by 1        FAIL

NEW (introduced by this change)
  unused     src/billing/tax.ts:41  calcLegacyTax() — added but nothing uses it
NEW (derived, in untouched code)
  test-only  src/util/csv.ts:8      exportCsv() now only reached from tests (2 test roots)
             └ last production reference removed by src/billing/index.ts:12 (this change)
FIXED
  unused (dependency)  package.json  "date-fns" — first production usage added
```

The header answers "better or worse?" in one line; the budget block shows **every configured
tolerance with its measured value and verdict** (§5) — a FAIL is never mysterious, and `over by`
states exactly how much must be fixed to pass. NEW findings split by `delta_origin`
(RFC 0004 §6): *introduced* (you added something unwired) before *derived* (your change had
effects at a distance).

Full mode renders one section per **group**, in fixed order — defects, waste, risk, hygiene
(RFC 0005 taxonomy rule 4) — because that is the reader's triage order: fix what's broken,
delete what's dead, then plan refactors. Within a section, findings group by category with
counts, worst-first, truncated with `… and N more (kndo check --only unused)`:

```
DEFECTS (2)
  unresolved  src/api/client.ts:3   import "./transpor" resolves to nothing
  …
WASTE (51)
  unused      src/legacy/           directory unreachable — 14 files, safe to delete
  …
RISK (7)
  crap        src/billing/tax.ts    calcTax() CRAP 41 (complexity 9, coverage 0%)
  …
```

Diff mode uses the same group order inside its NEW and FIXED sections.

### 4. Machine output

- `--format json`: the versioned envelope in contracts/output-schema.md — findings, health
  breakdown, delta section in diff modes, diagnostics, timings. **Schema stability contract:**
  additive changes bump minor; anything else bumps major and keeps the previous major available
  via `--schema <ver>` for one release cycle.
- `--format sarif`: SARIF 2.1.0 mapping (category → ruleId) for GitHub code scanning et al.
- `--format agent`: a **token-frugal plain-text format designed for LLM consumption**
  (grammar in contracts §9). JSON is for programs; an LLM pays 3–5× the tokens for JSON's
  structural overhead and doesn't need it to parse. The agent format keeps every machine anchor
  (finding ids, selectors, counts) in a deterministic line grammar, drops all decoration, and
  states its affordances inline (which command shows more). Same information as JSON — nothing
  exists in one format only. An agent harness sets `KNDO_FORMAT=agent` once and every kndo
  invocation in that session answers in it, `check` and navigation verbs alike.
- Agent ergonomics, all formats: ids are stable across runs (content-anchored, not
  line-anchored — see contracts §finding-id), so an agent can act on a finding, re-run, and
  verify that exact id disappeared.

### 5. Exit codes & delta budgets

| Code | Meaning |
|------|---------|
| 0 | ran; nothing at/above `--fail-on` and every delta budget holds |
| 1 | ran; findings at/above `--fail-on` **or** a delta budget exceeded |
| 2 | kndo failed (bad config, unreadable repo, internal error) — never fails a commit silently |

**Delta budgets** turn "tolerable" into declared policy, layered on top of `--fail-on`
(diff modes only):

```toml
[delta]                      # defaults: strict ratchet
max-health-drop = 0.0        # health never drops
max-net-findings = 0         # new − fixed ≤ 0: pay for what you dirty

[delta.budget]               # finer tolerances, by group or category
defect = 0                   # new defects: never
duplicate = 2                # up to 2 new clones tolerated per change
```

Semantics, chosen to keep budgets from becoming normalized decay:

- Defaults are the strict ratchet (0 / 0); any positive tolerance is an explicit, visible
  opt-in. The **baseline never grows automatically** regardless of budgets — a budget loosens
  the *gate of one change*, never the recorded debt.
- Budgets evaluate **against the merge-base, not cumulatively across pushes** of the same
  branch/PR — pushing a fix must not "recharge" the allowance.
- `fixed` findings compensate only inside `max-net-findings`; per-group/category budgets are
  absolute (`defect = 0` means zero, even if the change fixes ten others).
- Every configured rule is reported with its measured value, verdict, and `over_by` when
  exceeded — in all formats (RFC 0009 §5, RFC 0010 §4, output-schema §1/§9).

Pre-commit recipe (`kndo init` offers to install it):

```bash
kndo check --staged --fail-on warning
```

### 6. Baseline & adoption path

`kndo baseline` snapshots current findings into `.kndo/baseline.json` (committed). Baselined
findings are excluded from failure counting and shown only as a one-line summary
(`baseline: 412 acknowledged`), while diff modes still catch every *new* finding. A fixed
baselined finding is auto-dropped on `--update`; the baseline can only shrink automatically —
growth requires an explicit `kndo baseline --update` in a reviewed commit. This makes day-one
adoption in a legacy repo non-punitive while ratcheting health monotonically.

### 7. Configuration (`kndo.toml`)

Optional, at project root; discovered upward like `.gitignore`. Everything has a default.

`[project]` (`roots`, `exclude`) was in this section's original sketch and is **not implemented,
and no longer proposed**. `kndo init` wrote it commented-out for a while and the engine never
read it; a commented key is still a promise, so it was removed rather than carried. Both things
it offered have working answers with sharper semantics: `.ignore` keeps files out of discovery
(gitignore syntax, read by the discovery layer, invisible to git), and `[[rule]]` with `paths`
keeps *verdicts* off files that stay in the graph — which is the one you usually want, since
dropping a file from discovery also drops every edge through it and can turn one silenced
finding into several new false ones elsewhere.

`[health.weights]` is the same kind of promise and is likewise **not implemented**:
`config::LIVE_TABLES` has no `health` entry, `parse` never reads one, and `kndo init`'s template
does not write one. RFC 0005 §11's weight table is fixed until a config surface for it ships —
not shown in the example below, per the same "wire it or leave it out" rule that removed
`[project]`.

```toml
[analysis]
skip = []                              # categories or category:subject, e.g. ["unused:enum-member"]
min-confidence = "probable"            # report floor; "possible" only with --verbose

[analysis.duplicate]
min-tokens = 50

[analysis.crap]
threshold = 30

[performance]
threads = 0                            # 0 = physical cores (RFC 0008 §5); --threads flag wins

[delta]                                # diff-mode gate budgets — semantics in §5
max-health-drop = 0.0

[[rule]]                               # per-path overrides
paths = ["examples/**"]
skip = ["unused"]                      # one verdict covers symbols, files and directories

[plugins.<name>]                       # RFC 0003 §4
```

`kndo.toml` plays no part in the graph cache key (RFC 0004 §3, deliberately): every table listed
above acts strictly post-assembly — filtering, thresholds, and suppression applied to an
already-built graph, never a change to what gets extracted or resolved — so a config edit takes
effect on the very next run without needing to invalidate anything cached.

### 8. Non-goals for 1.0

Watch mode, LSP/IDE server, HTML report, historical trend storage beyond the last snapshots
(external systems can archive the JSON), auto-fix/codemod (`kndo clean` is a tempting post-1.0
verb — deliberately deferred until confidence data has real-world mileage).

## RFC 0007: Graph navigation

**Status:** Accepted · **Depends on:** RFC 0001, 0004, 0006 · **Normative schema:** [contracts/output-schema.md](CONTRACTS.md) §8

### 1. Motivation

`kndo check` already pays the cost of building a whole-project semantic graph and keeping it warm
in `.kndo/`. That graph answers questions agents (and humans) currently burn enormous effort on
with grep-and-read loops: *who uses this? what does this depend on? why does A end up depending
on B? is this safe to delete?*

Navigation commands expose the graph **read-only** through the CLI, so an LLM agent can ask a
precise question and get a precise, bounded answer in milliseconds — replacing multi-file context
stuffing with a handful of cheap, verifiable tool calls. This is the second half of the "for
humans and agents" mission: `check` prevents waste; navigation prevents the wasteful *exploration*
that produces it.

### 2. Design tenets

1. **Read-only, warm, fast.** Navigation verbs never mutate findings or baseline. They revalidate
   the cache exactly like `check` (patching changed files first, RFC 0004 §4), so answers reflect
   the working tree, within the same < 500 ms warm budget. Startup + revalidation dominate that
   budget, so batching (§4.8) amortizes them: many questions, one process, one graph load.
2. **Bounded by default.** Every listing is capped (default 50 entries, `--limit`) with an explicit
   `"elided": N` count and deterministic ordering — an agent always knows whether it saw
   everything, and output can never blow up a context window.
3. **Selectors are stable addresses.** Every answer contains selectors (§3) that can be fed back
   into the next query verbatim. Chaining queries requires no parsing heuristics.
4. **Answers carry evidence.** Edges come with kind, confidence, and source span — an agent can
   jump straight to the proving line instead of trusting a summary.
5. **Same output discipline as `check`.** Human format on TTY, JSON (schema §8 of the output
   contract) when piped or with `--format json`, and the token-frugal agent format
   (output-schema §9) with `--format agent` / `KNDO_FORMAT=agent`. No information exists in
   one format only.

### 3. Selectors

A selector uniquely addresses a graph node:

| Node | Syntax | Example |
|------|--------|---------|
| File | project-relative path | `src/billing/tax.ts` |
| Symbol | `path#name`, nested via `.` | `src/billing/tax.ts#TaxTable.lookup` |
| Dependency (external) | `dep:<name>` | `dep:lodash` |
| Package (workspace unit, RFC 0011) | `pkg:<name>` | `pkg:@org/ui` |
| Root set | `roots:production` \| `roots:test` \| `roots:tooling` | `roots:production` |

Ambiguity (e.g. overloads) is an error listing the concrete candidates — never a guess.
`kndo find` (§4.1) is the discovery verb that turns names into selectors.

### 4. Verbs

#### 4.1 `kndo find <pattern>`
Search files and symbols by name (substring + qualified-suffix match, ranked exact > prefix >
substring). Filters: `--kind function|class|file|…`, `--color unreachable|test-only|…`,
`--lang <adapter-id>`. Returns selectors + kind + reachability color + declaring span.
The entry point of every agent workflow: name → selector.

#### 4.2 `kndo describe <selector>`
Everything the graph knows about one node, in one call:
declaration (span, kind, visibility, exported), file role/origin & reachability color, direct degree
(in/out, by edge kind), roots that reach it (nearest first), metrics (cyclomatic, LOC, tokens,
plus CRAP and coverage **only when a report was ingested** — an absent measurement is reported
absent, never as zero), one entry per callable *shape* rather than per symbol; duplication group
membership (the `duplicate` findings this node is a member of, with every member's selector);
open findings attached to it, provenance (adapter/plugins that produced its facts). For a `dep:` selector: manifest scope, importing files
count, usage status. For a `pkg:` selector: mode (library/app), member counts, dependents.
For files: declared symbols (capped).

#### 4.3 `kndo uses <selector>`
Outgoing dependencies: what this node needs. `--depth N` (default 1), `--transitive`
(fixpoint, deduplicated, depth-annotated), `--edges imports|references|all` (default all).
Answers *"what am I pulling in?"* — e.g. before extracting a module.

#### 4.4 `kndo used-by <selector>`
Incoming dependents: who needs this node. Same flags as `uses`, plus `--split-by-color` to
separate production, test-only, and tooling consumers. `used-by X` returning only test consumers
is the *"safe to delete (with its tests)"* signal — the query twin of the `test-only`
finding. Empty result ⇒ matches an `unused` finding by construction.

#### 4.5 `kndo trace <from> [<to>]`
Explain connectivity as concrete paths, every hop with edge kind, confidence, and span:

- `kndo trace A B` — directed path(s) from A to B. Default: one shortest path;
  `--all --max-paths K` for alternatives; exit code 1 (no-path) tells an agent decoupling
  succeeded. Answers *"why does A depend on B?"*.
- `kndo trace X` (single argument) — **liveness trace**: shortest path from the nearest root to
  X, `--roots production|test|tooling|all` (default production, falling back to test with a
  note). Answers *"why is this code alive?"* — the evidence chain behind reachability colors.
- Wildcard edges (dynamic constructs) appearing in a path are rendered explicitly
  (`—[dynamic import, possible]→`) so weak links are visible, not laundered.

#### 4.6 `kndo impact <selector> [--if-deleted]`
Forward-looking blast radius, built on the same machinery as diff-mode derived effects
(RFC 0004 §6):

- Default: the reverse closure of the node (who is affected if it changes), grouped by depth and
  color, plus affected roots — *"what do I retest if I touch this?"*.
- `--if-deleted`: simulate removal on the patched graph and report the finding flips it would
  cause — symbols that become unreachable or test-only, dependencies that become unused, files
  orphaned. Simulation only: nothing is written. This lets an agent *plan* a deletion and know
  the full cleanup set before editing a single line.

#### 4.7 `kndo explain <finding-id>`
Everything the graph knows about one already-reported finding, in one call: the finding itself
verbatim (message, evidence chain, provenance, rollup count) paired with `describe` (§4.2) of
its subject (color, roots that reach it, degree, the other findings on the same node).
Deliberately a pair, not a new derivation — re-deriving either half would be a second answer to
a question `describe` or the analysis already answered. A finding id absent from the current
run is `not-found`, never "no such finding": it may have been fixed, suppressed, or acknowledged
in the baseline since it was last seen. A verb over a *finding* rather than a selector — its
"selector" is an id — but otherwise the same machinery as every other verb: same envelope
shape, same batching through `kndo query`.

#### 4.8 Batching & `kndo query` — many questions, one process

Per-invocation cost (process start + cache revalidation, ~120 ms warm) dwarfs per-query cost
(~a few ms on the loaded graph). An agent exploring a subsystem asks dozens of questions;
paying startup dozens of times wastes both wall-clock and the 500 ms mental budget. Two
amortization levels:

1. **Every verb is batched.** All verbs accept multiple selectors/patterns:
   `kndo used-by selA selB selC`. The `results` array holds one per-selector result in
   argument order (schema §8). `trace` takes repeated `--pair A,B` for multiple traces.
2. **`kndo query`** — the composite query interface: heterogeneous questions in one process.
   Reads JSON Lines from stdin (one request per line: `{ "verb", "selectors": […],
   "flags": {…}, "id"? }`), revalidates the cache **once**, answers in input order as JSON
   Lines on stdout, one envelope per request, echoing the optional caller-supplied `id` for
   correlation.

```
$ kndo query <<'EOF'
{"id":"q1","verb":"used-by","selectors":["src/billing/tax.ts#calcLegacyTax"],"flags":{"split_by_color":true}}
{"id":"q2","verb":"trace","flags":{"pairs":[["src/api/routes.ts","dep:decimal.js"]]}}
{"id":"q3","verb":"impact","selectors":["src/billing/tax.ts#TaxTable"],"flags":{"if_deleted":true}}
EOF
```

`kndo query` semantics:

- **Isolation:** a failing request (bad selector, no path) yields an error/status envelope on its
  line; the run continues. Nothing is ever mutated — all requests see the same graph snapshot,
  so answers are mutually consistent (no torn reads across lines).
- **Streaming:** responses are flushed per line as computed — an agent can pipeline.
- **Bounds still apply** per request (caps + `elided`); a query run is limited to 1000 requests
  (diagnostic + truncation status beyond that, guarding against runaway generation).
- `kndo query` is JSON-only (no human format) and is the intended transport for a future
  `kndo serve`/MCP wrapper (§7): one MCP tool call ⇒ one request line, same envelopes.
- The individual verbs (§4.1–4.7) are sugar over the same engine: one verb ≡ a single-line
  query. Future composition features (joins, set operations over results) belong to `kndo
  query`, keeping the verbs simple — tracked as open question 4 (§8).

### 5. Agent workflow (worked example)

Goal: "remove the legacy tax path".

```
kndo find calcLegacyTax                        → selector src/billing/tax.ts#calcLegacyTax
kndo used-by src/billing/tax.ts#calcLegacyTax --split-by-color
                                                → 0 production, 2 test consumers
kndo impact src/billing/tax.ts#calcLegacyTax --if-deleted
                                                → also orphans TaxTable + frees dep:decimal.js
<agent edits: deletes function, tests, TaxTable, dependency>
kndo check --staged                            → verifies: 4 fixed findings, 0 new, health +1
```

Four bounded calls replace reading five files into context, and the final `check` is the
machine-verifiable proof the cleanup is complete — the anti-slop loop closed end to end.
After `find`, the middle queries are independent — an agent that already knows its questions
collapses them into one `kndo query` invocation (§4.8), paying startup once.

### 6. Exit codes & failure semantics

| Code | Meaning |
|------|---------|
| 0 | query answered (even if the answer is an empty list) |
| 1 | selector/path not found (`find` with zero hits, `trace` with no path) |
| 2 | kndo failed (bad selector syntax, ambiguous selector, no cache and cold build failed) |

The 0-vs-1 distinction is load-bearing for agents scripting checks like "assert nothing uses X
anymore" (`kndo trace roots:production X` → expect 1).

Multi-selector and `kndo query` runs report per-request status inside each envelope
(`"status": "ok" | "not-found" | "error"`); the process exit code is the *worst* individual
status (0 < 1 < 2), so single-question scripting semantics survive batching unchanged.

### 7. Non-goals (1.0)

- No arbitrary graph query language (Datalog/Cypher-style) — the fixed verbs cover the known
  workflows; a query API is the same post-1.0 item as custom analyses (RFC 0003 §6).
- No mutation verbs (`kndo clean` stays in the post-1.0 parking lot).
- No long-running server/MCP mode — but the verbs are deliberately shaped so a future
  `kndo serve` can expose them 1:1 as MCP tools without redesign (parking lot, ROADMAP).

### 8. Open questions

1. ~~Flat verbs (`kndo uses`) vs. namespaced (`kndo graph uses`)?~~ **Resolved: flat.**
   `internal/README.md`'s open-questions resolution log records the decision: flat reads better
   and the verb set is small and closed; namespacing would free verb names for features nothing
   currently plans to add.
2. ~~Should `describe` inline the first level of `uses`/`used-by`?~~ **Resolved: no.** `describe`
   answers "everything about *this* node"; the neighbourhood is a different question and `uses`
   is one round-trip away, with its own `--depth`, `--edges` and `--limit` that an inlined,
   fixed-cap copy could not offer. `reached_by_roots` stays because it is a property of the node
   (what keeps it alive), not a listing of its neighbours.
3. ~~`trace --all` path explosion policy: cap by `--max-paths` only, or also by path length?~~
   **Resolved: both, plus a total-expansion budget.** The DFS enumeration (`enumerate_paths` in
   `crates/kndo-core/src/query.rs`) stops at `--max-paths` results, restricts every path to a
   length ceiling of `shortest_len + 2` (so "alternatives" stays close to the shortest path
   rather than "every walk in the graph"), and bounds total edge expansions by a fixed
   `TRACE_EXPANSION_BUDGET` — three independent limits so a pathological graph can't blow the
   query budget even before `max_paths` is reached.
4. How much composition does `kndo query` grow before it *is* the deferred query language —
   1.0 draft: independent requests only (no joins/set operations/piping between lines);
   result-set composition is evaluated post-1.0 with real agent usage data.

## RFC 0009: Human interface

**Status:** Accepted · **Depends on:** RFC 0005 (groups), RFC 0006 (commands), contracts §5 (Engine)

### 1. Scope & position

This RFC owns everything a human *sees* in the terminal. It binds only `kndo-cli`: the core
returns data (`RunResult`), the CLI renders it — the separation is contractual (contracts §5),
so any future frontend (LSP, GUI, `kndo serve`) can define its own presentation without
touching this document or the core. Machine formats (JSON/SARIF, and the LLM-oriented agent
format) are out of scope here — they are serialized core-side and schema-governed
(output-schema §9).

### 2. Design principles

1. **Scannable in two seconds.** The first line answers "am I fine?"; the layout answers "what
   do I fix first?" without reading everything. Triage order = group order (RFC 0005 rule 4).
2. **Quiet success.** A clean run's header is one line (`kndo · clean · 214 files (198 claimed,
   1205 symbols, 340 deps, 2618 edges) · 187ms`), followed by the health line when health
   tracking is on. No banners, no ASCII art, no emoji noise, no advertising. Silence is the
   reward.
3. **Semantic color, never decorative.** Color encodes exactly two things: the finding's group
   and delta polarity (new/fixed). If it's colored, it means something; if it means something,
   it's *also* expressed without color (§4) — color-blind users and CI logs lose nothing.
4. **Stable geometry.** Same columns, same order, same indentation every run — muscle memory is
   an interface. New information may append, never reshuffle.
5. **Every finding is actionable in place.** Each line carries its id (feeds `explain` and
   suppressions) and a `path:line` the terminal can make clickable. Dead ends are forbidden:
   truncation always names the command that shows the rest.

### 3. Visual vocabulary

| Semantic | Color | Prefix glyph (unicode / ASCII fallback) |
|----------|-------|------------------------------------------|
| group `defect` | red | `✗` / `x` |
| group `waste` | yellow | `◦` / `o` |
| group `risk` | magenta | `▲` / `^` |
| group `hygiene` | blue | `·` / `.` |
| group `convention` | cyan | `•` / `?` |
| delta `fixed` | green | `✓` / `+` |
| evidence / secondary | dim | `└` / `\`- ` |
| health up / down | green / red | `↑` / `↓` (`^`/`v`) |

- Category and confidence render as **text** (`unused`, `(probable)`) — never encoded only in
  color or glyph (principle 3).
- One accent color per line maximum; paths and messages stay in default foreground. kndo output
  should look calm next to a compiler's.
- `convention` is reserved for plugin-contributed findings (`category` is an open
  `plugin:<coordinate>/<rule>` namespace) — no core analysis emits it, but it carries its own
  color and glyph so a plugin's findings render with the same discipline as every other group.

### 4. Capability degradation

Detection order, no configuration required:

1. **Rich TTY**: truecolor/256 + unicode → full vocabulary.
2. **Basic TTY**: 8/16 colors, or unstable width → same layout, basic colors, ASCII glyphs.
3. **No TTY / CI log / `NO_COLOR` / `--color never`**: plain text; glyph column keeps the ASCII
   fallbacks so grep-ability and meaning survive (`x`, `o`, `^`, `+`).
4. **`TERM=dumb`**: additionally no cursor tricks, no width fitting — plain lines only.

`--color auto|always|never` (default `auto`); `NO_COLOR` env always wins over `auto`.
Width: fit to terminal width with truncation-by-column-priority (message truncates before path;
id never truncates); below 60 columns, fall back to two-line-per-finding layout.

### 5. Layout grammar

One finding = one primary line, optional evidence lines, fixed column order:

```
<glyph> <category>[:<subject>] <path:line>  <message> [confidence] [id]
        └ <evidence>  (cause / kept-alive-by / cycle path…)
```

- Groups render as **sections** with a count header (`WASTE (51)`), in fixed group order; empty
  sections are omitted, not shown as zero.
- Within a section: sorted by severity, then path, then span — deterministic (RFC 0008 §4
  applies to rendering too).
- Rollup findings state their scope in the message (`directory unreachable — 14 files`), and
  `explain` unrolls them.
- Diff mode: `NEW (introduced by this change)` then `NEW (derived, in untouched code)` then
  `FIXED` blocks (`delta_origin` split, RFC 0004 §6), each internally in group order; the header
  line always shows counts and net (`3 new · 2 fixed · net +1`), followed by the health movement
  line and — when any `[delta]` budget is configured — the **budget block**: one line per rule
  with limit, measured value, and verdict glyph; failures append `over by N` (exactly how much
  to fix), and a health drop near a grade boundary appends the distance (`B (1.9 from C)`).
  The PASS/FAIL word closes the block — the gate is never mysterious.

  ```
  health   84.1 ──▶ 81.9   −2.2 ↓   B  (1.9 from C)
  budget   health-drop ≤ 1.0   −2.2  ✗   over by 1.2      FAIL
  ```
- Health block (in `kndo health` and full runs): score, grade, and per-category penalty bars
  built from `▁▂▃▄▅▆▇` (ASCII: `#` scaled) — a shape, not a chart; details stay tabular.

### 6. Streams, progress & verbosity

- **stdout** carries the report and nothing else; **stderr** carries progress and diagnostics.
  In `--format json`, stdout is pure parseable JSON — the discipline that makes piping safe is
  absolute, and it holds for human format too.
- Warm runs show **no progress at all** (they're done before a spinner would spin). Cold runs
  print a single self-overwriting stderr line (`indexing 3 412/5 210 files`), TTY-only — CI logs
  get one start line and one end line instead.
- `--quiet`: header line + exit code only. `--verbose`: adds `possible`-confidence findings,
  per-phase timings, and cache state. Neither changes *what* was analyzed (RFC 0006 flags do).
- Errors speak human within what exists today: `EngineError` has exactly one variant
  (`ProjectRootNotFound`), and every command renders it the same way, a plain
  `eprintln!("kndo: {e}")` of `thiserror`'s `Display` message (`kndo: project root does not
  exist or is not a directory: /path/to/project`) — no separate probable-cause or next-command
  fields exist yet. A structured problem + probable cause + next command render, the way
  `cache locked by pid 4211 — another kndo is running; retry or kndo doctor` would read, is
  design intent for variants `EngineError` doesn't have, not current behavior. Never a bare
  Rust error chain outside `--verbose`.

### 7. Non-goals (1.0)

Interactive TUI (panes, filtering), themes/config for colors beyond `--color`, localization
(English only until the schema stabilizes), notification/sound hooks, markdown/HTML human
reports (the JSON feeds external renderers).

### 8. Open questions

1. Should `kndo health` render a sparkline of the last N snapshots (data exists in
   `.kndo/cache/`) or stay single-run until a real trend store lands post-1.0?
2. Glyph set on Windows legacy consoles (cmd.exe pre-Windows-Terminal): force ASCII always, or
   trust UTF-8 codepage detection?

## RFC 0010: CI, GitHub Action and PR reporting

**Status:** Implemented (M6 — `action/`, composite; dogfooded by `ci.yml`'s test job on kndo's own PRs) · **Depends on:** RFC 0004 (diff modes, cache), RFC 0006 (formats, exit codes),
contracts §5 (Engine boundary) · **Ships:** M6 (ROADMAP)

### 1. Goal

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

### 2. Architectural position

The Action is a **frontend** (contracts §5): it downloads the pinned kndo binary, invokes
`kndo check --diff <base> --format json`, and renders/publishes the result. It contains zero
analysis logic and reads only the JSON contract — meaning any CI system (GitLab CI, Buildkite,
Jenkins) can build the same integration against the same JSON without core changes; the GitHub
Action is simply the one we ship and dogfood. Comment markdown is presentation, so it is
**frontend-owned** (RFC 0009 principle applied to a different frontend), built from the JSON
delta envelope.

### 3. What a run does

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

### 4. Publishing surfaces

**Sticky comment (primary).** One comment per PR, **upserted in place** on every push —
identified by a hidden HTML marker (`<!-- kndo-report -->`) — never appended: a PR with thirty
pushes gets one living report, not thirty stale ones. `action/render.mjs` builds the markdown
directly from the JSON envelope (no CLI text is reused); layout:

```markdown
### kndo · 3 new · 2 fixed · health 82.4 → 84.1 (B) ↑

**New**
| | finding | where | why |
|-|---------|-------|-----|
| ✗ | `unresolved` import | `src/api/client.ts:3` | `./transpor` resolves to nothing |
| ◦ | `unused` function | `src/billing/tax.ts:41` | last production ref removed by this PR |

**Fixed** ✓ `unused` dependency `date-fns`

<details><summary>47 baseline findings unchanged</summary>…</details>

<details><summary>1 diagnostic</summary>

- kndo could not parse `src/legacy/vendor.min.js` — skipped
</details>

_2 finding(s) suppressed (inline: 1, config: 1)_

<sub>[run](…) · mode `diff` · fail-on `warning` · kndo 0.9.0</sub>
```

Group order and glyph vocabulary follow RFC 0009 §3 (rendered as text/emoji-safe equivalents);
findings sort by group then id; long sections collapse under `<details>` with a hard cap per
section and a link to the workflow run for the rest. Fixed findings always render — the reward
loop (RFC 0004 §6) applies to reviewers too. The diagnostics and suppressed-count lines appear
only when non-empty.

**No budget checklist.** `[delta]` rules are not evaluated or rendered by the Action at all —
the JSON envelope carries a `budget` block (RFC 0006 §5), but `action/render.mjs` never reads
it. The budget's only rendering is CLI-side: `kndo check` prints a plain rule-by-rule `ok`/`FAIL`
table (`kndo-cli/src/render.rs::budget_block`) to the terminal. A PR's merge gate is still
`fail-on` against the JSON envelope's findings (§3); the per-rule budget breakdown is visible
locally, not on any of the three PR-facing surfaces below.

**File annotations.** New findings of severity ≥ warning are emitted as GitHub annotations on
the diff (`::warning file=,line=`) so they appear inline in Files Changed. Capped (GitHub limit
~10/step honored explicitly, overflow noted in the comment).

**Job summary.** The full report is always written to `GITHUB_STEP_SUMMARY` — it works in every
context, including where comments are impossible.

**SARIF (opt-in).** `sarif: true` uploads the SARIF rendering to code scanning; findings then
also appear in the Security tab and as native PR review comments. Redundant with the sticky
comment by design — teams pick one or both.

### 5. Fork PRs & permissions

Declared permissions: `contents: read`, `pull-requests: write` (comment), `security-events:
write` (only with `sarif: true`). On fork PRs the default token cannot write comments: the
Action **degrades, never fails** — annotations + job summary still publish, and the comment is
skipped with a notice in the summary. No `pull_request_target` gymnastics in v1: kndo runs
static analysis on untrusted code safely (it never executes the analyzed project), but
write-token workflows on forks are a security decision we don't make for users.

### 6. Non-goals

- No auto-fix commits or suggested-change batches from CI (post-1.0, alongside `kndo clean`).
- No trend dashboards — the JSON is stable; external systems archive it (vision non-goal).
- No GitHub App in 1.0: the Action + SARIF cover the surface without hosting anything. A
  richer App (checks UI, org dashboards) is a parking-lot item that would consume the same
  JSON contract.
