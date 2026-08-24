---
name: kndo
description: >-
  Use kndo to navigate a codebase through its reference graph and to find and
  fix what the codebase no longer needs. Trigger when exploring an unfamiliar
  project, before deleting or renaming code ("is this safe to remove?", "who
  calls this?"), when asked about dead code, unused dependencies, duplication,
  cycles, or project health, and whenever interpreting or resolving kndo
  findings. Prefer kndo's graph verbs over grep for symbol-level questions —
  they answer in a fraction of the tokens.
version: {{version}}
---

# kndo

<!-- Installed by `kndo agents install`. These files are owned by kndo and are
     overwritten on reinstall — put project-specific notes elsewhere. -->

kndo is a multi-language static analyzer (JS/TS, Rust, Go, Java, Kotlin, Swift, JSON,
CSS/SCSS) that builds one reference graph across the whole project and reports what nothing
references anymore: dead code, unused/phantom dependencies, test-only code, duplicates,
cycles, untested-and-complex code. It also answers graph questions directly — who uses this,
why is it alive, what dies with it — which is the token-efficient way to explore a codebase.

Set the output dialect once and every command speaks it by default:

```sh
export KNDO_FORMAT=agent
```

Run kndo from the project root (the directory with `kndo.toml` or the repo root). There is no
path argument — any stray positional token is a usage error (exit 2).

## Token-efficient exploration (use this instead of grep)

For symbol-level questions ("where is X defined?", "who calls X?", "can I delete X?"),
querying the graph beats reading files: one command line returns ranked, structured answers
with reachability already computed. Reach for grep/file reads only for questions about text,
comments, or code kndo does not model.

Selectors name graph nodes:

| Selector | Names |
|---|---|
| `src/billing/tax.ts` | a file |
| `src/billing/tax.ts#calcTax` | a top-level symbol |
| `src/billing/tax.ts#TaxTable.lookup` | a member, owner-qualified |
| `dep:lodash` | a declared dependency |
| `pkg:@acme/core` | a workspace package |
| `roots:production` \| `roots:test` \| `roots:tooling` | a root set (`trace` endpoints only) |

Six read-only verbs (all accept multiple selectors; a bad selector fails only its own entry):

| Verb | Question it answers |
|---|---|
| `kndo find <pattern>` | search names → selectors (`--kind`, `--color`, `--lang`) |
| `kndo describe <sel>` | everything about one node: declaration, degree, roots, finding ids |
| `kndo uses <sel>` | what it depends on (outgoing edges) |
| `kndo used-by <sel>` | what depends on it — **the deletion question** |
| `kndo trace <sel>` | a concrete root→node path: WHY is this alive |
| `kndo impact <sel> --if-deleted` | the full blast radius: everything that dies with it |

Every node in an answer carries a reachability **color**: `production`, `test-only`,
`tooling-only`, or `unreachable`. The two patterns that matter most:

1. **Before deleting anything**, run `kndo used-by <sel>`. `by_color` with `production: 0`
   means nothing in production depends on it. Follow with
   `kndo impact <sel> --if-deleted` to see what else becomes unreachable and which
   dependencies are freed — the whole cleanup, before the edit.
2. **Asking more than two questions?** Batch them: `kndo query` reads one JSON request per
   line on stdin and answers all of them over a single graph load.

```sh
printf '%s\n' \
  '{"id":"q1","verb":"used-by","selectors":["src/tax.ts#calcTax"]}' \
  '{"id":"q2","verb":"impact","selectors":["src/tax.ts#calcTax"],"flags":{"if_deleted":true}}' \
  | kndo query
```

Full verb flags, envelope shapes, and orientation recipes: read
[references/navigation.md](references/navigation.md).

## The check loop

`kndo check` (or bare `kndo`) analyzes the project and returns findings, each with a stable
id `kndo-<12 hex>` that hashes the code object — never line numbers. The loop:

1. `kndo check --format agent` → the worklist, with ids.
2. Navigate (`used-by`, `trace`, `impact --if-deleted`) to plan a safe edit.
3. Fix per the playbook — read [references/findings-playbook.md](references/findings-playbook.md)
   before resolving anything; it prescribes the recipe, order, and verification per category.
4. `kndo check --diff <base> --format agent` → assert the targeted ids appear under
   `fixed:` and nothing unexpected under `new:`. The `net` figure on the `result:` line is
   the one-token summary.

Scoped runs: `--staged` (what `git commit` would commit) and `--diff <ref>` (vs
`merge-base(<ref>, HEAD)`) analyze only the change — much faster feedback than a full run.

## Exit codes

- `check`: `0` clean · `1` findings at/above `--fail-on` (`error|warning|info|none`;
  default `none` for full runs, `warning` for diff runs) · `2` kndo could not run.
- Navigation verbs: `0` found · `1` not found (a zero-hit `find`, a `trace` with no path —
  note an empty `used-by` is a legitimate `0`: "nobody uses it" IS the answer) ·
  `2` malformed request. Informative, not failures.

## Command cheat sheet

| Command | Purpose |
|---|---|
| `kndo health [--by-package]` | health score 0–100 with per-category breakdown |
| `kndo baseline [--update]` | acknowledge current findings in `.kndo/baseline.json` |
| `kndo doctor` | what kndo sees: adapters, cache, plugins, config |
| `kndo init [--hook]` | write `kndo.toml`; `--hook` installs the pre-commit hook |
| `kndo agents install` | (re)install this skill package |

## Load on demand

| When you need to | Read |
|---|---|
| Resolve findings with consistent criteria (recipes, order, fix-vs-suppress) | [references/findings-playbook.md](references/findings-playbook.md) |
| Deep-dive a verb's flags, the JSON envelope, or `kndo query` grammar | [references/navigation.md](references/navigation.md) |
| Parse output precisely (agent grammar, JSON schemas, SARIF) | [references/output-format.md](references/output-format.md) |
| Suppress a false positive or manage the baseline | [references/suppressions-and-baseline.md](references/suppressions-and-baseline.md) |
