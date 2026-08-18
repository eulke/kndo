# RFC 0004 — Project Graph Cache & Incremental Analysis

**Status:** Accepted · **Depends on:** RFC 0001, 0002 · **Related ADR:** 0004 (cache format)

## 1. Goal

Make the warm path — the pre-commit path — complete in **< 500 ms** by never redoing work whose
inputs did not change, while still reporting **derived effects**: findings that appear or
disappear far from the edited files because reachability changed.

## 2. What is cached

The cache lives in `.kndo/` at the project root (gitignored by default; `kndo init` adds it):

```
.kndo/
  cache/
    facts/<adapter>/<file-hash>.bin   # FileFacts per (adapter, content) — content-addressed
    graph.bin                         # last assembled Project Graph snapshot
    findings.bin                      # last full findings snapshot (for diffing)
  baseline.json                       # acknowledged legacy findings (RFC 0006 §6) — committed
```

- **Facts cache** — keyed by `(adapter id, facts schema version, file content hash)`. Content-
  addressed: renames, branch switches, and `git stash` all hit the cache; a file reverted to an
  old version re-hits its old entry. Pruned by LRU size cap (default 256 MB).
- **Graph snapshot** — the assembled graph plus the resolution inputs that shaped it (config hash,
  active plugin set, adapter versions). Loaded via zero-copy/mmap-friendly layout (ADR 0004).
- **Findings snapshot** — full analysis results of the last run, used to compute *new/fixed*
  findings and derived effects (§6).

Cache corruption or version mismatch is never an error: the affected layer is rebuilt from
scratch (a cold run), with a diagnostic.

## 3. Invalidation model

A cache entry's key is the hash of **all of its inputs**:

| Layer | Key inputs |
|-------|-----------|
| FileFacts | file content hash · adapter id+facts-schema-version |
| Graph | set of (path, content hash) · manifest hashes · kndo config hash · active plugins (id+version+wasm hash) · core graph-schema version |
| Findings | graph hash · enabled analyses + their config · coverage report hash (if any) |

There is no time-based invalidation and no reliance on mtimes for correctness (mtime+size is used
only as a fast-path hint to skip re-hashing unchanged files, à la git index).

## 4. Warm-run algorithm

1. Load graph snapshot header; verify config/plugin/schema keys. Mismatch ⇒ cold rebuild.
2. Compute changed file set `C`: stat-scan (or git index for `--staged` / `merge-base` diff for
   `--diff`), re-hash suspects, compare to snapshot.
3. For each file in `C`: fetch or compute `FileFacts` (only true content changes re-parse).
4. **Patch** the graph: remove nodes/edges owned by old versions of `C`, insert new facts,
   re-run resolution for: (a) files in `C`, (b) files whose *unresolved or resolved* imports could
   match paths added/removed in `C` (the resolver maintains a specifier→candidate index to find
   these), (c) plugin contributions whose declared inputs intersect `C`.
5. Re-run analyses on the **dirty region** (§5).
6. Diff findings vs. snapshot; persist new snapshots; report.

## 5. Dirty region = blast radius

Reachability-style analyses are global, but their *change* is local to the affected subgraph:

- Let `Δ` = nodes added/removed/re-resolved in step 4.
- **Dirty region** = `Δ` ∪ reverse-closure(`Δ`) ∪ forward-closure(`Δ`) over reference/import
  edges, bounded by fixpoint (in practice small for typical commits).
- Incremental reachability: kndo maintains per-node reachability colors
  (`production | test-only | unreachable`, RFC 0005 §3). After a patch, colors are recomputed
  only within the dirty region using standard incremental BFS with frontier re-validation;
  a change that flips a node's color propagates until colors stabilize.
- Analyses that are inherently pairwise (duplicate detection) use index structures instead:
  fingerprints of changed functions are (re)inserted into the global fingerprint index and only
  buckets touched by `Δ` are re-compared.

**Fallback honesty:** if the dirty region exceeds a threshold (default 30% of the graph — e.g.
after a big rebase), kndo falls back to a full recompute, which is still cache-warm for parsing.
Correctness never depends on the incremental path: `kndo check --no-cache` must produce
byte-identical findings, and CI runs both on a fixture matrix to enforce it.

## 6. Derived effects in diff modes

In `--staged` / `--diff` modes the *scope of reporting* is not "findings located in changed files"
but **"findings that changed because of the change set"**:

```
report = (findings_after − findings_before) ∪ (findings_before − findings_after)
         restricted to categories enabled for the mode
```

Both sides are computed on the full graph (warm), so:

- You delete the last production caller of `foo()` in `a.ts` ⇒ `foo()` in `b.ts` becomes
  *test-only* ⇒ reported as a **new** finding, attributed to your change, even though `b.ts` was
  never touched. The report includes the causal hint ("last production reference removed by
  `a.ts:42`").
- You add a first real usage of a dep ⇒ its `unused` (dependency) finding is reported as **fixed**.

Every delta finding carries a `delta_origin`: **`introduced`** (the finding sits inside the
change set itself — e.g. this diff adds a symbol nothing uses: *dead on arrival*, the moment an
agent or author can self-correct before committing) vs **`derived`** (the finding lives in
untouched code and flipped because of the change). Renderers may lead with introduced findings;
both fail the same gates.

Fixed findings are shown as positive deltas (they add to the health score movement, RFC 0005 §11),
which makes the pre-commit experience rewarding rather than purely punitive.

## 7. Concurrency & storage details

- Single-writer lock (`.kndo/cache/lock`) with graceful read-only degradation for concurrent runs.
- Hashing: blake3 (parallel, fast, collision-safe). Serialization & layout: ADR 0004.
- The cache is per-clone and disposable; nothing in `cache/` is ever committed. `baseline.json`
  is the only committed artifact and lives outside `cache/`.
