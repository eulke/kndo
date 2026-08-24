# Getting started

## First run

From any project root:

```console
$ kndo
```

That is `kndo check` in full-scan mode: every language adapter claims its files, the project
graph is assembled, and every analysis runs. No configuration is required — manifests
(`package.json`, `go.mod`, `Cargo.toml`, `pom.xml`, `build.gradle`, `Package.swift`) tell
kndo what your entry points and dependencies are.

On a terminal you get the human report; when stdout is piped, kndo emits pure JSON instead
(`kndo | jq .findings` just works). See [CLI reference](cli.md#output-formats) for the full
format-selection rules.

## Reading the human report

```console
$ kndo
✗ undeclared src/api/client.ts:3  lodash is imported but not declared by package web [kndo-4f19c2aa07b3]
◦ unused (dependency) package.json  date-fns is declared but never imported [kndo-a3f81c92e5d4]
◦ unused src/billing/tax.ts:41  calcLegacyTax() is unreachable from any production or test root [kndo-77b0e4f2c19d]
▲ cyclic src/state/store.ts:1  4 files form an import cycle [kndo-9c04d1b2aa7e]
   └ evidence: src/state/store.ts → src/state/actions.ts → src/state/selectors.ts → src/state/store.ts

health   82.4  B
  unused-symbols       ▃  −6.2  (47)
  duplication          ▄  −7.1  (8412 tokens)
  crap                 ▂  −4.0  (12, load 1912.4, coverage none)
```

- Findings are grouped in a fixed triage order — **defect** (`✗`), **waste** (`◦`),
  **risk** (`▲`), **hygiene** (`·`) — worst first. Severity is implied by the group and
  category; it is never repeated per line.
- Each line reads `<glyph> <category>[:<subject>] <path:line> <message> [(confidence)] [id]`.
  Confidence is shown only when it is below `certain`. The bracketed id is stable: it hashes
  the code object (category, path, symbol), never line numbers, so it survives reformatting.
- Findings that carry an evidence chain (cycles, for example) render it as indented `└` lines.
- The health block appears after the findings: score, grade, and the non-zero per-category
  penalties. `kndo health` shows the full table — see [Health & coverage](health.md).
- A clean run is one line: file/symbol/edge counts and the run duration.
- `--quiet` collapses everything to a single summary line; `--verbose` adds per-phase timings
  and cache state.

Diagnostics — a coverage report that is too old, a file that failed to parse — go to
**stderr**, in every format. stdout is always the pure report.

## Exit codes at a glance

- `0` — clean (no findings at or above the threshold).
- `1` — findings at or above `--fail-on`.
- `2` — kndo itself could not run (bad flag, unresolvable diff base, broken project root).
  An analysis that did not run can never read as a clean pass.

Full mode defaults to `--fail-on none` (exploratory — a legacy repository's pre-existing
findings shouldn't fail a plain `kndo`); diff modes default to `--fail-on warning` (a gate
should gate).

## Adopting kndo on an existing codebase

Acknowledge everything that exists today, then keep new waste out:

```console
$ kndo baseline
kndo: baseline written — 412 findings acknowledged (.kndo/baseline.json)
```

Baselined findings stop appearing in reports and never fail a run; the report header keeps an
honest count (`baseline: 412 acknowledged`). When an acknowledged issue is actually fixed,
its entry goes stale and `kndo baseline --update` drops it — the baseline only ever shrinks
on its own; it never grows without an explicit, reviewable `--update`.
Details: [Suppressions & baseline](suppressions.md).

## The pre-commit gate

```console
$ kndo init --hook
kndo.toml: written
.gitignore: added .kndo/
pre-commit hook: installed at .git/hooks/pre-commit
```

`kndo init` writes a fully-commented `kndo.toml` (every setting optional — see
[Configuration](configuration.md)) and adds `.kndo/` to `.gitignore`. With `--hook` it also
installs a pre-commit hook — but only if `.git/hooks/pre-commit` doesn't already exist; kndo
never clobbers an existing hook. The hook is one line:

```sh
exec kndo check --staged --fail-on warning
```

It analyzes exactly what `git commit` would commit (the index) against `HEAD`.

## Diff modes

```console
$ kndo check --staged          # index vs HEAD (what the pre-commit hook runs)
$ kndo check --diff main       # working tree vs merge-base(main, HEAD)
```

Both analyze the **before** and **after** trees fully and report the difference:

- `NEW (introduced by this change)` — findings whose subject is inside your change: code that
  is dead on arrival.
- `NEW (derived, in untouched code)` — findings your change *flipped* elsewhere: you removed
  the last production reference, and a distant symbol became unreachable.
- `FIXED` — findings present before and gone after. Deleting dead code shows up as a win.

The header carries the net (`2 new · 3 fixed · net −1`) and the health movement
(`health 82.0 ──▶ 84.1  +2.1 ↑`). Pre-existing findings never appear and never gate — diff
mode judges the change, not the repository.

## Everyday commands

| Command | What it does |
|---|---|
| `kndo` / `kndo check` | full scan |
| `kndo check --staged` / `--diff <ref>` | judge a change by its blast radius |
| `kndo health [--by-package]` | 0–100 score with per-category penalties ([Health & coverage](health.md)) |
| `kndo baseline [--update]` | acknowledge current findings ([Suppressions & baseline](suppressions.md)) |
| `kndo doctor` | what kndo sees: adapters, cache, plugins and why each is active |
| `kndo find` / `describe` / `uses` / `used-by` / `trace` / `impact` | graph navigation ([Graph navigation](navigation.md)) |
| `kndo query` | batched navigation over stdin ([For agents](agents.md)) |
| `kndo plugin …` | install, list, remove, author plugins ([Plugins](plugins.md)) |

Next: the [CLI reference](cli.md) for every flag, or the
[Findings reference](rules.md) to understand what kndo just told you.
