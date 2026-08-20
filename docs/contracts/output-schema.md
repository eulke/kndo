# Contract — JSON Output Schema

**Status:** Accepted · Normative for `--format json`. Versioned: `schema_version` uses semver;
additive = minor, breaking = major (RFC 0006 §4). A machine-readable JSON Schema
(`schemas/kndo-output.schema.json`) is generated from the Rust types at build time and must
round-trip these examples in CI.

## 1. Envelope

```jsonc
{
  "schema_version": "1.0.0",
  "kndo_version": "0.3.1",
  "run": {
    "mode": "staged",                    // "full" | "staged" | "diff"
    "base_ref": null,                    // set for "diff"
    "started_at": "2026-08-18T12:00:00Z",
    "duration_ms": 312,
    "cache": "warm",                     // "warm" | "cold" | "partial" | "disabled"
    "project_root": ".",
    "adapters": [ { "id": "js-ts", "files": 1240 } ],
    "plugins":  [ { "id": "nextjs", "activated_by": "detected: dependency react" } ]
  },
  "health": { /* §4 */ },
  "budget": {                            // diff modes, only when [delta] rules are configured (RFC 0006 §5)
    "verdict": "fail",                   // "pass" | "fail"
    "rules": [
      { "rule": "max-health-drop", "limit": 0.0, "measured": 1.7, "verdict": "pass" },
      { "rule": "max-net-findings", "limit": 0, "measured": 1, "verdict": "fail", "over_by": 1 }
    ]
  },
  "findings": [ /* §2 — in diff modes: only new findings */ ],
  "fixed": [ /* §3 — diff modes only */ ],
  "baseline": { "acknowledged": 412, "stale": 3 },
  "suppressed": { "inline": 9, "config": 2 },
  "diagnostics": [ { "level": "warn", "message": "coverage report older than 7d — ignored" } ]
}
```

## 2. Finding

```jsonc
{
  "id": "kndo-a3f81c92e5d4",            // stable content-anchored id, §5
  "category": "unused",                 // verdict; registry in §6
  "group": "waste",                     // the verdict's nature: defect | waste | risk | hygiene (fixed mapping, §6)
  "subject_kind": "function",           // what the verdict landed on: symbol kind | file | directory | dependency | import | suppression
  "severity": "warning",                // "error" | "warning" | "info"
  "confidence": "certain",              // "certain" | "probable" | "possible"
  "message": "calcLegacyTax() is unreachable from any production or test root",
  "location": { "path": "src/billing/tax.ts", "range": { "start": [41,1], "end": [78,2] },
                "symbol": "calcLegacyTax", "package": "@org/billing" },   // owning workspace package (RFC 0011)
  "rolled_up": null,                    // file/directory rollups: count of subsumed findings
  "related": [                           // evidence chain (also what `kndo explain` renders)
    { "role": "cause", "path": "src/billing/index.ts", "range": { "start": [12,1], "end": [12,42] },
      "note": "last production reference removed by this change" }
  ],
  "evidence": {                          // category-specific block, keyed by category
    "test_roots": [],                    // e.g. for test-only
    "kept_alive_by": []
  },
  "sources": ["adapter:js-ts"],          // provenance: adapters/plugins whose facts contributed
  "remediation": "Delete calcLegacyTax() (and its export in src/billing/index.ts).",
  "delta": "new",                        // diff modes: "new"; absent in full mode
  "delta_origin": "derived"              // diff modes: "introduced" (inside the change set — dead on arrival) | "derived" (flipped by it); RFC 0004 §6
}
```

## 3. Fixed finding (diff modes)

Same shape as a finding, with `"delta": "fixed"` and the *previous* location. Lets CI/agents
credit improvements and lets pre-commit output celebrate deletions.

## 4. Health

```jsonc
{
  "score": 84, "grade": "B",
  "previous": { "score": 82, "grade": "B" },    // from last snapshot, if any
  "categories": [
    { "category": "unused-symbols", "ratio": 0.031, "penalty": 6.2, "count": 47 },
    { "category": "duplication",  "ratio": 0.058, "penalty": 7.1, "tokens_duplicated": 8412 },
    { "category": "crap",         "penalty": 4.0, "crapload": 1912.4, "coverage": "lcov (2d old)" }
  ]
}
```

## 5. Finding id stability

