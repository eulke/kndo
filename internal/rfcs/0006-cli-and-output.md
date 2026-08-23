# RFC 0006 — CLI, Output & Configuration

**Status:** Accepted · **Depends on:** RFC 0001, 0004, 0005 · **Normative schema:** [contracts/output-schema.md](../contracts/output-schema.md)

## 1. Design tenets

- **Zero-config first run.** `kndo check` in any repo produces a useful report with no setup
  (ADR 0006). Config only *adjusts*; it is never required.
- **One mental model.** Every mode is "compute findings, show the relevant slice". Flags select
  the slice, not different engines.
- **Two audiences, one engine.** Human output optimizes for scanning; `--format json|sarif` is
  the same data, schema-versioned for agents/CI. No information exists in one and not the other.

## 2. Commands

```
kndo check [PATHS…]           # full analysis (default command: `kndo` = `kndo check`)
    --staged                   # scope report to effects of the staged changes
    --diff <ref>               # scope report to effects of changes vs merge-base(ref)
    --format human|json|sarif|agent  # default: human on TTY, json when piped; KNDO_FORMAT env overrides the default
    --fail-on <severity>       # exit-code threshold (default: warning in diff modes, none in full)
    --only <cats> / --skip <cats>
    --strict                   # promote severities (see RFC 0005), stricter confidence floor
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

`--staged`/`--diff` report the **findings delta** (new + fixed, including derived effects far from
the touched files — RFC 0004 §6), not "findings inside touched files".

## 3. Human output

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

## 4. Machine output

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

## 5. Exit codes & delta budgets

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

## 6. Baseline & adoption path

`kndo baseline` snapshots current findings into `.kndo/baseline.json` (committed). Baselined
findings are excluded from failure counting and shown only as a one-line summary
(`baseline: 412 acknowledged`), while diff modes still catch every *new* finding. A fixed
baselined finding is auto-dropped on `--update`; the baseline can only shrink automatically —
growth requires an explicit `kndo baseline --update` in a reviewed commit. This makes day-one
adoption in a legacy repo non-punitive while ratcheting health monotonically.

## 7. Configuration (`kndo.toml`)

Optional, at project root; discovered upward like `.gitignore`. Everything has a default.

```toml
[project]
roots = ["src", "packages/*"]          # default: auto (git ls-files minus ignores)
exclude = ["**/generated/**"]

[analysis]
skip = []                              # categories or category:subject, e.g. ["unused:enum-member"]
min-confidence = "probable"            # report floor; "possible" only with --verbose

[analysis.duplicate]
min-tokens = 50

[analysis.crap]
threshold = 30

[health.weights]                       # override RFC 0005 defaults
duplication = 25

[performance]
threads = 0                            # 0 = physical cores (RFC 0008 §5); --threads flag wins

[delta]                                # diff-mode gate budgets — semantics in §5
max-health-drop = 0.0

[[rule]]                               # per-path overrides
paths = ["examples/**"]
skip = ["unused"]                      # one verdict covers symbols, files and directories

[plugins.<name>]                       # RFC 0003 §4
```

Config hash participates in cache keys (RFC 0004 §3), so edits invalidate exactly what they affect.

## 8. Non-goals for 1.0

Watch mode, LSP/IDE server, HTML report, historical trend storage beyond the last snapshots
(external systems can archive the JSON), auto-fix/codemod (`kndo clean` is a tempting post-1.0
verb — deliberately deferred until confidence data has real-world mileage).
