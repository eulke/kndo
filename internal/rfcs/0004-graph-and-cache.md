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

**Implementation status (updated at M4.5, 2026-08-20; original M2 note below):** the revisit
condition fired. The M4.5 benchmark suite's 50k fixture measured the all-or-nothing rebuild at
1 962 ms for a one-file change (E0b baseline) — the patch is required. M4.5 landed the
*surrounding* pieces first: step 2's stat-scan (a `(mtime, size) → blake3` sidecar with git's
racy-write guard — unchanged files are no longer even read), step 6's persist off the critical
path, and parallel per-file resolution (RFC 0008 §2), which together brought the 50k one-file
change to ~1 130 ms. **Step 4's patch then landed as RFC 0013** (design and invariants of
record there): a one-file change at 50k now costs ~575 ms — the same as a no-op, which is the
target this section set. The v1 guard is RFC 0013 §5's (surface-signature equality, file set
unchanged, no manifests, ≤ 5% changed — measured, stricter than this section's 30% sketch);
the equivalence gate is byte-identity of the patched graph against a scratch rebuild,
enforced by a dedicated suite over the conformance corpora. The design notes below are kept
as the record of the analysis that produced RFC 0013:

- *v1 guard (body-only fast path):* patch only when the file **set** is unchanged and every
  changed file's **surface is identical** — same declarations (name/kind/exported/visibility/
  member_of, spans free to move), same imports/re-exports, same roots, same unit/unit_name,
  same detected origin and class, and no manifest touched. That is the dominant pre-commit
  case (editing bodies), and it makes the dirty set exactly the changed files: nobody else's
  resolution can change. Everything outside the guard falls back to the full rebuild — the
  fallback-honesty rule above, applied maximally at first.
- *Old-facts comparison:* the guard compares fresh facts against the old facts fetched from
  the facts cache by the file's **old** content hash (already cached), via a span-normalized
  surface signature. Old facts missing ⇒ full rebuild.
- *Splice mechanics:* FileIds are stable (path-sorted, set unchanged); a surface-identical
  change keeps each changed file's symbol run identical in names/order, so SymbolIds are
  stable too — the patch updates spans/signature_spans in place, regenerates the changed
  files' owned edges (Declares/References/ImportsFile/ImportsDependency/Wildcard/in-source
  roots — ownership is derivable from edge shape), their function_metrics, suppressions, and
  per-file diagnostics, and leaves every other file's contributions untouched.
- *Tables for one-file re-resolution:* almost everything comes from the loaded snapshot
  (symbols → bare/qualified/unit/member tables); the two facts-only inputs are the import
  targets' `unit_name` (qualifier defaults) and barrel re-export aliases — fetch just the
  changed files' direct import targets' facts (bounded, cache-hot) or persist those two onto
  the snapshot.
- §5's incremental reachability-color propagation is **explicitly deprioritized with data**:
  after the CSR/bitset rewrite (RFC 0008 §3), full recoloring costs 13 ms at 50k files —
  two orders below the phases the patch addresses. It stays accepted design with a measured
  trigger instead of a guess.

**Original M2 note (2026-08-20):** step 4's *patch* — reusing part of a stale
graph — and §5's incremental BFS/reachability-color propagation are **not built**; today any
change to the discovered file set is a full graph rebuild (all-or-nothing, keyed as in §3), with
only the facts layer staying warm per file. This was deliberately deferred rather than blocking
M2: measured on the 5k-file benchmark repo, a single changed file still rebuilds in ~84 ms
(facts-cache-warm) against the 500 ms budget, so the fallback-honesty path this section already
requires (§5, full recompute) turned out sufficient at benchmark scale without the patch
algorithm existing yet. This section remains the accepted target design — revisit if profiling on
larger real repos shows the all-or-nothing rebuild cost growing past budget.

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