`id = "kndo-" + hash(category, subject_kind, project-relative path, symbol path (not line numbers),
category-specific discriminator)`, truncated to 12 hex chars. Line/column changes do **not** change the id; renames
and moves do (a rename is a different code object). Guarantees: an agent that fixes finding X can
re-run kndo and assert X is absent; a baseline survives reformatting.

## 6. Category registry (1.0)

Categories are pure verdicts (RFC 0005 taxonomy rule):
`unused`, `test-only`, `untested`, `undeclared`, `unresolved`, `version-skew`, `duplicate`,
`internal-only`, `private-type-leak`, `cyclic`, `deep-import`, `crap`, `stale`.
New categories are additive (minor bump); consumers must ignore unknown categories.

Each category maps to exactly one `group` — `defect` (unresolved, undeclared, version-skew,
private-type-leak), `waste` (unused, test-only, duplicate, internal-only), `risk` (crap, cyclic,
untested, deep-import), `hygiene` (stale) — normative mapping in RFC 0005. The
field is redundant with `category` by design: it is included so consumers section and sort
without maintaining the mapping themselves. New groups are additive; consumers must render
unknown groups after known ones rather than dropping their findings.

What the verdict landed on travels in `subject_kind`: the `SymbolKind` names from
[core-traits.md](core-traits.md) in kebab-case, plus `file`, `directory`, `package`,
`dependency`, `import`, `suppression`. Suppression/config targets may append the subject as
`category:subject` (e.g. `unused:enum-member`, `test-only:dependency`). Subject kinds are
additive like categories and are not a registry of their own. Human renderers compose the
two (`unused (dependency)`); JSON consumers filter on either axis independently.

## 7. SARIF mapping

`category` → `rule.id`; `severity` → SARIF `level` (error/warning/note); evidence chain →
`relatedLocations`; confidence → `properties.confidence`. One run object per kndo run.

## 8. Query envelopes (navigation verbs, RFC 0007)

All navigation verbs share one envelope; `result` is verb-specific. Listings are always capped
and carry explicit `elided` counts (RFC 0007 §2) — consumers must treat `elided > 0` as "there is
more", never as "that's all".

```jsonc
{
  "schema_version": "1.0.0",
  "query": { "verb": "used-by", "selectors": ["src/billing/tax.ts#calcLegacyTax"],
             "flags": { "depth": 1, "split_by_color": true }, "id": "q1" },   // id: query-mode echo, optional
  "run": { "cache": "warm", "duration_ms": 74 },
  "status": "ok",                        // "ok" | "not-found" | "error" (per request)
  "results": [ { /* one verb-specific result per selector, argument order */ } ],
  "diagnostics": []
}
```

Verbs accept multiple selectors; `results` always aligns 1:1 with `query.selectors` (a failed
selector yields an inline `{ "status": "not-found" | "error", … }` entry without failing its
siblings). In `kndo query` mode (RFC 0007 §4.7) this same envelope is emitted as one JSON Line
per request, in input order, `run` appearing only on the first line (shared graph snapshot).

Common building blocks:

```jsonc
// NodeRef — every node mention, everywhere:
{ "selector": "src/billing/tax.ts#TaxTable.lookup", "kind": "method",
  "color": "test-only", "span": { "path": "src/billing/tax.ts", "start": [90,3], "end": [104,4] } }

// EdgeRef — every edge mention:
{ "edge": "references", "confidence": "certain",
  "site": { "path": "src/billing/index.ts", "start": [12,10], "end": [12,23] } }
```

Verb result shapes (fields beyond these are additive/minor):

- **find**: `{ "matches": [NodeRef…], "elided": N }` — ranked.
- **describe**: `{ "node": NodeRef, "declaration": {…}, "degree": { "in": {...by edge kind}, "out": {…} },
  "reached_by_roots": [NodeRef…], "metrics": { "cyclomatic": 14, "crap": 36.2, "coverage": 0.12 },
  "findings": [finding-id…], "uses": [ {NodeRef, via: EdgeRef}… ], "used_by": [ … ],
  "elided": { "uses": N, "used_by": M } }`.
- **uses / used-by**: `{ "node": NodeRef, "entries": [ { "node": NodeRef, "via": EdgeRef,
  "depth": 1 }… ], "by_color": { "production": N, "test-only": M, "tooling": K },
  "elided": N }`.
