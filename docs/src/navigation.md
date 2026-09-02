# Navigation

The analysis builds a graph; the query verbs read it. They answer from the same
index the `unused` judgment ran on, so `used-by` is empty exactly where
`unused` accused, and `trace` shows exactly the path that kept a file alive.

```text
kndo find <pattern>...        nodes by name: exact, then prefix, then substring
kndo describe <selector>...   one node in full: reach, keepers preview, findings on it
kndo uses <selector>...       what a node depends on: imports and referenced names, resolved
kndo used-by <selector>...    what keeps a node alive — the deletion question, with sites
kndo trace <selector>...      why is this alive: the shortest root-to-node path
kndo impact <selector>...     what transitively depends on a node
kndo explain <finding-id>...  everything behind one finding id
```

## Selectors

| Form | Names |
|---|---|
| `src/store.ts` | a file |
| `src/store.ts#Store` | a symbol declared in that file |
| `src/store.ts#Store.drop` | a member of an owner |
| `kndo-c4258d23a768` | a finding, for `explain` |

Every input gets its own result in order, so one invocation answers many
questions. Reachability comes back as a **color**: `production`, `test-only`,
`tooling-only`, or `unreachable` — which root kind reaches the node, with
production outranking the others.

## Options

| Option | Verbs | Meaning |
|---|---|---|
| `--root <dir>` | all | project root (default: the current directory) |
| `--limit <n>` | all | listing cap (default 50); elision is always reported, never silent |
| `--kind <kind>` | `find` | keep only this kind — a symbol kind, or `file` |
| `--reach <color>` | `find` | keep only this reachability color |
| `--roots <kind>` | `trace` | root set to trace from (default: production, then test, then tooling) |
| `--to <selector>` | `trace` | directed form: the shortest path from each input **to** this node |
| `--if-deleted` | `impact` | also simulate the removal and report the reachability flips |
| `--format` | all | `json` (the default when piped) or `agent` (the default on a terminal) |

## Examples

```text
$ kndo find release
[scripts/releaseUtils.ts] file · tooling-only
[scripts/releaseUtils.ts#releaseTag] function · tooling-only · 1-3
[scripts/detect-release.ts] file · tooling-only
elided: 0
next: kndo describe scripts/releaseUtils.ts · kndo used-by scripts/releaseUtils.ts
```

```text
$ kndo used-by scripts/releaseUtils.ts --format json
{
  "schema": "kndo-query/1",
  "verb": "used-by",
  "results": [
    {
      "status": "ok",
      "node": { "selector": "scripts/releaseUtils.ts", "kind": "file", "color": "tooling-only" },
      "kept_by": [
        { "kind": "binding", "site": { "path": "scripts/detect-release.ts", "lines": { "start": 1, "end": 1 } } }
      ],
      "by_color": { "tooling-only": 1 },
      "elided": 0
    }
  ]
}
```

`impact --if-deleted` is a simulation on the real graph: the node is removed,
reachability is recomputed, and the nodes whose color flips are listed — never
fabricated findings. The JSON answers follow the `kndo-query/1` schema, pinned
by the test suite; the agent answers are the grammar in [Agents](agents.md),
each ending with the `next:` verbs worth running.
