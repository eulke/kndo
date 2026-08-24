# For agents

kndo treats LLM coding agents as a first-class audience: after generating or deleting code,
an agent runs kndo and gets machine-checkable feedback that its edit is complete — nothing
new is orphaned, nothing it targeted survives. Set `KNDO_FORMAT=agent` in the agent's
environment and every command speaks this dialect by default.

## The agent format

```console
$ kndo check --staged --format agent
kndo 0.1.0 agent-format 1 | mode staged | cache warm | 312ms
result: 2 new, 1 fixed, net +1 | health 82.4 -> 84.1 (B) | baseline 412 acknowledged
new:
1. [kndo-a3f81c92e5d4] unused function src/billing/tax.ts:41 calcLegacyTax
2. [kndo-9c04d1b2aa7e] test-only function src/util/csv.ts:8 exportCsv
fixed:
3. [kndo-77b0e4f2c19d] unused dependency package.json date-fns
more: none
next: kndo check --format json
```

Line-oriented plain text optimized for context windows: maximum information per token,
deterministic grammar, no decoration. The same engine and guarantees as `--format json` —
the agent format is a *rendering* of the same result and can never carry information the JSON
lacks. Its grammar is versioned independently (`agent-format 1` in the header); a grammar
change bumps the version, and the old version stays available for a release cycle.

The grammar, normatively:

- **Header + `result:` lines always come first**, fixed field order, `|`-separated — an agent
  reads two lines and knows the outcome.
- **One finding = one numbered line**: `N. [id] <category> <subject_kind> <path:line> <name>`,
  with an optional indented `evidence:` line (populated by `cyclic`'s cycle path). Numbers are
  cheap references ("fix 1 and 3"); the bracketed ids are the durable anchors. There is no
  `fix:` line in the stream — the JSON carries no `remediation` field to render, so the
  format never fabricates one. Resolution guidance instead lives in the
  [findings playbook](#the-kndo-skill), installed alongside the code so it can be as detailed
  as a category needs without taxing every run's output.
- Findings appear in group order (defect, waste, risk, hygiene) within `new:` / `fixed:` /
  `findings:` blocks — the same triage order as every renderer.
- **Elision is always explicit**: a `more:` line closes every listing (`more: none` or a
  count plus the drill-down command). A model never has to guess whether it saw everything.
- **`next:` closes every response** with the relevant drill-down commands — affordances
  travel with the data, so the model needn't memorize the CLI.
- Confidence below `certain` is appended in parentheses (`(probable)`); severity is implied
  by category and never repeated.
- UTF-8, no ANSI, no glyphs; byte-stable across thread counts and cache states.

The navigation verbs render in the same grammar (`kndo used-by <sel> --format agent`):
numbered `[selector] kind path:line` entries plus the verb's specifics, same `more:`/`next:`
discipline.

## Stable finding ids

`id = "kndo-" + hash(category, subject_kind, path, symbol path, discriminator)`, 12 hex
chars — line numbers never participate. The loop this enables: read finding `kndo-3f2a…` →
fix it → re-run → **assert that id is absent**. Reformatting can't break the assertion; a
rename honestly produces a new id. The same ids key the [baseline](suppressions.md), so
"acknowledged" and "fixed" are machine-checkable states, not prose.

## Navigation before editing

The [graph verbs](navigation.md) answer "can I delete this?" *before* the edit:

```console
$ kndo used-by src/tax.ts#calcTax --format agent      # who depends on it (by_color: production 0 → safe)
$ kndo trace src/tax.ts#calcTax --format agent        # WHY is it alive — the path to cut
$ kndo impact src/tax.ts#calcTax --if-deleted --format agent   # everything that dies with it
```

Exit codes are informative, not failures: `0` found, `1` not found, `2` malformed request.

## Batching: kndo query

`kndo query` reads one JSON request per line from stdin and answers them all over a
**single** graph load — the cheap way to ask fifty questions:

```console
$ printf '%s\n' \
    '{"id":"q1","verb":"used-by","selectors":["src/tax.ts#calcTax"]}' \
    '{"id":"q2","verb":"impact","selectors":["src/tax.ts#calcTax"],"flags":{"if_deleted":true}}' \
  | kndo query
```

Request grammar, one JSON object per line:

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
  request shape works everywhere.
- Answers come back as one JSON Line per request, **in input order**; the shared `run` block
  appears only on the first line.
- Malformed lines and unknown verbs are reported per line on stderr without dropping the
  rest; empty lines are skipped.
- A batch is capped at 1000 requests; excess lines are counted and reported, never silently
  dropped.
- The process exit code is the worst individual status — single-question scripting semantics
  survive batching.

`kndo query` is JSON-only by design (it *is* the machine transport); run it on a terminal
without piped stdin and it tells you what it wants instead of hanging.

## The check loop

1. `kndo check --format agent` → the worklist, with ids.
2. Navigate (`used-by`, `trace`, `impact --if-deleted`) to plan a safe edit.
3. Edit.
4. `kndo check --diff <base> --format agent` → assert: targeted ids in `fixed:`, nothing
   unexpected in `new:`. `net` on the `result:` line is the one-token summary.

## The kndo skill

```console
$ kndo agents install
skill: installed at .agents/skills/kndo
link: .claude/skills/kndo -> ../../.agents/skills/kndo
skill: commit .agents/ and .claude/ so every agent session picks it up
```

`kndo agents install` writes a small, versioned documentation package that teaches an agent
everything above — plus a **findings-resolution playbook** with a prescribed recipe per
category, a fix-vs-suppress decision tree, and a triage order (defect → waste → risk →
hygiene → convention), so that resolving findings is consistent across agents and sessions
instead of improvised each time.

Layout: `.agents/skills/kndo/` (`SKILL.md` — the always-loaded entry point — plus
`references/*.md`, loaded on demand) is the harness-neutral copy; `.claude/skills/kndo` is a
relative symlink to it, so Claude Code and any other `.agents`-aware harness share one set of
files. Both paths are meant to be **committed** to the project. The installed files are
kndo-owned: re-running `kndo agents install` after a binary upgrade overwrites drifted or
outdated content — that is the update flow. `kndo init` prints a reminder when the skill
isn't installed yet.
