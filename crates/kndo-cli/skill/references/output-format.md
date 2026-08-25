# Output formats reference

Format resolution, for every command: `--format` flag > `KNDO_FORMAT` env > tty autodetect
(`human` on a terminal, `json` when piped). Formats: `human`, `json`, `agent`, `sarif`
(check only; nav verbs support human/json/agent; `kndo query` and `kndo doctor` have fixed
formats). `NO_COLOR` disables color in human output.

## The agent format (`--format agent`)

Line-oriented plain text optimized for context windows: UTF-8, no ANSI, byte-stable across
thread counts and cache states. Versioned independently (`agent-format 1` in the header).
This is a rendering of the same result as `--format json` — it can never carry information
the JSON lacks.

### check, full run

```text
kndo 0.1.0 agent-format 1 | mode full | cache warm | 312ms
result: 2 findings | health 82.4 (B) | baseline 412 acknowledged
findings:
1. [kndo-9c04d1b2aa7e] undeclared import src/api/client.ts:3 lodash
2. [kndo-a3f81c92e5d4] unused function src/billing/tax.ts:41 calcLegacyTax
more: none
next: kndo check --format json
```

### check, diff/staged run

```text
kndo 0.1.0 agent-format 1 | mode staged | cache warm | 118ms
result: 1 new, 1 fixed, net +0 | health 82.4 -> 84.1 (B)
new:
1. [kndo-58ab12f0c3d4] cyclic file src/state/store.ts:1 -
   evidence: src/state/actions.ts:4 store.ts → actions.ts → store.ts
fixed:
2. [kndo-a3f81c92e5d4] unused function src/billing/tax.ts:41 calcLegacyTax
more: none
next: kndo check --format json
```

The grammar, normatively:

- Header first: `kndo <version> agent-format <n> | mode <full|staged|diff> | cache <state> | <n>ms`.
- `result:` second: `clean` or `N findings` (full) / `N new, M fixed, net ±K` (diff), then
  optional `|`-separated segments: `health <score> (grade)` or `health <a> -> <b> (grade)`,
  `baseline N acknowledged`, `suppressed N inline, M config` (only when non-zero).
- One finding = one numbered line:
  `N. [id] <category> <subject_kind> <path:line> <name> [(confidence)]` — space-separated;
  `-` stands in for a missing path or name; the parenthesized confidence appears only below
  `certain`. Numbering runs in group order (defect, waste, risk, hygiene) and continues
  across `new:` into `fixed:`.
- Optional indented `   evidence: <path:line> <note>` lines follow a finding (populated by
  `cyclic`'s cycle path). **There are no `fix:` or `cause:` lines** — remediation lives in
  [findings-playbook.md](findings-playbook.md), not in the stream.
- `more:` closes every listing (`more: none` or an elided count) — never guess whether you
  saw everything; `next:` closes every response with a drill-down command.

Numbers are cheap in-conversation references; the bracketed ids are the durable anchors —
use ids, not numbers, when tracking a fix across runs.

### Navigation verbs, agent form

Same grammar family: header (`| verb <verb> |`), a `status:` line, one block per selector
(prefixed `[N] <selector>` only when the request had several), numbered
`[selector] kind path:line` node lines plus the verb's specifics (`by_color` totals on
impact's `affected:` line, `path N: A -[edge, confidence]-> B` chains on trace,
`if_deleted:` sub-blocks), the same `more:`/`next:` discipline.

## JSON

The normative schemas ship in the repo: `schemas/kndo-output.schema.json` (check) and
`schemas/kndo-query-output.schema.json` (nav verbs / query).

Check envelope top level: `schema_version`, `kndo_version`, `run`, `findings[]`, `fixed[]`
(diff modes), `health`, `baseline`, `suppressed`, `diagnostics[]`. A finding:
`{id, category, group, subject_kind, severity, confidence, message,
location{path, range, symbol, package}, related[], delta, delta_origin, advisory}` —
`related[]` is the evidence chain (`{role, path, range, note}`), `delta` is `new|fixed` in
diff modes, `advisory: true` marks findings that never gate.

Query envelope: `schema_version`, `kndo_version`, `query{verb, selectors, id}`,
`run{cache, duration_ms}`, `status`, `results[]` (1:1 with selectors, in order),
`diagnostics[]`.

Stable ids: `kndo-` + 12 hex of hash(category, subject_kind, path, symbol path,
discriminator) — line numbers never participate. Reformatting cannot break an id assertion;
a rename honestly produces a new id.

## SARIF

`kndo check --format sarif` for CI/code-scanning upload. Findings only — no health or
baseline blocks.

## Environment

`KNDO_FORMAT` (default format), `KNDO_THREADS`, `KNDO_PLUGIN_DIR`, `NO_COLOR`. Piping is
safe: SIGPIPE is restored to default, so `kndo … | head` behaves like any Unix filter.