- **trace**: `{ "from": NodeRef, "to": NodeRef, "paths": [ { "hops": [ { "node": NodeRef,
  "via": EdgeRef }… ], "weakest_confidence": "possible" }… ], "paths_elided": N }` —
  liveness traces set `"from"` to the root found.
- **impact**: `{ "node": NodeRef, "affected": [ { "node": NodeRef, "via": EdgeRef,
  "depth": N }… ], "by_color": {…}, "elided": N,
  "affected_roots": [ { "kind": "production"|"test"|"tooling", "node": NodeRef }… ],
  "affected_roots_elided": N, "if_deleted": { "newly_unreachable": [NodeRef…],
  "newly_unreachable_elided": N, "newly_test_only": [NodeRef…],
  "newly_test_only_elided": N, "freed_dependencies": [name…] } }` — `if_deleted` present
  only with the flag. `affected` reuses uses/used-by's depth-annotated entry shape (one
  grammar, not two); the simulation reports *typed reachability flips* rather than
  synthesized §2 finding objects — the flips are the graph-level fact, and fabricating
  finding ids/messages for findings that don't exist yet would put untruths in the envelope.

Query exit codes are defined in RFC 0007 §6 and are part of this contract.

## 9. Agent format (`--format agent`)

A line-oriented plain-text rendering of the same data, optimized for LLM context windows:
maximum information per token, deterministic grammar, no decoration. Versioned independently of
the JSON schema (`agent-format 1` in the header); grammar changes bump the version and the old
version stays available for one release cycle, like JSON majors.

```
kndo 0.3.1 agent-format 1 | mode staged | cache warm | 312ms
result: 3 new, 2 fixed, net +1 | health 82.4 -> 84.1 (B) | baseline 412 acknowledged
budget: fail (2/3) | health-drop<=0.0 ok +1.7 | defects=0 ok 0 | net<=0 FAIL +1 over-by 1
new:
1. [kndo-a3f81c92e5d4] unused function src/billing/tax.ts:41 calcLegacyTax
   cause: last production reference removed by src/billing/index.ts:12 (this change)
   fix: delete calcLegacyTax() and its export in src/billing/index.ts:12
2. [kndo-9c04d1b2aa7e] test-only function src/util/csv.ts:8 exportCsv (2 test roots: src/util/csv.test.ts)
   fix: delete exportCsv() together with its tests
fixed:
3. [kndo-77b0e4f2c19d] unused dependency package.json date-fns
more: none
next: kndo explain <id> | kndo used-by <selector> --format agent
```

Grammar rules (normative):

- **Header + result lines always first**, fixed field order, `|`-separated. An agent reads two
  lines and knows the outcome.
- **`budget:` line** appears only when `[delta]` rules are configured (RFC 0006 §5): overall
  verdict + one `rule op limit ok|FAIL measured [over-by N]` segment per rule — a failing agent
  reads `over-by` and knows exactly how much work remains, without interpretation.
- **One finding = one numbered line**: `N. [id] <category> <subject_kind> <path:line> <name>`,
  followed by optional indented `cause:` / `fix:` / `evidence:` lines. Numbers let a model refer
  to findings cheaply ("fix 1 and 3"); ids are the durable anchors.
- **Findings appear in group order** (defect, waste, risk, hygiene) within `new:` / `fixed:` /
  `findings:` blocks — same triage order as every other renderer.
- **Elision is always explicit**: `more: 47 unused (kndo check --only unused --format agent)`
  or `more: none`. A model must never have to guess whether it saw everything.
- **`next:` closes every response** with the drill-down commands relevant to what was shown —
  affordances travel with the data, so the model needn't memorize the CLI.
- Confidence below `certain` is appended in parentheses (`(probable)`); severity is implied by
  group/category and never repeated per line.
- Navigation verbs (RFC 0007) render in the same grammar: numbered entries of
  `[selector] kind path:line` plus the verb's specifics (depth, via-edge, cycle path), same
  `more:`/`next:` discipline. `kndo query` (JSONL) is unaffected — it stays JSON by nature.
- Encoding: UTF-8, no ANSI, no glyphs, stable across `--threads` and cache states (RFC 0008 §4).

The agent format is a *rendering* of `RunResult`/`QueryResult` — it can never carry information
absent from the JSON, and anything added to it must land in the JSON schema first. Like JSON and
SARIF it renders **core-side** (machine formats, contracts §5): every frontend — CLI today,
`kndo serve`/MCP tomorrow — emits byte-identical agent text.
