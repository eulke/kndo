# RFC 0006 — CLI, Output & Configuration

**Status:** Draft · **Depends on:** RFC 0001, 0004, 0005 · **Normative schema:** [contracts/output-schema.md](../contracts/output-schema.md)

## 1. Design tenets

- **Zero-config first run.** `kondo check` in any repo produces a useful report with no setup
  (ADR 0006). Config only *adjusts*; it is never required.
- **One mental model.** Every mode is "compute findings, show the relevant slice". Flags select
  the slice, not different engines.
- **Two audiences, one engine.** Human output optimizes for scanning; `--format json|sarif` is
  the same data, schema-versioned for agents/CI. No information exists in one and not the other.

## 2. Commands

```
kondo check [PATHS…]           # full analysis (default command: `kondo` = `kondo check`)
    --staged                   # scope report to effects of the staged changes
    --diff <ref>               # scope report to effects of changes vs merge-base(ref)
    --format human|json|sarif  # default: human on TTY, json when piped
    --fail-on <severity>       # exit-code threshold (default: warning in diff modes, none in full)
    --only <cats> / --skip <cats>
    --strict                   # promote severities (see RFC 0005), stricter confidence floor
    --no-cache                 # bypass cache (CI correctness check, debugging)
kondo explain <finding-id>     # full evidence chain for one finding, human or --format json
kondo health                   # health score + category breakdown + trend vs previous snapshots

# graph navigation (read-only, warm-cache; full spec in RFC 0007)
kondo find <pattern>           # name → selector (files, symbols, packages)
kondo describe <selector>      # everything the graph knows about one node
kondo uses <selector>          # outgoing dependencies (--depth, --transitive)
kondo used-by <selector>       # incoming dependents (--split-by-color: safe-to-delete signal)
kondo trace <from> [<to>]      # concrete path A→B, or root→X liveness trace (why is X alive?)
kondo impact <selector>        # blast radius; --if-deleted simulates removal → finding flips
kondo query                    # composite queries: JSONL on stdin, one graph load, JSONL answers

kondo init                     # write minimal kondo.toml, .gitignore entry, offer pre-commit hook
kondo baseline [--update]      # create/refresh baseline from current findings (RFC 0006 §6)
kondo doctor                   # what was detected: adapters, plugins active & why, cache state, timings
```

`--staged`/`--diff` report the **findings delta** (new + fixed, including derived effects far from
the touched files — RFC 0004 §6), not "findings inside touched files".

## 3. Human output

Compact, grouped, colored; every finding shows its id (for `explain`/suppression) and confidence
when below `certain`. Diff mode leads with the delta and the health movement:

```
kondo · 3 new · 2 fixed · health 82 → 84 (B)

NEW
  unused     src/billing/tax.ts:41  calcLegacyTax() became unreachable
             └ last production reference removed by src/billing/index.ts:12 (this change)
  test-only  src/util/csv.ts:8      exportCsv() now only reached from tests (2 test roots)
  …
FIXED
  unused (dependency)  package.json  "date-fns" — first production usage added
```

Full mode renders one section per **group**, in fixed order — defects, waste, risk, hygiene
(RFC 0005 taxonomy rule 4) — because that is the reader's triage order: fix what's broken,
delete what's dead, then plan refactors. Within a section, findings group by category with
counts, worst-first, truncated with `… and N more (kondo check --only unused)`:

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
- Agent ergonomics: ids are stable across runs (content-anchored, not line-anchored — see
  contracts §finding-id), so an agent can act on a finding, re-run, and verify that exact id
  disappeared.

## 5. Exit codes

| Code | Meaning |
|------|---------|
| 0 | ran; nothing at/above `--fail-on` |
| 1 | ran; findings at/above `--fail-on` |
| 2 | kondo failed (bad config, unreadable repo, internal error) — never fails a commit silently |

Pre-commit recipe (`kondo init` offers to install it):

```bash
kondo check --staged --fail-on warning
```

## 6. Baseline & adoption path

`kondo baseline` snapshots current findings into `.kondo/baseline.json` (committed). Baselined
findings are excluded from failure counting and shown only as a one-line summary
(`baseline: 412 acknowledged`), while diff modes still catch every *new* finding. A fixed
baselined finding is auto-dropped on `--update`; the baseline can only shrink automatically —
growth requires an explicit `kondo baseline --update` in a reviewed commit. This makes day-one
adoption in a legacy repo non-punitive while ratcheting health monotonically.

## 7. Configuration (`kondo.toml`)

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

[[rule]]                               # per-path overrides
paths = ["examples/**"]
skip = ["unused"]                      # one verdict covers symbols, files and directories

[plugins.<name>]                       # RFC 0003 §4
```

Config hash participates in cache keys (RFC 0004 §3), so edits invalidate exactly what they affect.

## 8. Non-goals for 1.0

Watch mode, LSP/IDE server, HTML report, historical trend storage beyond the last snapshots
(external systems can archive the JSON), auto-fix/codemod (`kondo clean` is a tempting post-1.0
verb — deliberately deferred until confidence data has real-world mileage).
