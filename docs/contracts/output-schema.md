# Contract — JSON Output Schema

**Status:** Draft · Normative for `--format json`. Versioned: `schema_version` uses semver;
additive = minor, breaking = major (RFC 0006 §4). A machine-readable JSON Schema
(`schemas/kondo-output.schema.json`) is generated from the Rust types at build time and must
round-trip these examples in CI.

## 1. Envelope

```jsonc
{
  "schema_version": "1.0.0",
  "kondo_version": "0.3.1",
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
  "category": "unused-code",            // registry in §6
  "severity": "warning",                // "error" | "warning" | "info"
  "confidence": "certain",              // "certain" | "probable" | "possible"
  "message": "calcLegacyTax() is unreachable from any production or test root",
  "location": { "path": "src/billing/tax.ts", "range": { "start": [41,1], "end": [78,2] },
                "symbol": "calcLegacyTax", "symbol_kind": "function" },
  "related": [                           // evidence chain (also what `kondo explain` renders)
    { "role": "cause", "path": "src/billing/index.ts", "range": { "start": [12,1], "end": [12,42] },
      "note": "last production reference removed by this change" }
  ],
  "evidence": {                          // category-specific block, keyed by category
    "test_roots": [],                    // e.g. for test-only-code
    "kept_alive_by": []
  },
  "sources": ["adapter:js-ts"],          // provenance: adapters/plugins whose facts contributed
  "remediation": "Delete calcLegacyTax() (and its export in src/billing/index.ts).",
  "delta": "new"                         // diff modes: "new"; absent in full mode
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
    { "category": "unused-code",  "ratio": 0.031, "penalty": 6.2, "count": 47 },
    { "category": "duplication",  "ratio": 0.058, "penalty": 7.1, "tokens_duplicated": 8412 },
    { "category": "crap",         "penalty": 4.0, "crapload": 1912.4, "coverage": "lcov (2d old)" }
  ]
}
```

## 5. Finding id stability

`id = "kndo-" + hash(category, project-relative path, symbol path (not line numbers), category-specific
discriminator)`, truncated to 12 hex chars. Line/column changes do **not** change the id; renames
and moves do (a rename is a different code object). Guarantees: an agent that fixes finding X can
re-run kondo and assert X is absent; a baseline survives reformatting.

## 6. Category registry (1.0)

`unused-code`, `test-only-code`, `unused-file`, `unused-dependency`, `undeclared-dependency`,
`duplicate-code`, `crap`, `stale-suppression`. New categories are additive (minor bump);
consumers must ignore unknown categories.

## 7. SARIF mapping

`category` → `rule.id`; `severity` → SARIF `level` (error/warning/note); evidence chain →
`relatedLocations`; confidence → `properties.confidence`. One run object per kondo run.
