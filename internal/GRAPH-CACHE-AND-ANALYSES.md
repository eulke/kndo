# Graph, Cache & Analyses

## RFC 0004: Graph and cache

**Status:** Accepted · **Depends on:** RFC 0001, 0002 · **Related ADR:** 0004 (cache format)

### 1. Goal

Make the warm path — the pre-commit path — complete in **< 500 ms** by never redoing work whose
inputs did not change, while still reporting **derived effects**: findings that appear or
disappear far from the edited files because reachability changed.

### 2. What is cached

The cache lives in `.kndo/` at the project root (gitignored by default; `kndo init` adds it):

```
.kndo/
  cache/
    facts/<adapter>/<schema-version>-<file-hash>.bin   # FileFacts per (adapter, schema, content) — content-addressed
    graphs/<key>.bin     # content-addressed Project Graph snapshots — several coexist by design
    graphs/latest        # pointer to the most recently written key (the RFC 0013 patch's entry point)
    stat-index.bin        # (mtime, size) → blake3 sidecar — a fast-path hint, never a correctness input
    blob-hashes.bin        # git blob id → blake3 sidecar
    lock                    # single-writer advisory lock (§7)
  baseline.json         # acknowledged legacy findings (RFC 0006 §6) — committed
```

- **Facts cache** — keyed by `(adapter id, facts schema version, file content hash)`. Content-
  addressed: renames, branch switches, and `git stash` all hit the cache; a file reverted to an
  old version re-hits its old entry. Pruned by LRU size cap (default 256 MB), sharing one pool
  with graph snapshots (oldest-by-mtime evicted first, across both layers).
- **Graph snapshots** — content-addressed under `graphs/<key>.bin`, keyed as in §3; several
  coexist (the working tree's, plus diff modes' before/after tree states — a single mutable slot
  would ping-pong between them at a 0% hit rate). `graphs/latest` names the most recently written
  key, which is what the incremental patch (RFC 0013) loads on a key miss. Loaded via
  zero-copy/mmap-friendly layout (ADR 0004).

There is no findings cache: diff modes compute the new/fixed delta (§6) by assembling and
analyzing both tree states directly on each run, not by diffing against a findings snapshot
persisted from a previous one.

Cache corruption or version mismatch is never an error: the affected layer is rebuilt from
scratch (a cold run), with a diagnostic.

### 3. Invalidation model

A cache entry's key is the hash of **all of its inputs**:

| Layer | Key inputs |
|-------|-----------|
| FileFacts | file content hash · adapter id+facts-schema-version |
| Graph | set of (path, content hash) for every discovered file — manifests need no separate entry, since a manifest is just one more discovered file · each registered adapter's id+facts-schema-version · each registered graph-mutating plugin's identity (id, declared version, and — WASM only — component content hash) · `GRAPH_SCHEMA_VERSION` |

A kndo config hash is deliberately absent from the graph key, not an oversight: no config
subsystem shapes the graph today (every live `kndo.toml` knob acts post-assembly — RFC 0006 §7),
so folding one in would fake precision the cache doesn't have. Adding one is required before any
future config surface *does* affect assembly.

There is no time-based invalidation and no reliance on mtimes for correctness (mtime+size is used
only as a fast-path hint to skip re-hashing unchanged files, à la git index).

### 4. Warm-run algorithm

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

### 5. Dirty region = blast radius

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

### 6. Derived effects in diff modes

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

### 7. Concurrency & storage details

- Single-writer lock (`.kndo/cache/lock`) with graceful read-only degradation for concurrent runs.
- Hashing: blake3 (parallel, fast, collision-safe). Serialization & layout: ADR 0004.
- The cache is per-clone and disposable; nothing in `cache/` is ever committed. `baseline.json`
  is the only committed artifact and lives outside `cache/`.

## RFC 0013: Incremental graph patch

**Status:** Accepted, implemented (same milestone) · **Depends on:** RFC 0004 (§4 step 4 is the accepted target this
implements), RFC 0008, RFC 0012 · **Related:** ADR 0004

### 1. Problem, with the M4.5 numbers

RFC 0004 §4's warm-run algorithm promised a *patch*: reuse the previous graph and re-derive
only what a change touched. M2 deferred it; M4.5's benchmark suite fired the revisit trigger:

| 50k-file fixture, warm one-file change | ms |
|---|---|
| discovery (stat-scan, post-M4.5) | ~160 |
| facts fetch for 49 999 *unchanged* files | ~250 |
| resolve + link all 50k files (parallel, post-M4.5) | ~265 |
| everything else (analyses overlapped, render, process) | ~450 |
| **total** | **~1 130** |

The middle two rows are pure waste — work proportional to the repo instead of the change.
The patch replaces them with: load the previous snapshot (~50 ms), re-extract the changed
files (~1 ms each), re-resolve exactly the changed files, splice. Target: a one-file change
costs what a no-op costs, plus the change itself.

### 2. Design stance: derive the dirty set, never guess it

The vice an incremental system accumulates is *ad-hoc invalidation* — a pile of special
cases ("if X changed also redo Y") that drifts from the real dependency structure until
correctness is folklore. This design refuses that shape. Instead:

1. **Ownership is explicit.** Every edge records the file whose facts produced it. Nothing
   is inferred from edge shape — inference is exactly where today's shapes already lie
   (a narrowed-dynamic `Wildcard` points *from the target* but is produced by the importer;
   a barrel's re-export `Root` promotion targets a symbol in *another* file).
2. **Dependencies are named, and detected by signature.** A file's derived contribution is a
   function of (its own facts) × (a small, enumerable set of *resolution inputs*). Each
   input gets a signature; the patch applies only when every input a change could disturb is
   provably unchanged. There is no "probably fine".
3. **Full rebuild is the only fallback, and it is always correct.** RFC 0004 §5's
   fallback-honesty rule, applied maximally: any condition outside the proven-safe region
   falls back to the plain rebuild. The patch is an optimization with an equivalence proof
   obligation, never a second semantics.

#### 2.1 The dependency inventory

What file `f`'s contribution actually depends on, from the assembly code (graph.rs phases
2–3b), exhaustively:

| Input | Used by | Changes when |
|---|---|---|
| `f`'s own facts | everything `f` owns | `f`'s content changes |
| project file **set** (paths) | import path-probing, dynamic-dir narrowing, package ownership | file added/removed/renamed |
| manifests (all facts) | packages, declared deps, workspace index, library roots, script-invoked deps | any manifest content changes |
| import targets' **export surface** | binding resolution, qualified refs, barrel aliasing, `unit_name` qualifiers | a target's declarations/re-exports/unit change |
| unit siblings' surfaces | unit-scoped name resolution (Go) | a sibling's declarations/unit change |
| project-wide member table + ladders | duck-typed member fallback (RFC 0012 §3) | any file's *member* declarations or visibility change |
| adapters (ids, versions, ladders, policies) | everything | binary upgrade — already in the graph key |

The last four rows share one detector: the **surface signature** (§4). When no changed
file's surface signature moved, *every* row below "own facts" is untouched by construction,
and the dirty set is exactly the changed files. That is not a heuristic — it is the precise
statement "no resolution input changed", checked, not assumed.

### 3. Prerequisite hardening — fix the foundation first

Three latent order-dependences in today's assembly would either break the patch's
equivalence guarantee or force the patch to *reproduce artifacts bug-for-bug*. Reproducing
an artifact is how a vice gets load-bearing. Each is fixed **before** the patch lands, as
its own commit with the full suite green:

**(a) Canonical edge order becomes a graph invariant.** Today the edge vector's order is an
accident of construction (phase order, file order, and — since M4.5 — parallel-merge
order). Order is almost invisible, but not quite: `cyclic` keeps the first edge on a
confidence tie when picking evidence spans, so two differently-ordered but semantically
identical graphs can render different evidence ranges — which would break the
`patched ≡ full` gate and quietly weakens today's `--no-cache ≡ cached` gate too. Fix:
every assembly (full or patched) ends by sorting edges with one total, documented
comparator. Cost: one `sort_unstable` over the edge vector (~10³–10⁵ elements, tens of ms
at worst). From then on edge order is *data*, not history. Diagnostics get the same
treatment (sort by path/span/message at the end of assembly).

**(b) Barrel re-export aliasing becomes order-independent.** Phase 3a-bis resolves
`export { x } from './b'` aliases in one pass in FileId order — a barrel chaining through
another barrel resolves only if discovery order cooperates (documented as a one-hop
limitation in js-ts.md §5). A patch that reuses *persisted* alias tables would see the
*final* state and diverge from a full rebuild's order artifact. Rather than teach the patch
to reproduce the artifact, remove it: resolve re-export bindings to a fixpoint over the
re-export graph (deterministic worklist, cycle-guarded — a cycle of re-exports resolves to
nothing, as today's one-hop de facto does). Multi-hop barrels start resolving correctly —
a user-visible improvement js-ts.md already promises as "a future increment".

**(c) Snapshot diagnostics stop entangling discovery with extraction.** Today the snapshot
stores the original build's *merged* diagnostics (discovery + extraction + manifest) and a
warm hit replays all of them while discarding the fresh walk's own diagnostics. That is
only coherent because a key match implies an identical tree. A patch (tree *not*
identical) can't split the merged vector. Fix the model: the snapshot stores only
**extraction + manifest** diagnostics (the ones whose producers are skipped on warm paths);
discovery diagnostics are always the fresh walk's. Hit, patch, and full paths all compose
`fresh discovery ∪ stored-or-recomputed extraction` — one rule, no special cases.

### 4. Persisted additions (graph schema 10)

| Addition | Why |
|---|---|
| `Edge.owner: FileId` | explicit ownership (§2 point 1); removal set = `owner ∈ C`, exact |
| per-file `surface_sig: [u8; 32]` | the §2.1 detector: blake3 over the file's span-normalized surface — adapter id + facts schema version + class(role, origin) + `unit` + `unit_name` + declarations `(name, kind, exported, visibility, member_of)` in order + imports `(specifier, bindings(local, imported), reexported, opaque_namespace_use, local_alias)` in order + in-source roots `(kind, target, confidence)` in order + dynamics `(narrowed_to)` in order. Spans excluded everywhere — bodies and positions move freely under the guard. Unclaimed files: no signature (nothing derived to guard) |
| per-file `unit_name: Option<SmolStr>` | qualifier defaults (RFC 0012 §9) currently live only in facts; targets' facts must not be re-fetched |
| per-file re-export alias table `Vec<(SmolStr, SymbolId)>` | the one table not derivable from `graph.symbols`; post-(b) it is order-independent state, safe to reuse |
| per-file `module_bindings: Vec<ModuleBinding>` | the names a file's imports bind to in-repo FILES — the table RFC 0012 §9-bis's hop pass reads across *all* files, including unchanged ones the patch never re-extracts. Not a new guard: the surface signature already covers the imports it derives from |
| `graphs/latest` pointer (last written key) | a miss must find the previous snapshot; content-keyed filenames can't |

Everything else the patch needs is already derivable from the snapshot: bare/qualified/unit
symbol tables and the member table from `graph.symbols`; `library_root_files` from kept
manifest-owned `Root` edges; `role_root_files` from the file's own class; ladders, cycle
policies, packages, declared dependencies — stored since M3/M4.

### 5. The patch algorithm

On a graph-key miss with a loadable `latest` snapshot:

1. **Delta.** Compare old vs new discovered sets. Any path added/removed → full rebuild.
   `C` = files whose content hash differs.
2. **Guards** (each falls back to full rebuild):
   - any `c ∈ C` matches an adapter's manifest globs;
   - `|C|` > **5%** of files — measured, not assumed: under the 30% ceiling RFC 0004 §5
     sketched, the benchmark suite's 1k/100-file scenario regressed +22% (the patch's fixed
     costs — snapshot load, table rebuild — beat the saved resolution); the crossover sits
     near 5%, and the pre-commit case the patch exists for is far below it;
   - any *claimed* `c ∈ C`: extract fresh facts (facts cache handles it), compute the new
     surface signature, compare to the stored one — any mismatch → full rebuild.
   Unclaimed changed files are always patchable (their only contribution is a content hash).
3. **Splice**, in place on the loaded graph:
   - `FileId`s are stable (path-sorted set, unchanged). `SymbolId`s are stable (equal
     surface ⇒ each changed file's symbol run has identical names/kinds/order; only
     `span`/`signature_span` fields update in place). `DependencyId`s are stable (equal
     imports ⇒ the dependency set and its first-appearance order are unchanged).
   - remove everything owned by `C`: edges (`owner ∈ C`), `function_metrics` of `C`'s
     symbols, suppressions, stored extraction diagnostics for `C`'s paths;
   - update `C`'s `FileNode.content_hash` (class/unit unchanged by guard);
   - re-run, for `C` only: phase 3a's per-file emissions (Declares spans, in-source roots,
     metrics, export promotions), 3a-bis (against persisted alias tables — sound after
     §3(b)), and 3b's per-file resolution — against lazily built tables: bare/unit/qualified
     tables only for the files `C` actually resolves into (its import targets and units),
     the member table project-wide (one linear pass over `graph.symbols`);
   - re-sort edges and diagnostics canonically (§3(a)) — the splice's insert position
     becomes irrelevant.
4. **Persist** the patched graph under the new key (existing deferred writer), update
   `latest`.

The engine's phase timings gain `patch` vs `rebuild` visibility (`--verbose` shows which
path ran — silent mode switches are their own vice).

### 6. Equivalence obligation & enforcement

The patch has exactly one correctness statement: **a patched graph is byte-identical to the
full rebuild of the same tree** — not "same findings", the same *graph*, serialized. §3(a)
makes this checkable (order is canonical). Enforcement:

- a dedicated equivalence suite: for each conformance fixture and the generated bench
  fixtures, apply a body edit / span shift / comment edit, run the patch path and a scratch
  full rebuild, assert identical serialized snapshots and identical output envelopes;
  repeat for guard-tripping edits (surface change, manifest change, file add/remove) and
  assert the fallback engaged *and* produced the same envelope;
- the existing gates keep running: `--no-cache ≡ cached` (now strictly stronger: no-cache ≡
  cached-hit ≡ cached-patched), the `--threads 1 ≡ N` matrix, the conformance corpus;
- the bench suite gains a `warm-1-file` expectation at 50k of the same order as `warm-noop`.

### 7. Explicitly out, with reasons of record

- **Fine-grained dirty propagation** (re-resolving *importers* of a surface-changed file
  instead of full rebuild): the architecture supports it — same signatures, plus reverse
  indices — but v1 refuses it. Surface changes are the minority of pre-commit edits, and
  every increment of cleverness here is an increment of proof obligation. Revisit with
  data, per the house rule.
- **Incremental reachability recoloring** (RFC 0004 §5): deprioritized *with data* — full
  recoloring costs 13 ms at 50k after the M4.5 CSR rewrite. The trigger is written in RFC
  0004's status note.
- **Patching across file-set changes** (add/remove/rename): FileId renumbering invalidates
  the id-stability proof wholesale; the honest v1 answer is the fallback.
- **Plugin contributions** (RFC 0004 §4 step 4c): out of scope here when written — the plugin
  system contributed nothing to the graph yet. Since landed by RFC 0017 §3, and not via
  `Edge.owner` as this bullet guessed: plugin edges carry `Provenance::Plugin`, so the patch
  strips them all and re-runs the plugin round against the patched graph (same function as
  the full build), guarded by a snapshot-stored plugin-set identity digest. The §6
  equivalence obligation extends unchanged: patched ≡ full rebuild, plugin round included
  (`the_patch_re_derives_plugin_contributions_instead_of_bypassing`).

### 8. Rollout

Three commits, suite-green after each: (1) §3(a)+(b) hardening — canonical order and
fixpoint aliasing (no snapshot semantics touched); (2) schema 10 — §3(c)'s diagnostics
split plus the persisted additions: owner, signatures, unit_name, alias tables, latest
pointer (additive; the patch not yet active); (3) the patch path + equivalence suite +
measured numbers in the commit message. `--no-cache` remains
the operational escape hatch — no new knobs.

## RFC 0005: Analyses

**Status:** Accepted · **Depends on:** RFC 0001, 0002, 0004

All analyses are pure functions over the Project Graph (+ optional enrichments such as coverage).
Each finding carries: stable id, category, severity, confidence, location(s), evidence, and a
remediation hint (schema in [CONTRACTS.md](CONTRACTS.md)).

**Taxonomy rule — a category is a verdict; the subject is a facet; the reporting level is a
rollup.** Three orthogonal things, kept orthogonal:

1. **Category = verdict, nothing else**: `unused`, `test-only`, `untested`, `undeclared`,
   `unresolved`, `version-skew`, `duplicate`, `internal-only`, `private-type-leak`, `cyclic`,
   `deep-import`, `crap`, `stale` (the normative registry lives in
   [CONTRACTS.md](CONTRACTS.md) §6). A verdict means the same thing
   whatever it lands on — there is no `unused-file` vs `unused-code`: both are `unused`.
2. **Subject = the `subject_kind` facet**: the kind of graph node the verdict landed on — `file`,
   `directory`, `package`, `dependency`, `import`, `suppression`, or any symbol kind (`function`, `type-alias`,
   `enum-member`, `css-rule`…). Configuration and suppressions target a bare category or
   `category:subject` (e.g. `unused:enum-member`, `test-only:dependency`); knip-style
   `unused-type` ≡ `unused:type-alias`.
3. **Reporting level = widest uniform node**: when a verdict holds for every symbol in a file
   *and* for the file node itself, kndo emits **one** finding on the file (subject `file`), not
   N symbol findings; when it holds for every file in a directory, one finding on the directory;
   when it holds for a whole workspace package, one finding on the package (RFC 0011 §6).
   Rollup is presentation of the same facts, not a different verdict — "test-only file" is the
   `test-only` verdict reported at file granularity.

4. **Group = the verdict's nature, fixed per category**: every verdict belongs to exactly one
   presentation group, declared here — never configurable, never guessed by renderers:

   | Group | Verdicts | Meaning for the reader |
   |-------|----------|------------------------|
   | `defect` | `unresolved`, `undeclared`, `version-skew`, `private-type-leak` | something is broken or lying — fix it |
   | `waste` | `unused`, `test-only`, `duplicate`, `internal-only` | something can be removed, consolidated, or narrowed |
   | `risk` | `crap`, `cyclic`, `untested`, `deep-import` | something is dangerous to change — refactor or test it |
   | `hygiene` | `stale` | kndo's own bookkeeping is outdated |

   Groups drive ordering and sectioning in every renderer (defects before waste before risk
   before hygiene — see RFC 0006 §3) and give consumers a stable coarse filter. A future verdict
   must declare its group on arrival (e.g. candidate `layer-violation` → `risk`,
   `redundant-export-binding` → `waste`); new groups are additive and rare.

Intentional absences are documented decisions, not oversights: there is no `tooling-only`
verdict (tooling reachability is healthy).

### 1. Reachability foundation

Most detections derive from one computation. Roots are partitioned by `RootKind`:

- **Production roots** — language-defined entry points (`main`, published/public API of a library,
  package `exports`/`bin`) + plugin-contributed roots (framework handlers, DI-registered beans…).
- **Test roots** — test functions/files (language role detection + test-framework plugins).
- **Tooling roots** — build/config scripts (webpack.config, build.gradle, migrations…): they keep
  their imports alive but are not production code themselves.

A declared root's *kind* is capped by its file's role: a `main`/manifest-bin entry point in a
Tooling-role file (an `xtask/` binary) is a Tooling root, in a Test-role file a Test root —
the adapter states the language fact ("this is an entry point"), the role decides who
consumes it. Plugin-contributed roots are exempt (targeted consumer knowledge outranks a
directory convention), and so are library-surface promotions ("the production API re-exports
this") — though no promotion chain ever *starts* from a capped file.

**Plugin-contributed edges are liveness evidence, never architecture evidence (RFC 0017
§5.4).** A plugin's `References`/`ReferencesFile` contributions feed reachability — where a
false positive can only *suppress* findings, the safe direction — and are ignored by `cyclic`
and by every analysis that would *create* a finding from an edge's existence. This is a
contract, not an implementation accident: it is what lets the platform accept
lower-precision, convention-derived edges from third-party plugins without ever risking the
zero-false-positive bar.

Two reachability passes (production-only roots; then all roots) assign every symbol/file a color:

| Color | Meaning |
|-------|---------|
| `production` | reachable from a production root |
| `test-only` | reachable only from test roots |
| `tooling-only` | reachable only from tooling roots |
| `unreachable` | reachable from nothing |

**Color and confidence resolution.** Two things are easy to leave ambiguous and both resolve
from one algorithm: what happens when a node is reachable from roots of *different kinds*, and
what confidence a node gets when reachability depends on *non-certain* edges.

For each `RootKind` κ and threshold τ ∈ {certain, probable, possible} — strength order
certain > probable > possible, so "edges at least as strong as τ" shrinks as τ strengthens —
define `R(κ, τ)`: the nodes reachable from κ-roots using only edges at least as strong as τ.
`R(κ, possible)` therefore uses every edge regardless of confidence and is the largest set;
`R(κ, certain)` uses only certain edges and is the smallest. Root edges carry their own
confidence too (RFC 0001 §3) and seed the traversal at that strength — a plugin-contributed
root nobody is fully sure about only participates from `probable` onward, like any other edge.

Wildcard edges are not a separate mechanism — they are folded into this same computation: a
`Wildcard { from }` edge expands into `possible`-confidence edges from that file to its
**plausible target set** (same-file symbols, symbols a plugin marked externally-consumed via
`annotate_symbols`, and whatever an adapter's `DynamicUse` reason narrows the scope to — a
partial string prefix narrows it, a bare `eval` does not). One mechanism, not two.

**Attribution and the module-load rule (RFC 0012 §4).** Reference edges are attributed to the
declared symbol they execute *inside* when the adapter supplies it (`RawReference::within` —
contracts §2), and to the file otherwise (module-level code, and adapters that don't emit the
field). Two rules govern the traversal:

- **Execution rule:** a symbol-attributed reference fires only when its symbol is reached — a
  dead function's calls keep nothing alive, so transitive death is visible.
- **Module-load rule:** reaching a symbol also reaches its owning file, at the same τ — using
  a symbol loads its module, so the file's load-time (`within: None`) references and its
  `ImportsFile` edges fire. This is also what makes symbol-only roots work (Go's `func
  main`/`init`/exported-declaration promotion, docs/adapters/go.md §0, §2 — no manifest-level
  entry file to root alongside them): `R(κ, τ)`'s traversal enqueues a reached symbol's owning
  file alongside the symbol.
- **Containment rule:** reaching a member also reaches its owning declaration, at the same τ —
  a method cannot execute without the type that declares it, so if the member is alive the
  owner is too, and a class whose only live member is invoked by a container or dispatched
  through a vtable is not deletable. The owner resolves by the `member_of` convention in the
  member's own file (same lookup the machinery-dispatch rule uses, read upward); every
  same-name candidate links. Without it a run could assert both halves of a contradiction:
  spring-petclinic's `@Configuration` class was reported `unreachable` while the same run
  called its own `@Bean` methods production-reachable, and three Java conformance fixtures had
  codified the same shape by expecting `unused` on the class holding `public static void main`.
- **Invoked-program rule:** an `InvokesFile` edge — a file executing another file **as a
  program**, the process boundary no import crosses (a test running its own workspace binary
  via `env!("CARGO_BIN_EXE_…")`, resolved through the manifest's named executable targets,
  `ManifestFacts::executables` × `FileFacts::invoked_executables`) — traverses to the target
  file AND to every Production `Root` target declared inside it, each at the weaker of the
  invocation's and the root's own confidence. Importing a module runs only its load-time
  code; *executing* a program runs its entry point, so the invoked binary's whole call tree
  inherits the invoker's colors. Non-Production roots inside the invoked file stay out
  (running the binary runs neither its inline tests nor its tooling entries). Like
  `ReferencesFile`, this is liveness evidence only — no finding-creating analysis reads it,
  so a false edge can only ever suppress findings.
- **Machinery-dispatch rule:** a member the adapter marked `implicitly_invoked`
  (contracts §2) is exercised by the language's own machinery whenever its OWNER is used —
  an operator (`==` → `eq`), a formatting hook (`{}` → `fmt`), a destructor (scope end →
  `drop`), a loop protocol (`for` → `next`). The call site never writes the method's name,
  which is exactly why no reference edge can exist for it. Reachability derives an implicit
  owner → member edge at `Probable` (using the type IS plausibly using the hook — degrade
  toward silence), the owner resolved by `member_of` in the member's own file, twins
  included; an unreached owner propagates nothing. WHICH traits/protocols qualify is each
  adapter's curated knowledge (Rust: the stdlib fmt hooks, operators, `Drop`, `Hash`,
  `Iterator`, `Future`, `FromStr`, `Error` — docs/adapters/rust.md §2); name-called trait
  methods (`.clone()`, `.into()`) stay out — the duck fallback already reaches those. This
  composes with (not replaces) the dispatch rule's `Probable` Production roots on
  trait-impl members: the root keeps a hook alive with zero owner usage, the implicit edge
  lets it *inherit the owner's colors* — which is what `untested` (§9) needs to stop
  calling a formatting hook a test-blind spot when its type is test-covered. The rule reads
  TWO sources of the same fact: the adapter's `Declaration::implicitly_invoked` (the
  language's own machinery) and a plugin's `mark_implicitly_invoked` annotation (a
  *framework's* machinery — serde calling `serialize`; `kndo:serde`, docs/src/plugins/serde.md
  — third-party dispatch the language adapter must never learn about). A plugin normally
  reaches that annotation through `AnnotationSink::mark_machinery_impls`, which matches its
  curated trait table against `SymbolNode::implements` — the trait whose implementation
  declares the member, an adapter-supplied fact the core carries and never interprets
  (core-traits.md). Same rule, same two sources; the plugin brings only the table.
- **Implement-dispatch rule:** calling through a trait IS plausibly executing every
  implementation — the vtable, as declared. For each `RefKind::Implement` edge
  (`impl Trait for T` emits one from the implementing type's symbol to the trait's), every
  member of the TRAIT fans out to the implementing type's same-named member in the impl's
  own file (the edge's owner — an impl block need not share its type's file), at
  `Probable`. Fully derived from facts already in the graph — no trait lists in the core; a
  trait member nothing reaches propagates nothing, an Implement edge whose source fell back
  to file attribution contributes nothing, and an out-of-repo trait (serde) never resolves
  an Implement edge at all (that gap is the `kndo:serde` plugin's, above). This is what
  turns one test exercising a `&dyn Flag` call site into test-reachability for every flag
  impl: the site resolves to the trait's member (the dyn receiver's *declared* type), and
  the fan-out inherits from there.

File attribution (the pre-RFC-0012 behavior, still what a `within`-less adapter gets) is the
deliberate over-approximation: everything a live file references stays alive. Every
degradation — unresolvable `within`, absent field — falls back to it, never the other way.

Every node's `(color, confidence)` comes from the first matching rule, in this fixed order:

1. `production` — if the node ∈ `R(production, possible)`; confidence = the strongest τ for
   which it's still in `R(production, τ)`.
2. `test-only` — same test, against `R(test, τ)`.
3. `tooling-only` — same test, against `R(tooling, τ)`.
4. `unreachable` — the node is in `R(κ, possible)` for **no** κ: zero evidence at *any*
   confidence tier, from *any* root kind, wildcard-expansion included.

Color precedence deliberately outranks confidence: a node *possibly* production must never be
offered up for deletion just because it is *certainly also* test-only — "maybe still used for
real" beats "definitely only used by tests" for what a reader should do next.

**Consequence: dead is always `certain`.** Rule 4 fires only when a node has no evidence at
*any* tier, so an `unused` finding's confidence is always `certain` — there is no "probably
dead". A node with any evidence, however weak, is colored **alive** (rules 1–3) at that weak
confidence instead: a symbol reachable only through a `possible` edge is production-*possible*,
never test-only-*probable* or dead-*probable*. `test-only`/`internal-only`/etc. findings, unlike
`unused`, do inherit sub-certain confidence — they fire on nodes that *are* reachable, just not
from the root kind that would make them safe.

*Example:* symbol `S` is called from `src/prod/x.ts` (a production root) through a duck-typed
dispatch (`probable`), and imported directly from `tests/y.test.ts` (`certain`). `S` is in
`R(production, probable)` but not `R(production, certain)`, and rule 1 fires before rule 2 is
even checked ⇒ color `production`, confidence `probable` — no `unused` finding, and
`kndo describe S` reports "production (probable), kept alive by `src/prod/x.ts:12`
(duck-typed call)".

**Library mode:** for library packages the public API is a production root by definition —
kndo will not call exported API "unused" just because the repo doesn't call it. Within an
unpublished application package, however, `export` is *not* a root; an exported-but-never-imported
symbol is still dead. Adapters/manifests decide which mode applies per package.

### 2. `unused` — unreachable code, files & dependencies

`unreachable` symbols. Severity: warning. Evidence: the symbol, why nothing reaches it, nearest
former consumer if known from the findings snapshot. Confidence downgrades if any wildcard edge
could plausibly target it (name exposed to reflection/serialization, plugin annotations, FFI).

**Member granularity.** The analysis descends into type members: methods, fields, and enum
members are symbols in their own right (`subject_kind` facets `method`, `field`, `enum-member`),
so "class member nothing calls" and "enum variant nothing references" are ordinary
`unused` findings. Dynamic dispatch is handled through the graph, not guessed around:
adapters emit implements/overrides references, so an interface/trait method implementation is
alive whenever the interface method is reached; where dispatch is not statically resolvable, the
member's liveness evidence is at best `probable` and findings demote accordingly.

### 3. `test-only` — non-productive code

Nodes colored `test-only`, excluding test-role files themselves and declared test utilities
(`testkit`/`fixtures` conventions, configurable). This is the "you built it, tests enshrined it,
production never came" detector — the finding explicitly lists the test roots that keep the node
alive, so deleting code + its tests together becomes mechanical.
Default severity: **info**. M6, the milestone this was gated on, has landed (ROADMAP.md) and the
default has not moved: `analysis/test_only.rs` still emits `Severity::Info` for both the file and
symbol findings. Raising it to warning remains available but has no decision behind it yet;
changing it is a defaults-contract change (ADR 0006) and would need its own dogfooding case made,
not just the passage of a milestone.

### 4. File & directory subjects (rollup, not new categories)

`unused` and `test-only` apply to file nodes like any other node: a file with no incoming edge
and no root is `unused` (subject `file`) — this subsumes asset/config orphans via cross-language
edges (CSS, JSON); a production-role file whose every incoming edge comes from test roots is
`test-only` (subject `file`). Per the rollup rule (§ taxonomy), the file finding *replaces* the
per-symbol findings it summarizes, and a directory whose every file carries the same verdict
rolls up once more (subject `directory`) — "you can delete this whole folder" is one finding,
not fifty. Generated and vendored origins are exempt by default.

**Served is not used.** A file whose ONLY inbound evidence is `EdgeKind::ReferencesFile` — the
file-liveness edge, "if `from` is alive, that file is in use", what a template's `<link href>`
or a framework config naming an asset by path contributes — is *served*, not *used*, and its
symbols are not judged one by one. The evidence says the bytes ship and names none of them, so
reporting each unread symbol is an accusation it cannot support: connecting spring-petclinic's
layout to its stylesheet turned one file-level verdict into 48 dead `--bs-*` custom properties
from a compiled Bootstrap bundle. The FILE is judged normally (the link is exactly the evidence
that keeps it alive), and any other inbound edge at all — an `ImportsFile`, an `InvokesFile`, a
`Root`, a `References` naming one of its symbols — is symbol-level evidence and restores
ordinary jurisdiction. This is the same asymmetry `ReferencesFile` already carries by contract:
liveness evidence, never architecture evidence.

### 5. Dependency & import hygiene

For each `ManifestDependency` with scope `prod`, classify by its importers:

| Importers | Finding |
|-----------|---------|
| none | `unused` (subject `dependency`) — declared, never imported |
| only test-role / test-only-reachable files | `test-only` (subject `dependency`) — belongs in dev scope, not shipped weight |
| at least one production- or tooling-reachable file | used (no finding) |

The neutral scope taxonomy is `prod | dev | build | peer | optional` (adapters map ecosystem
scopes onto it — npm `peerDependencies`, Cargo `build-dependencies`, Gradle configurations):

- **dev / build** — checked against all files; unused only if *nothing* imports them.
- **peer** — a contract with the consumer, not a usage claim: exempt from `unused`
  (an un-imported peer is at most an info-level note), and never `test-only`.
- **optional** — runtime-conditional by design: findings demote to `possible` confidence, below
  the default report floor.

Adapter-provided package mappings handle subpath imports, type-only packages (`@types/*` bound to
their runtime package), and side-effect-only imports (`import "polyfill"` counts as usage).
Internal workspace dependencies get the same verdicts with boundary-aware remediation
(RFC 0011 §4).

Two further import-side findings:

- `undeclared` (subject `dependency`) — an import resolves to a package absent from the manifest
  (phantom deps via hoisting/transitivity). Severity: warning; error in `--strict`.
- `version-skew` (subject `dependency`) — the same external dependency declared with diverging
  version requirements across workspace packages (RFC 0011): three packages pinning three
  `lodash` versions is an inconsistency someone will debug eventually. Manifest-only detection,
  zero-config; evidence lists every declaring manifest with its requirement. Severity: warning.
- `deep-import` (subject `package`) — an import bypassing a provider's *declared* entry-point
  surface. Applies to workspace siblings **and external dependencies alike** — a plain
  single-package app importing `some-lib/dist/internal/x` gets the finding when `some-lib`
  declares an `exports` map. Contract-gated, pair-level rollup, computed remediation — full
  design in RFC 0011 §4.
- `unresolved` (subject `import`) — a relative/internal import specifier that resolves to no file
  (`Resolution::Unresolved` after all adapters decline): almost always a broken path or a missed
  rename. Failed *package* resolution surfaces as `undeclared` instead, never twice.
  Severity: error (it is a defect, not waste) — but confidence-gated: the finding inherits the
  import's own confidence, so a dynamic specifier the adapter could only partly read demotes and
  drops below the default report floor. **Which specifiers are relative is the adapter's call**
  (`ImportKind`), never a shape the core guesses at; assembly records the failed resolutions and
  the analysis decides what follows. Generated and vendored files are exempt, like everywhere.

### 6. `duplicate` — structural clones

Token-based fingerprinting over adapter-normalized token streams (identifiers/literals
canonicalized ⇒ catches Type-1 and Type-2 clones; Type-3/semantic clones are out of scope for 1.0):

- Granularity: callable **shapes** ≥ `min-tokens` (default 50). A declaration contributes its
  own body plus one shape per callable nested inside it that clears the same floor
  (`MetricsSyntax::nested_callable_kinds` names the node kinds per language); a promoted
  shape's tokens leave the enclosing stream, which keeps one `FN` in their place. This is what
  makes N call sites passing the same callback report on the callback, where the duplication
  is, instead of on N otherwise-different callers. A nested callable *below* the floor stays
  part of its owner's body — promoting it would leave both halves under the floor and delete
  real findings (measured on the field corpus: 83 clone participants).
- Winnowing fingerprints into a global index; matches only within the same language.
- **A body that only constructs a value is not clone-eligible.** There the normalization
  inverts: a construction expression has no control flow, its structure IS the field list the
  type declaration dictates, and the only authored content is the field values — exactly what
  `ID`/`LIT` erases. Two constructions of one type therefore fingerprint alike by definition of
  the type, not by evidence of copying, and the false family grows with how *central* the type
  is. `MAX_POSTING` already concedes the same belief using popularity as the proxy; this names
  the cause. All-or-nothing (a function that constructs *and* does work has authored
  structure), not configurable (`min-tokens` is a floor on size, and this is not about size),
  and needing no carve-out for a construction carrying a callback — that callback is its own
  shape and is not exempt. `MetricsSyntax::construction_kinds` is empty for languages where
  construction is an ordinary call (Kotlin, Swift), which keeps today's behaviour there rather
  than having the adapter guess.
- Finding groups all instances, largest group first; evidence shows the shared shape.
- **Structural clones target production code**: test-role files and sub-file test regions
  (`FileFacts::test_spans`) are exempt, unconditionally — the same two-level exemption `crap`
  and `untested` apply. Parallel arrange-act-assert bodies across a fixture matrix are the
  *point* of table-shaped tests, not waste. The exact-file-duplicate half keeps its
  no-carve-out rule.

Severity: info by default (duplication is sometimes deliberate); the *metric* (duplication %)
always feeds health regardless of severity.

**Exact file duplicates.** Byte-identical files — same blake3 content hash, computed anyway for
the cache — are the same `duplicate` verdict with subject `file`: copy-pasted configs, images,
and any other asset that token-based clone detection cannot see (binaries included). One finding
groups all copies. Costing nothing beyond hashing, this lands in M1, ahead of structural clones.

### 7. Visibility mismatch: `internal-only` & `private-type-leak`

Both directions of one comparison — a symbol's **declared** visibility against what its usage
**requires** — using the language's visibility ladder (adapter-declared: private → file →
package/crate → public). *The ladder's concrete contract form (rungs as `(scope, label)` data
on the adapter descriptor), the generalized tightest-sufficient algorithm, and
`private-type-leak`'s implementation design (`RefKind` tagging + signature spans) are
specified in RFC 0012 §§5–6.*

**`internal-only`** (declared > required; group `waste`): the analysis computes the **tightest
sufficient visibility** — the lowest ladder level that still covers the origin of every incoming
reference. Declared above it ⇒ finding; the remediation names the concrete change in the
language's own terms (`private`, `pub(crate)`, unexported lowercase name), supplied by the
adapter.

**`private-type-leak`** (declared < required; group `defect`): a public/exported symbol whose
signature references a type of *lower* visibility — the API promises a type its consumers cannot
name. Derivable directly from `TypeUse` edges crossing visibility levels downward; no new
vocabulary. Severity: warning in library-mode packages (a lying public API), info in app
packages. Remediation offers both directions: export the type, or narrow the symbol.

Covers your whole ladder of cases uniformly: exported symbol referenced only within its own file
(`internal-only:function`), public member used only inside its own type (`internal-only:method` —
"should be private"), Rust `pub` used only in-crate ("should be `pub(crate)`").

Exemptions: library-mode public API (roots are externally consumed by definition), symbols
plausibly targeted by wildcard edges (demote to `possible`), and symbols plugins mark as
externally consumed via `annotate_symbols` (FFI, serialization, DI). Severity: info.

### 8. `cyclic` — dependency cycles

Strongly connected components (Tarjan) of size ≥ 2 in the file-import graph, and in the
package/module graph where manifests define units. **One finding per cycle**, not per
participant (rollup spirit): anchored at the cycle's most-referenced node, with a shortest
cycle path in `related` as the evidence chain. Incrementally, SCCs are recomputed only within
the dirty region's weakly connected component.

Cycle tolerance is a language fact, so adapters declare it per graph level and defaults stay
honest: severity `warning` where the ecosystem treats cycles as hazards (JS/TS file cycles —
init-order bugs); idiomatic levels (Rust modules within a crate) and impossible levels (Go
package cycles — the compiler already forbids them) alike emit nothing — an idiomatic cycle
is true information about legal structure, and information is never dressed up as a defect.
A mixed-language cycle reports iff any participant's language calls it a hazard.

### 9. `untested` — static test-blind spots

The exact inverse of `test-only`, computed from the same coloring passes at zero extra cost:
symbols **production-reachable but reachable from no test root whatsoever** — not "low
coverage" (dynamic, needs a report) but "no test even *imports* this, transitively". CRAP tells
you complex code is poorly covered *if* you feed it coverage; `untested` finds the blind spots
statically, zero-config, day one.

- Active only when the project has test roots at all — a repo without tests **abstains** (§12):
  one diagnostic, not a thousand findings, and the category is reported as unjudged rather than
  as clean.
- Severity: info. Subject granularity and rollup as usual (an entire untested file or package
  rolls up). Confidence demotes through wildcard edges like every reachability verdict.
- **Only units of testing.** A value is not one: `EnumMember`, `Const`, `Static`, `Variable`,
  `Field`, `CssRule`, `CssVariable` join `TypeAlias`, which was already excluded here with this
  exact reasoning ("no runtime footprint — can never be 'covered'"). Callables and types stay,
  and it is a DENYLIST so an adapter-defined kind (Kotlin's `object` arrives as `Other`) is not
  silenced by a guess. Measured before adopting: 422 of 2621 findings across the field corpus,
  56% of one project's, were values. This is only sound because a computed property is no longer
  a `Field` — while a stored constant and a getter-with-a-body shared one kind, excluding
  `Field` would have taken real logic with it.
- A file that declares symbols and not one of them is a unit of testing is out of scope, which
  is what answers "a `.scss`/`.json`/`.md` cannot be tested" without any per-language opt-out —
  and keeps a `.scss` carrying a `@function` in scope, which such a flag would have silenced. A
  file the adapter extracted NOTHING from stays in scope deliberately: dropping that guard
  silenced 14 real source files across the corpus whose adapters simply saw no declarations.
- Evidence: the production roots that reach the symbol (proof it matters) and the nearest tested
  neighbor (where a test could start).

### 10. `crap` — Change Risk Anti-Patterns

Per function/method, with `comp` = cyclomatic complexity (adapter-extracted) and `cov` = fraction
of the function's statements covered:

```
CRAP(m) = comp(m)² × (1 − cov(m))³ + comp(m)
```

- Coverage comes from ingested reports (plugins, ADR 0005). No report at all ⇒ the analysis
  **abstains with one diagnostic** (§12) — the coverage factor would be a guess for every
  function at once, not a measurement, and a category-wide guess is noise, not risk (the same
  posture `untested` takes for a project with no test roots); the health score's crap axis
  contributes zero penalty with the absence reported explicitly. Coverage is *ingested, never
  measured*, so the absence of a report is routine (fresh clone, a CI job separate from the test
  job, a project with no coverage tooling) — which is exactly why it must be reported as
  unknown, not silently as clean. A report that doesn't instrument a
  particular function ⇒ **cov = 0**, flagged "coverage: none" — pessimistic per function, and
  the message says why.
- Threshold: findings for `CRAP > 30` (standard), configurable. Test code is exempt.
- Scored per callable **shape** (§6): a substantial closure carries its own complexity and its
  own coverage rather than its enclosing function's, and the finding points at the closure.
  Reporting nested shapes is not optional once they exist — a 40-branch closure inside a
  two-branch function scores 40 on the closure and 2 on the function, so skipping them would
  delete the risk from the report entirely. Identity: the closure's ordinal within its
  declaration, never its line, so a baseline survives edits above it.
- Output ranks the CRAP hotspot list — the refactor-next queue.

### 11. `health` — project health score

A 0–100 composite, deterministic and documented so trends are meaningful:

```
health = 100 − Σ category_penalty
category_penalty = weight × saturating_ratio(category)
```

| Category | Ratio basis | Default weight |
|----------|-------------|----------------|
| unused code | dead symbols / total symbols | 25 |
| unused deps | misdeclared (unused, test-only, undeclared) / declared | 15 |
| unused files | orphan files / total files | 10 |
| test-only code | test-only symbols / total symbols | 10 |
| duplication | duplicated tokens / total tokens | 20 |
| CRAP | CRAPload above threshold, normalized | 20 |
| cycles | files participating in cycles / total files | 5 |
| excess visibility | internal-only symbols / exported symbols | 5 |
| test blind spots | untested production symbols / production symbols | 5 |

`saturating_ratio` maps each raw ratio through a per-category curve (documented constants) so a
single bad file can't zero the score and improvements near zero still show. Grades: A ≥ 90,
B ≥ 80, C ≥ 65, D ≥ 50, F below. Output always shows the per-category breakdown and, in diff
modes, the delta caused by the change. Weights are configurable; defaults are the contract.

**Landed (M4) — the documented constants.** The curve is `saturating_ratio(r) = min(r /
saturation, 1)`: linear near zero, full weight at the saturation point. Defaults (the
contract until the config file lands):

| Category | Saturation (ratio = full weight) |
|----------|----------------------------------|
| unused code | 0.25 |
| unused deps | 0.5 |
| unused files | 0.25 |
| test-only code | 0.25 |
| duplication | 0.3 |
| CRAP | 0.5 |
| cycles | 0.25 |
| excess visibility | 0.5 |
| test blind spots | 0.5 |

Ratio definitions where the table above leaves room: *duplication* counts the tokens of every
clone-group member beyond its canonical (lexicographically first) instance — the copies you'd
delete, not the one you'd keep; *CRAP*'s raw ratio is `Σ max(0, CRAP(m) − threshold) /
(threshold × functions)` (average threshold-excess per function, in threshold units), with
`crapload = Σ CRAP(m)` over threshold-exceeding functions reported alongside; *cycles* counts
only files in tolerance-**reported** cycles (§8 — `Impossible`/idiomatic-skip cycles are not
penalties); *test blind spots* is skipped entirely when the project has no test roots, the
same honesty gate as §9's diagnostic. Health is computed before baseline and suppression are
applied — the score measures the codebase, not how much of it has been acknowledged away.
The score floors at 0 (weights sum to 115).

### 12. Suppression model

- Inline: `kndo:allow <category>[:<subject>] [reason]` in a comment on/above the declaration,
  or `kndo:allow-file …` for file scope. **Adapters extract** the pragmas (comment syntax is
  language-defined — `FileFacts.suppressions`, contracts §2.1); the **core validates and binds**
  them: a declaration-scoped allow covers the symbol and everything it declares. Suppression
  *marks* findings, never deletes them — analyses compute the full set first, then pragmas match
  against it, so an actively-suppressing pragma can never be `stale` and deleting a stale pragma
  can never resurrect a finding (no allow/stale flicker loop; contracts §2.1). `stale` itself is
  not inline-suppressible.
- **Abstention.** An analysis that could not judge (`crap` with no ingested report, `untested`
  with no test roots) emits no findings and returns `Verdict::Abstained`; its categories are
  *unknown* this run, not clean. A pragma naming such a category is **never** matched-nothing
  stale — reading that emptiness as "the issue is gone" is the flicker loop by another route:
  delete the pragma on the advice, add the coverage report, and the finding returns. The
  unknown-category and attaches-to-nothing verdicts still apply: both are structural errors that
  no analysis needs to run to establish. A category is unknown only when every analysis able to
  emit it abstained, and the run reports the set as `run.abstained` with each reason.
- Baseline: `.kndo/baseline.json` acknowledges existing findings at adoption time (RFC 0006 §6).
- Config: per-glob disables of categories or `category:subject` pairs (e.g. `examples/**` exempt
  from `unused`; `unused:enum-member` off globally for codebases with wire-format enums).
  All suppressions are themselves counted and reported (`suppressed: N`) — hidden waste is
  still waste, and a stale suppression (target finding gone) becomes an info finding.

### 13. Candidate rules — triage log & open candidates

Triage of 2026-08-18 (earlier promotions: `internal-only` → §7, `cyclic` → §8):

| Candidate | Decision |
|-----------|----------|
| `duplicate-asset` | **Adopted** — folded into `duplicate` with subject `file` (§6): byte-identical files, free on the blake3 hashes we already compute; lands M1 |
| `orphan-export` | **Dropped, subsumed** — fully covered by `internal-only` (§7), whose remediation ("lower the visibility") is strictly better than "maybe delete the export" |
| `unused-css-variable` | **Dropped, subsumed** — falls out as `unused:css-variable` once the CSS adapter extracts custom-property declarations and `var()` uses, which is already in its §RFC 0002 scope |
| `stale` on suppressions | **Already committed** — in the category registry; **landed in M6** (core `suppression.rs`: unknown-category, unbound, matched-nothing, and `kndo:allow stale` meta-suppression, all `info`/`hygiene` on the pragma's own span) |
| `barrel-abuse` | **Plugin territory** — a JS/TS ecosystem convention, not a language or graph fact; belongs to the js ecosystem plugin, post-1.0 |
| `dead-feature-flag` | **Plugin territory** — the useful version needs flag-system knowledge (LaunchDarkly, Unleash…); a constant-propagation assist in the core may follow real plugin demand |
| `layer-violation` | **Deferred post-1.0** — needs a layering-rules config DSL and does nothing under zero config; parking lot |
| `oversized-unit` | **Deferred post-1.0** — borders the linting non-goal; `crap` already covers the risky (untested) half |

Second triage, 2026-08-18:

| Candidate | Decision |
|-----------|----------|
| `private-type-leak` | **Adopted** into 1.0 (§7): zero new vocabulary — falls out of `TypeUse` edges crossing visibility downward; group `defect`; lands M3 with the visibility machinery |
| `redundant-export-binding` | **Deferred post-1.0**: requires modeling export *bindings* as contract entities distinct from symbols — real vocabulary cost for moderate value; parking lot |

| `deep-import` | ~~Deferred~~ → **Adopted** (superseding decision, same day): the contract-gate design removes the noise objection that motivated deferral — the finding fires only against a provider package that *declares* an explicit surface, so accepted-practice monorepos never see it. Full spec in RFC 0011 §4; group `risk`, pair-level rollup, computed remediation; lands M3 |

Two candidates remain open — neither is implemented in code, and neither has a Decision of the
kind the tables above record:

| Candidate | Mechanism | Status |
|-----------|-----------|--------|
| `hollow-test` | test root whose forward closure reaches zero production symbols | **Open** — the anti-slop "this test tests nothing real" detector (mocks-only tests); needs a named exemption mechanism for every legitimate zero-production-reach case (pure-assertion/property tests, contract tests against an external service) before it can claim zero FP, not just a low rate |
| `speculative-abstraction` | interface/trait with exactly one implementation and at most one consumer | **Open** — YAGNI materialized; trivially derivable from `Implement` edges; would take `probable` confidence (DI/test seams exempt via plugin annotations, library-mode public abstractions exempt) and land in group `waste` if adopted, but adoption itself has not been decided |

Future proposals enter through this table with the §13 acceptance bar; every row carrying a
Decision/Status above is settled, and the two rows immediately above are the exceptions — open
until one is reached.

**Deliberately out of core: stale TODOs.** Detecting aged/orphaned TODO comments requires comment
extraction plus non-graph data (git blame age, issue-tracker state). That breaks the pure
static-graph model; if wanted, it is a plugin with its own data sources, not an analysis.

Acceptance bar for any rule, present or future: derivable from the graph, zero-config by default,
**zero false positives** — not a rate, a hard bar — and explainable in one sentence. A false
positive already costs trust the rule can't earn back, so "rare" is not good enough. Any case
where soundness can't be guaranteed statically (reflection/serialization, dynamic dispatch,
macro-generated consumers, framework conventions the adapter doesn't model) must demote the
finding below the report floor or exclude the case outright — never ship it at reduced
probability. A rule that cannot reach zero FP by this mechanism does not ship, full stop, and
does not get a row in this table either — a rule with a known unclosable blind spot is not a
candidate to triage, it is a rule that has already failed the bar.

**Scope of the bar (RFC 0018).** Everything above covers kndo's own categories — the bare
names in output-schema §6. Plugin-contributed findings (categories under the `plugin:` prefix,
group `convention`) are third-party verdicts the host cannot audit; they live on a separate,
advisory-by-default severity channel, structurally distinguishable by prefix, and are excluded
from the health score and every gate unless the user opts a specific coordinate in via
`[plugins.gate]`. The zero-FP statement neither covers them nor is diluted by them.

## RFC 0012: Reference semantics and visibility

**Status:** Accepted · **Depends on:** RFC 0002, 0005, 0011 · **Changes:** contracts §2 (staged — each
stage updates `contracts/core-traits.md` in the same commit as its implementation, per repo rule)

### 1. Problem

The second language (Go, M3) stress-tested the adapter contract and surfaced a *class* of gaps
with one shared root cause: several facts the analyses need are currently either **encoded
inside strings** (a method's owner hidden in `"T.Method"`), **absent from the contract**
(which declaration a reference executes inside; whether a reference is a type-position use;
what scope each visibility level actually grants), or **fixed too early** (a file's origin
decided from its path alone, before content is ever seen). Each gap was individually worked
around or documented as an imprecision. This RFC eliminates them as a group, with one design
rule: **kndo must be precise, and the precision must live in the core** — adapters supply
declared facts, the core owns every mechanism. No proposal below may encode knowledge of a
specific language in `kndo-core`; every new contract field must map sensibly onto at least
three of the eight launch languages (RFC 0002 §7: JS/TS, Go, Java, Kotlin, Swift, Rust, JSON,
CSS) or it doesn't belong in the contract.

Concrete imprecisions this RFC closes, worst first:

1. A reference to `Method` never resolves to a declaration named `T.Method` — unexported Go
   methods used only inside their package read as `unused:method` (an active false positive,
   the one failure mode the product promises to never have).
2. A live file keeps alive **everything** it references, even references inside its own dead
   functions — transitively dead code is invisible today (under-reporting; the "AI slop"
   kndo exists to catch is exactly this shape).
3. Every `References` edge is hardcoded `RefKind::Read` — `private-type-leak` (RFC 0005 §7)
   is unimplementable, and `Extend`/`Implement`/`TypeUse` evidence doesn't exist.
4. `VisibilityLevel` is a bare index with no declared meaning — `internal-only` can only check
   the file boundary, which under-reports for every package-visibility language (Go, Java,
   Rust, Kotlin).
5. Generated files can't be detected (their marker is content, but origin is fixed at
   claim time from the path alone) — findings fire on code nobody authored.
6. An unaliased import's local name is guessed from the specifier's last path segment —
   wrong whenever the target's declared name differs (`gopkg.in/yaml.v3` binds as `yaml`).

### 2. Design principles (normative for every section below)

- **Data from the adapter, mechanism in the core.** Same pattern as `classify.rs`'s
  `PathPatterns` and `FileFacts::unit`: the adapter declares facts/tables; the core owns the
  single implementation of what they mean. A core `if language == X` still reverts the PR
  (ROADMAP standing rule).
- **Degrade toward keep-alive, never toward accusation.** Every fallback, unresolvable name,
  or missing fact must reproduce today's over-approximation (more code considered alive) —
  never a new way to call live code dead. "Never falsely accuse" outranks precision.
- **Additive and opt-in.** Every new field has a `None`/default that reproduces current
  behavior byte-for-byte. Adapters adopt independently; there is no flag day. A field's
  adoption bumps that adapter's `facts_schema_version` (RFC 0004 §3) — the designed
  invalidation mechanism; no cache migration is ever written (ADR 0004).
- **Confidence is the honesty channel.** Where type information doesn't exist, resolution
  uses RFC 0002 §5's ladder (`certain`/`probable`/`possible`) instead of guessing or refusing
  — the tiers already exist for exactly this.

### 3. Member declarations: `member_of` + visibility-scoped member-call resolution

**Contract:** `Declaration` gains `member_of: Option<SmolStr>` — the declared name of the
owning type, when this declaration is a member of one. The symbol's own `name` becomes the
bare member name (`Method`, not `"T.Method"`); display/symbol-path rendering joins them
(`T.Method`). `SymbolNode` carries the field through to the graph.

**Core mechanism — member-call fallback.** Reference resolution (assembly phase 3b) gains a
final tier after import-bound / same-file / same-unit exact-name lookups miss: the reference
matches member declarations (`member_of.is_some()`) with the same bare name. This is RFC 0002
§5's duck-typing rule ("duck-typed method with one candidate → probable"), finally
implemented, with its scope defined by §6's visibility ladder:

> A member declaration is a candidate iff its declared visibility scope **contains the
> reference site** — an unexported Go method (scope `Unit`) is only a candidate for
> references in its own unit; a public JS class method (scope `Public`) is a candidate
> project-wide.

Confidence: exactly one candidate in scope → `Probable`; several → `Possible` each (all get
edges — conservative keep-alive; `possible` sits below the default report floor, RFC 0006).
Dead-is-certain survives intact: a member with *no* same-named call anywhere is still
`unused` at `certain`. Until §6's ladder lands, the interim scope is same-file + same-unit —
for Go this is *complete by the language's own rules* (unexported members are only legally
callable in-package; exported members are roots, §4 of docs/adapters/go.md), so the interim
is exact for the one language that has members today, not an approximation.

Statically-typed adapters may later do better than the fallback (receiver-typed resolution
via `scope_context`, §9) — the fallback is the floor the core guarantees, not a ceiling.

**Language fit:**

| Language | `member_of` maps to |
|----------|--------------------|
| JS/TS | class/interface members (methods, fields, getters — js-ts.md §2 already promises them), enum members (owner = enum) |
| Go | methods via receiver type (`func (t T) M()` → `M` member_of `T`) |
| Java | every method/field (owner = enclosing class) — Java has no non-member functions |
| Kotlin | class members; top-level functions have `None` |
| Swift | members of struct/class/enum/protocol/**extension** (owner = extended type's name) |
| Rust | `impl T` fns (owner `T`), trait fns (owner = trait name) |
| CSS/JSON | `None` always |

### 4. Symbol-granular reference attribution: `within`

**Contract:** `RawReference` gains `within: Option<SmolStr>` — the declared name of the
symbol this reference executes inside, under one language-blind rule:

> **`within` = the declared symbol whose *use* triggers this code.**
> - Body of a callable (function, method, initializer that runs on call) → that callable.
> - Code that runs when the module/file **loads** (top-level statements, package-level
>   variable initializers, JS static class blocks — anything unconditional at load) → `None`.
> - Code that runs when a type is **instantiated or first used** (constructors, instance
>   field initializers, Java static initializers — which are lazy on first class use,
>   Swift lazy globals) → that type/symbol.

The principle decides the cases, not the list — the list is illustrative. Member bodies name
their `within` exactly as the `Declaration` is named (bare name + the §3 `member_of`
convention), so `within` resolution reuses the same tables.

**Core mechanism.** Phase 3b resolves `within` against the file's own symbol table and emits
`References { from: NodeRef::Symbol(enclosing), .. }` when it resolves; **any miss falls back
to `NodeRef::File` — today's behavior, the safe direction** (this fallback is the load-bearing
safety property; it gets its own regression test).

**Twins: the name is not enough, the span is.** `within` is a NAME, and same-name overloads
are legitimate twins sharing one `Owner.name` selector — Swift's `get(at:)` beside
`get(path:)`, Java's arity overloads. The qualified table is single-slot, so a name lookup
attributed every reference in EITHER body to whichever twin was inserted last; the other had
no outgoing references at all and read as dead unless something else named it (vapor's private
`get`, called from its public sibling one line above). Resolution therefore disambiguates among
`insert_qualified`'s twin set by **span containment** — a reference lies physically inside
exactly one declaration, so the containing one is its author. Exact, language-blind, and it
falls back to the single-slot answer when no candidate contains the span, so an adapter
reporting `within` without a matching span is no worse off than before. `graph::assemble`'s
`within_owner` is the one implementation. The reachability algorithm needs *zero
changes*: adjacency is already `NodeRef`-keyed, `Symbol → Symbol` edges already traverse, and
the symbol-reaches-its-owning-file propagation (added in M3, RFC 0005 §1) is the second half
of this model — formally:

> **Module-load rule:** reaching a symbol reaches its owning file (using a symbol loads its
> module — the file's `within: None` references and its `ImportsFile` edges fire).
> **Execution rule:** a symbol-attributed reference fires only when its symbol is reached.

Together these make transitive death visible: `main → a` keeps `a` alive; dead `z → b` no
longer keeps `b` alive. They also sharpen `test-only` (a test-only function's callees color
test-only instead of inheriting the file's production color) and — with §5 — let a dead
function's signature types die with it.

`DynamicUse`/wildcard expansion stays file-granular in this RFC (coarser = safer; revisit only
with evidence). Navigation (`uses`/`used-by`/`trace`) and finding evidence gain real
"which caller" attribution for free — the file-granularity apology in `engine.rs` is deleted.

**Consequences, stated honestly:** on real repos, findings *appear* that were previously
masked (transitively dead code). That is the point, but it mandates a dogfooding pass on the
JS corpus and Go fixtures before trusting, updated conformance fixtures in the same commit,
and (pre-1.0) accepting baseline churn.

**Language fit:**

| Language | callable bodies | load-time (`None`) | on-use/instantiation |
|----------|-----------------|--------------------|---------------------|
| JS/TS | functions, methods, arrows bound to a declaration | top-level statements, static blocks/fields (class evaluation runs at load) | constructor + instance fields → the class |
| Go | func/method bodies, `init` | package-level var/const initializers | — |
| Java | method/constructor bodies | — (class loading is lazy) | field initializers, instance *and static* init blocks → the class |
| Kotlin | functions, methods | top-level property initializers | init blocks/constructors → the class; companion initializers → the class |
| Swift | funcs, methods, closures bound to a declaration | — | lazy globals → the global's own symbol; type members → the type |
| Rust | fn bodies | — (no load-time execution) | const/static initializers → the const/static (compile-time, but the *dependency* is real: a dead const's referents die with it) |
| CSS | `@mixin`/`@function` bodies (SCSS only) → that callable's own symbol | plain rule bodies (no rule-level symbol exists to attribute to — selector/class/id extraction is deliberately out of v1 scope, docs/adapters/css.md §0) | — |

### 5. Reference kinds & signature spans → `private-type-leak`

**Contract:** two fields.

- `RawReference.kind: RefKind` — the vocabulary (`Call/Read/Write/Extend/Implement/Override/
  TypeUse`) has existed since M0; the core stops hardcoding `Read` and passes the adapter's
  kind through to the edge. Adapters adopt incrementally (untagged = `Read`, exactly today).
- `Declaration.signature_span: Option<Span>` — the sub-span covering the declaration's
  *signature* (parameters + return/result types; everything before the body). The adapter
  knows where a body starts; the core must not.

**Core mechanism — `private-type-leak` (RFC 0005 §7's second half), as pure core analysis:**
for each exported declaration `D` with a `signature_span`, each `TypeUse` reference whose span
lies inside it, resolved to a symbol `T`: if `T`'s visibility scope (§6) is narrower than
`D`'s → finding (group `defect`). Zero language knowledge; evidence is the reference span.

**v1 scope, honestly bounded:** callables only. A *type's* leak surface is its exported
*fields* — which requires fields to exist as member declarations with their own visibility
(§3) and their own type spans; that lands with member extraction, not before. Firing on a
whole struct body without field-level visibility would accuse exported-struct/unexported-field
cases falsely — exactly what the degradation principle forbids.

**Language fit:** Go gets `TypeUse` nearly free (`type_identifier` node kind *is* the
type-position signal) and embeddings → `Extend`. TS: type positions → `TypeUse` (extraction
work), `extends`/`implements` → `Extend`/`Implement`, `import type` bindings → `TypeUse`.
Java/Kotlin/Swift: extends/implements/conformance clauses and signature type positions map
one-to-one. Rust: `impl Trait for T` → `Implement`, path-in-type-position → `TypeUse`.
CSS: `var(--name)`/bare SCSS `$name` → `Read`, `@include`/any other call-expression → `Call` —
no `TypeUse`/`Extend`/`Implement` analogue exists (no type system). `composes` (CSS Modules)
would also be `Read` in spirit, but stays unimplemented in v1 alongside selector/class
extraction (docs/adapters/css.md §0/§5) — nothing to resolve it against yet. JSON: none.

### 6. The visibility ladder as data

**Contract:** `AdapterDescriptor` gains the ladder the adapter's `VisibilityLevel` indices
into — each rung declaring its **scope** (a graph concept the core already owns) and its
**label** (the language's own word, for remediation text — RFC 0005 §7 requires remediation
"in the language's own terms, supplied by the adapter"):

```rust
pub struct VisibilityRung { pub scope: VisibilityScope, pub label: SmolStr, pub surface_transitive: bool }
pub enum VisibilityScope { File, Unit, Package, Public }
// AdapterDescriptor gains: pub visibility_ladder: Vec<VisibilityRung>  (index = VisibilityLevel)
```

`File` = same file · `Unit` = same `FileFacts::unit` key · `Package` = same `PackageId`
(RFC 0011) · `Public` = everywhere. Two rungs may share a scope (the ladder is the language's
own level list; the scope is what the core can check). Empty ladder = visibility analyses
skip the language entirely (CSS, JSON).

**`surface_transitive` (M6):** whether a re-export chain can carry a declaration at this rung
*outside its package* — an axis `scope` cannot express. Rust `pub` and a JS `export` are
**relative** (as visible as the module path re-exporting them → `true`); Rust `pub(crate)`,
Java package-private, Swift `internal`, and Go exports under an `internal/` path element are
**capped** (no re-export widens them → `false`). Two core mechanisms read it: library-mode
symbol promotion (RFC 0011 §5 — only transitive rungs are consumable surface) and the
surface-member closure (a surface type's transitive members are surface too — a `pub` method
of a re-exported struct is consumer-callable API even with zero in-package references).
Note Java `protected` and JS "exported" are transitive despite non-`Public`-looking
consumption paths: external subclasses override `protected`, and a JS entry file's exports
are the package surface by definition.

**Core mechanism — `internal-only` generalized:** tightest-sufficient visibility = the lowest
rung whose scope contains **every** incoming reference's origin (checked per edge against
facts the graph already has: same file / same unit / same package). Declared rung above it ⇒
finding; the remediation names the lower rung's `label`. This replaces today's file-boundary
approximation, fixes the documented Go under-reporting (an exported symbol used only by
same-unit siblings → "could be `unexported`"), and is what §3's member-fallback scoping and
§5's leak comparison read from — one ladder, three consumers.

**Language ladders (adapter-declared data, listed here as the design record):**

| Language | ladder (index → scope, label) | notes |
|----------|-------------------------------|-------|
| JS/TS | 0 `File` "module-local" · 1 `Package` "exported" · 2 `Public` "package surface" | rung 2 = reachable through the `exports` map (js-ts.md §4); adapter emits 0/1 today, 2 lands with surface-awareness |
| Go | 0 `Unit` "unexported" · 1 `Package` "exported (internal)" · 2 `Public` "exported" | rung 1 (M6): an export under an `internal/` path element — the compiler itself walls it off from external modules, so it is capped (`surface_transitive: false`) and Package-scoped, keeping it out of the library-surface machinery while `internal-only` can still advise narrowing; the adapter assigns it by path, the one visibility fact Go keeps outside the identifier |
| Java | 0 `File` "private" · 1 `Unit` "package-private" · 2 `Public` "protected" · 3 `Public` "public" | `private` ≈ enclosing file (nested classes share it); `protected` maps conservatively to `Public` — subclasses live anywhere, never suggest narrowing onto them — and is **exported** (subclass-consumable API, M6); interface/annotation members with no modifier are implicitly `public` (JLS §9.4) |
| Kotlin | 0 `File` "private" · 1 `Package` "internal" · 2 `Public` "protected" · 3 `Public` "public" | `internal` = compilation module ≈ Package (unlike Java, Kotlin's `package` carries no visibility meaning at all — the default with no modifier is `public`, not package-scoped); `protected` (members only, same "package ∪ subclasses anywhere" shape as Java's) maps conservatively to `Public`, mirroring Java's own two-rungs-share-a-scope pattern |
| Swift | 0 `File` "private" · 1 `File` "fileprivate" · 2 `Package` "internal" · 3 `Public` "public" · 4 `Public` "open" | the ladder applies uniformly at top-level and member position (no restricted subset); `internal` — the default with no modifier at all — is a *third* distinct default among the launch languages (Java ≈ `Unit`, Kotlin = `Public`); `open` (subclassable outside the module) maps conservatively to `Public` alongside `public`, mirroring Java's `protected`/`public` collapse — kndo's scope model can't distinguish the two |
| Rust | 0 `Unit` "private" · 1 `Package` "pub(crate)" · 2 `Public` "pub" | unit = module; `pub(super)`/`pub(in …)` map to the nearest **wider** rung (conservative) |
| CSS/JSON | `[]` | visibility analyses skip |

Conservative-mapping rule (normative): when a language level has no exact `VisibilityScope`,
the adapter maps it to the nearest **wider** scope — over-approximating who may see a symbol
can only suppress an `internal-only` finding, never fabricate one.

### 7. Content-derived origin: `detected_origin`

**Contract:** `FileFacts` gains `detected_origin: Option<FileOrigin>` — extraction may
*correct the origin axis* of the claim-time `FileClass` (role stays claim-time; no use case
justifies content-derived roles yet). Assembly applies the override when building `FileNode`,
before the role-derived-roots phase, so every origin exemption (`unused`, `test-only`,
`untested`, `internal-only` all exempt `Generated`) sees the corrected value.

**Why this shape:** origin-by-content is a fact *about the content*, and `FileFacts` is the
content-addressed fact bundle — the override rides the facts cache with zero extra I/O and no
claim-time slowdown. The rejected alternative (a content-peek at claim time) breaks claim's
"fast, name-based" property and discovery parallelism.

**Toolkit:** a data-driven first-N-lines scanner (`ContentMarkers { line_patterns,
scan_window_lines }`, mirroring `PathPatterns`) for comment-marker languages. The *field* is
the contract; the scanner is a convenience — an adapter with a structured signal (Java's
`@Generated` annotation, parsed from the AST it already has) sets the field from its own
parse instead.

| Language | generated signal |
|----------|------------------|
| Go | `^// Code generated .* DO NOT EDIT\.$` (the `go generate` convention — single authoritative source) |
| JS/TS | `@generated` markers, codegen banners (js-ts.md §1's existing list, finally actionable) |
| Java/Kotlin | `@Generated`/`@javax.annotation.Generated` annotations (AST-derived, not line-scanned) |
| Swift | `// Generated by` banners (sourcery et al) |
| Rust | `// @generated` / build-script banners |

Known non-goal: JS's ".d.ts sibling of a same-name .ts" rule needs *cross-file* knowledge —
neither claim (one path) nor extract (one content) can see siblings; that stays a documented
js-ts gap, unsolved by this RFC rather than half-solved.

### 8. Unit-key conventions (no contract change — a design record)

`FileFacts::unit` is an opaque key; the core only groups by it. That opacity is load-bearing:
adapters encode their language's *real* resolution unit in it without the core learning
anything. Conventions per language, recorded so adapters stay mutually consistent in spirit:

**A unit key is unique only within a package.** Java/Kotlin key on the declared package name
and Swift on the target name, so two Gradle modules declaring `package retrofit2;`, or two
Swift packages each declaring a target `Core`, share one key repo-wide. The core therefore
keeps the reverse index in two forms — repo-global, and partitioned by owning package
(`graph::assemble::build_unit_indexes`, shared with the patch path) — and
`ResolveCtx::unit_files_from` prefers the importer's own package, falling back to global only
when it has no candidate. Without the preference a resolver picking `.first()` by path order
binds intra-module imports to unrelated siblings; without the fallback genuine cross-module
imports stop resolving. Reachability never depended on which file an edge landed on (same-unit
fallback), but `cyclic` reads the literal edge as evidence, and read phantom package cycles out
of it in four of the six languages a field audit covered.

**And a NAME is not unique within a unit.** The mirror of the §4 twins case, on the incoming
side: two files of one unit may legitimately declare one name when the language makes them
mutually exclusive — Go's `//go:build` alternates (gin's `binding.go` under `!nomsgpack` and
`binding_nomsgpack.go` under `nomsgpack`, both `func validate`), Rust's `#[cfg]` alternates.
Span containment cannot disambiguate here (the twins sit in *different* files and the
reference is inside neither), and nothing should: kndo analyzes the union of build
configurations, so both declarations are live and a reference under either configuration
reaches its own. The single-slot unit table gave the last-inserted twin every reference — 16
of them in gin — and left the other at zero incoming edges, falsely `unused`. Core therefore
keeps the displaced declarations (`symbol_twins_per_unit`, built beside the name table in
both `graph::assemble` and `graph::patch`) and emits the edge to **every** twin. Twins are
consulted only when the winner came from the unit table: a name bound by an import, or
declared in the referencing file itself, is one specific symbol and not a member of a twin
set. Same keep-alive direction as everywhere else — an extra edge to a twin that some build
configuration excludes costs recall under that configuration, never a false accusation.

**Which tier resolved decides which twins apply.** The bare-name ladder is: names bound by
this file's imports, then its own declarations, then its unit's, then units a wildcard import
makes visible. Kotlin multiplatform is the case that exercises every rung — `expect` in
`common` beside one `actual` per platform, all under one declared package, hence all twins of
each other — and the callers reach them from all four positions: an explicit import (the
binding's own twin set, recorded when the binding resolved through the TARGET's unit table),
the declaring file itself (`expect inline fun yieldThread()` sits in the file that calls it),
a sibling file of the unit, and a wildcard-imported package. Each tier therefore carries the
unit's full twin set for the name, minus whichever member of it that tier returned.

**Wildcard-visible names rank BELOW members in scope.** The last rung is consulted only after
the §3 duck-typed member fallback comes up empty, which is where every language with this tier
puts it: Kotlin resolves an unqualified call against local names, then implicit receivers, and
only then imported top-level names. Ranked above the fallback instead, an unrelated top-level
`updateState` in a wildcard-imported package took both call sites of kotlinx.coroutines'
`StateFlowImpl.updateState` — a method calling its own type's member — and left it `unused`.

| Language | unit key |
|----------|----------|
| Go | `dir#declared-package-name` — splits external test packages (`foo_test`) from `foo` in the same directory, closing the documented §1.1 imprecision of docs/adapters/go.md with zero core changes |
| Java | declared package name (dotted string from the `package` statement) — never directory-derived, sidestepping source-root detection (`src/main/java` is a build-tool convention, not language-visible from a bare file path); docs/adapters/java.md §0 |
| Kotlin | same as Java (declared dotted package name) — but note this key carries *zero* visibility meaning for Kotlin (§6), only resolution meaning (same-package unqualified reference, wildcard import enumeration) |
| Rust | the file's own module, keyed by path with `mod.rs`/`lib.rs`/`main.rs` folded into their directory — so **one file per key**. Rust files still resolve nothing implicitly across files (a one-file unit table IS the file's own, already the earlier tier), which is exactly why turning the key on moves no resolution; what it buys is twin tracking, which is per-unit and which a keyless language therefore had none of. `internal/adapters/rust.md` §2 |
| Swift | target/module name |
| JS/TS, CSS, JSON | `None` — file-scoped languages |

### 9. Qualified-reference resolution in the core (accepted direction, scheduled after §§3–7)

The remaining string-guess in the system: an unaliased import's local name derived from the
specifier's last segment. Correct resolution needs the *target's declared name* — knowable
only where both sides exist: assembly. Contract (when scheduled): `RawImport.local_alias:
Option<SmolStr>` (the explicit alias, else `None`), `FileFacts.unit_name: Option<SmolStr>`
(the name importers bind this unit by — Go's `package` clause, Rust's module name), and the
already-existing-but-unused `RawReference.scope_context` carrying the receiver/qualifier
text. Phase 3b then resolves `qualifier.member`: qualifier matches the import's
`local_alias`, or — unaliased — the resolved target's `unit_name`. This deletes the
adapter-side dotted-binding synthesis (Go) and, long-term, subsumes JS's namespace-member
machinery (`ns.foo`) under the same core rule. Deferred because its current blast radius is
small (external targets have no in-graph symbols to mis-bind; in-repo dir≠package mismatches
are rare) and §§3–6 change the same code paths — land those first, then refactor once.

**As landed (M6, extended):** a matched qualifier resolves in order — the target file's bare
table, its unit siblings, then its *member table*: the alias may name a TYPE rather than a
module (`Thing::from_low_args()` through `use crate::thing::Thing`), where the member lookup
follows the qualifier symbol to its home file first (a barrel's re-export alias lands on the
original, so `SearchMode::Standard` through `use crate::flags::{SearchMode}` reaches the
declaring file's members). Hit or miss, a matched *alias* still settles. A qualifier matching
no alias but matching an **import binding** resolves `Original.member` in the bound symbol's
home file at Certain — and on a miss does NOT settle: a binding is a value/type, not a closed
namespace, so an unknown member falls through to the §3 duck-typed fallback exactly like a
receiver expression. Re-exported GLOBS (`pub use x::*`, `export * from './x'`) alias the
target's exported surface into the barrel inside the RFC 0013 §3b fixpoint (or-insert
collision rule; alternates that lose the collision stay alive through the glob's Wildcard
edge). In-source roots targeting a declaration name land on EVERY declaration sharing the
selector — twins are legitimate (two `impl Add for Stats` blocks both declare `Stats.add`).

**§3-bis, receiver typing (M6):** adapters may pin a receiver's TYPE from facts local to the
file (Rust: `self`/`Self` → impl owner, typed params/lets, struct-literal and `T::assoc(…)`
initializers with chain flow — see the adapter spec) and emit the type as `scope_context`
instead of the opaque receiver name. Core-side, the qualifier resolution above gains one
tier: a qualifier matching a name in scope — an import binding OR a same-file declaration —
resolves `Original.member` in that symbol's home file at Certain, and on a miss falls
through to the §3 duck fallback (never settles: a name in scope is a value/type, not a
closed namespace). Qualified-member hits land on EVERY declaration sharing the selector
(cfg-alternated twin impls both own `Data.from_path`; the single-slot table's displaced
twins are tracked and each gets the edge). The reliability invariant, both sides: a wrong
receiver type can only miss into the fallback or hit a member the named type genuinely
declares — silence-direction errors only.

**§3-bis cross-file tier (M6): member-type facts.** `FileFacts.member_types` carries
`(owner, member, yields)` — what accessing a member evaluates to, as declared (field types,
method returns, associated consts). A DOTTED qualifier is a chained pointer
(`LowArgs.context_separator`): the base resolves in the reference's scope, `yields` comes
from the owner's home-file facts, the yielded type name resolves first where the annotation
was written (the home's declarations + re-export aliases) then in the reference site's own
scope, and the final member resolves in the yielded type's home — twins included, every hop
a declared fact, Certain on hit, duck fallback on any miss (never a settle). The resolved
chain also credits the yielded TYPE with a Read from the site. Facts are part of the
RFC 0013 §4 surface signature (an annotation change re-resolves dependents) and persist in
`FilePatchMeta` for the patch path. `RawMemberType.yields` carries the annotation's arguments
as written (`Result<ConfiguredHIR, Error>`); a pointer segment marked `?N` projects argument N
instead of the wrapper (`?` is shorthand for `?0`). The marker is structural — WHICH argument an
operation extracts is the adapter's knowledge (Rust's try operator → 0), the core just indexes.
**As landed, `yields` is a tree and a projection lands on a subtree** (§3-quater below): the
one-level list this paragraph originally described could not hold `Result<Vec<T>, E>`'s `T`. Pointers compose to
N hops (each hop a declared fact, each resolved hop's type credited with a Read from the
site); a projection index with no parameter at that position is a miss — duck fallback,
never a settle.

**§3-ter, a value's type from the call that produced it (M6).** `let entry = parse_entry(..);
entry.path` — the receiver's type is the callee's declared RETURN, which lives in the callee's
file. `RawMemberType::owner` is therefore `Option`: `None` reads "calling `member` evaluates to
`yields`", the same statement about a value's type that `Some(owner)` makes about a member
access, so the core walks it with the same chain machinery — a `None`-owner fact simply applies
where a pointer's BASE names the function, one step before `chain_hop` acts on a member segment
(`call_yield`). The projection marker composes there too: `let cfg = build(..)?` takes parameter 0.

A pointer's base also stopped having to be a symbol. Rust reaches free functions through their
module constantly (`rollup::directory_rollups(..)`), and there the file binds only the MODULE —
so `pointer_base` resolves a base that matches no name in scope against the qualifier table,
consuming the next segment as the symbol inside that target (its bare table, then its unit
siblings, the same pair every other tier uses). Without that arm the pointer died at its first
segment and every field the caller read looked file-local.

The adapter side of the same tier is language knowledge, and stays in the adapter: a member-type
fact for every top-level function's declared return; a fact from `#[derive(Default)]` (the derive
states the impl exists, the trait's signature states what it returns — curated stdlib knowledge
like `is_machinery_trait`, not inference); a bare or module-qualified callee typing its binding by
naming the FUNCTION rather than guessing its return; and a `for` variable projecting the
iterable's parameter 0, only where the iterable is a chain whose last hop is a declared fact — a
local's own annotation kept just its base name, so `Vec` alone has no parameter to project.

Measured on this repo: three types that carried an acknowledgement pragma
(`internal/detection-gaps.md` §3 — `DirRollup`, `ContributedRoot`, `ContributedEdge`) stopped
needing one, with the finding count and health unchanged and no movement in either direction on
six other codebases (§4's symmetry requirement).

**§3-quater, the chain carries a TYPE (M6).** `RawMemberType::yields` was a base name plus a
one-level list of parameter names, and the chain walked `SymbolId → SymbolId`. Both halves of
that lose the same thing: `Result<Vec<TreeEntry>, GitError>` reduced to `Result` +
`["Vec", "GitError"]`, and the `TreeEntry` was gone from the facts entirely — no projection
could recover what was never stored. Flattening was not a representation detail; it was the
reason `internal/detection-gaps.md` §3's last case could not be fixed.

`yields` is now a `TypeExpr` — `Named { name, args }` | `Param(N)` | `Unknown` — and the chain's
state is one of those plus, when the head name resolves to something this project declares, the
symbol whose home file holds its member table. The two are separate because they genuinely are:
`Vec<TreeEntry>` is a type no file here declares, so it has no symbol, and the chain still has to
walk *through* it to reach the `TreeEntry` inside. A `?N` marker means exactly what it meant, and
now lands on a SUBTREE with its own arguments intact.

Three pieces follow from that, all language-blind:

- **`Param(N)` and substitution.** A fact may state a relationship rather than a type — "`map_err`
  still yields a `Result` over the same argument 0" — and the receiver's own arguments make it
  concrete at the hop. A parameter the receiver does not have becomes `Unknown`: silence, not the
  type that happened to sit at that position. `Unknown` is a variant rather than a short argument
  list so a fact never has to lie about its arity to admit it cannot name something.
- **`AdapterDescriptor::builtin_member_types`**, a second lookup tier keyed by claim language,
  consulted after the owner's home file. It is the only tier that can apply when the head resolves
  to no declaration at all, which is every type the language itself provides. Same
  data-on-the-descriptor path as the ladder and the cycle policy; the knowledge stays the
  adapter's, the core just gets another table.
- **The chain carries the file its type was WRITTEN in.** A builtin hop introduces no home of its
  own: `Vec<TreeEntry>`'s `TreeEntry` was written wherever the receiver's fact was, and resolving
  it against the reference site instead finds nothing — the site imports the container's owner,
  not every type inside it.

Iteration needed no new syntax and got none. A `for` variable's pointer ends in an ordinary
member segment whose name the adapter chooses on both sides (Rust uses `@element`), and the
adapter's builtin table declares, per container, which argument iterating one yields. A container
that declares none simply does not type its loop variable — a map iterates to a tuple, which this
model has no way to name, and silence beats a confident wrong element. The same trick names
anonymous types: Rust calls a slice `@slice` so it can carry an `@element` like any container. The
core never interprets either string.

Persistence stayed out of the contract's way. rkyv 0.8 cannot derive `Archive` for a type that
recurses through `Vec<Self>`, so `crate::rkyv_support::TypeExprAsFlat` archives the tree as a
preorder walk with arity — each atom carries its own, its children are the next `arity` subtrees,
and a cursor rebuilds it with no indices to dangle. That module exists for exactly this: the
archive format does not get to dictate the shape adapters write.

Measured: `gitutil::TreeEntry`, read through `ls_tree(..).map_err(..)?` and then iterated, was the
last of §3's four acknowledged types. All four pragmas are gone, kndo's own finding count and
health are unchanged, and six other codebases moved in neither direction.

**§9-quater, what a synthesized import may and may not do (M6).** An adapter may reconstruct an
import from a use site — Rust writes `crate::a::b::f()` with no `use` in sight, and resolution
needs something to bind. `RawImport::reconstructed` says so outright. It replaced a proxy that was
wrong: §9-ter derived "does a qualifier miss settle" from the import's CONFIDENCE, and Rust's
`crate`/`self`/`super`-rooted synthetic imports are `Certain` about where they resolve while being
no statement at all. Confidence answers "does this resolve as stated"; this answers "did the file
state it". Two rules follow.

**A synthesized binding ranks below the file's own declarations.** The bare-name ladder is five
tiers now — a STATED import, the file's own declarations, a RECONSTRUCTED import, the unit, the
units a wildcard makes visible — and the middle move is the fix. No language kndo supports lets a
written import shadow a same-named local declaration (Rust E0255), so a collision there can only
ever come from a synthetic one, claiming a precedence the language never grants it. tokio's
`dump.rs` declares `pub struct Trace` and merely *mentions* `super::task::trace::Trace` in a
field; the synthetic binding captured the file's own `-> &Trace` and `private-type-leak` reported
a leak that was not there. Every site that asks what a bare name refers to shares one function
(`graph::assemble::name_in_scope`) so the order cannot drift between them.

**A qualifier names every file bound to it, not the first.** Rust's platform modules are
`#[cfg(windows)] #[path = "windows/sys.rs"] mod imp;` beside a `not(windows)` twin, so
`imp::ctrl_break()` names two live functions and the union-of-configurations rule (§8) says both
are real. `qualifier_targets` holds a LIST; a hit emits to all of them. Keeping the first left
every alternate but one with no incoming edge and a false `unused` — the same shape
`symbol_twins_per_unit` fixes for declarations, one level up at the module binding. A hop (§9-bis)
still fills only a qualifier nothing else claimed: a direct alias is stronger provenance.

**§9-bis, the one hop through a module file (M6).** `use crate::internals::{attr, check, Ctxt};`
followed by `check::check(cx, …)` registered ONE qualifier — the specifier's own target — and
left `check` a mere binding that resolved to no symbol, because `internals/mod.rs` declares
nothing by that name: it only re-links the file with its own `mod check;`. The chain is *a
binding on the target's own import table*, and no single pass can follow it (the target's table
does not exist when its consumer is resolved). Phase 3b is therefore three passes, shared by
`graph::assemble` and `graph::patch` alike — one resolution semantics, two data sources:

1. `resolve_imports` — per file, no cross-file dependency. Alongside the bindings, qualifiers,
   visible units and edges it already produced, it records `module_bindings`: every name this
   file's imports put in scope that points at an in-repo FILE (`graph::ModuleBinding`). Both
   shapes count and the pair is the point — a brace member kept even though it bound no symbol
   (the consumer side), and an import's `local_alias`, which is where a file-linking `mod x;`
   carries its name when it has no bindings at all (the producer side).
2. `link_module_bindings` — per file, reading every file's table. A name bound to file F that
   F's own table binds again registers the qualifier F sends it to. **One hop, never a
   fixpoint** (chasing further needs a cycle guard for evidence nobody has produced);
   **non-settling**, like every derived qualifier, so a miss takes the in-scope/duck ladder
   rather than binding the access to the wrong file and killing a live method; and an existing
   qualifier **always wins** — a direct alias is stronger provenance than a hop.
3. `resolve_references` — per file, the reference/dynamic/diagnostic body, against its own
   (now hop-extended) import resolution.

Language-blind by construction: no separator, no path arithmetic, nothing but "this name binds
to that file, and that file binds this name to another file." The alternative — extend the
specifier and re-resolve `crate::internals::check` — would teach the core that `::` joins path
segments, which the ignorance rule forbids.

No RFC 0013 §4 signature change was needed: `surface_signature` already hashes a file's complete
import list, so any change to F's imports moves F's surface and forces a full rebuild — a stale
hop is unrepresentable. `FilePatchMeta` carries each file's `module_bindings` so an unchanged
file contributes its table without re-extraction; `patch_equivalence` is the harness.

**§9-ter, the last hardcoded separator (M6).** With §9-bis's hop in place, the core stopped
deriving a qualifier from the specifier's last `::`-separated segment — the one place it knew a
language's path syntax. Two Rust shapes depended on that guess and now state the name
themselves, in `local_alias`: the single-qualifier bare path (`helpers::run()`) and the deep
one (`kndo_core::discovery::find_files_named(..)`), both of which the adapter reconstructs as
synthetic imports and both of which it qualifies by a segment only it can identify. What the
guess also did — registering `b` for `use a::b::{X, Y}`, where Rust does **not** bring `b` into
scope — is simply gone; the adapter already declines to set `local_alias` there, and did before
this change.

The alias's *strength* is no longer uniform, and it is the import's own confidence that decides
it: a qualifier miss settles iff the import is `Certain`. A real `use`/`import` statement names
a closed namespace — the member is in there or nowhere — while a reconstructed import is the
adapter's reading of a path, and closing a namespace on a misread path would kill a live method.
That is the same asymmetry §9 already stated between an alias (settles) and a binding (does
not), now derived from a fact the contract already carried instead of from which code path
produced the qualifier. Measured across serde, alacritty, axios, Exposed, kotlinx.coroutines,
vapor and kndo itself: byte-identical findings, so the adapter-side alias reproduces the split
exactly and the dropped over-approximation cost nothing.

### 10. Multi-module topology (`go.work` et al) — adapter work, one recorded divergence

`go.work` becomes a second claimed manifest contributing `workspace_members` (the field
exists); member modules already self-register by declared module path, and local `replace`
directives resolve through the same name index when the target declares the same path. The
one true gap — a `replace` that *renames* a module path — is a recorded divergence, not
modeled. Analogues: Cargo workspaces + path deps (same shape), Gradle `settings.gradle`
includes, SPM local packages. No contract change anticipated.

### 11. Cache, determinism, migration

Every serialized-shape change (`Declaration`, `RawReference`, `FileFacts`) bumps the adopting
adapter's `facts_schema_version` — invalidating only that adapter's facts (RFC 0004 §3); the
graph key already folds adapter versions, so graph snapshots self-invalidate. No migration
code, ever (ADR 0004). Determinism (RFC 0008 §4): every new resolution tier iterates sorted
candidate sets; the member-fallback and ladder checks are pure functions of the graph.

### 12. Landing order (each stage = code + contract doc + fixtures in one commit)

1. **§3 `member_of` + member-call fallback** (interim same-file/same-unit scope) — kills the
   active false positive; fixes the naming convention §4 depends on.
2. **§8 Go unit-key refinement** — trivial, adapter-only.
3. **§4 `within`** — core + Go adapter (the simple adopter: no hoisting, no CJS).
4. **§4 `within`** — JS/TS adapter (the bulk: class taxonomy, multi-pass threading).
5. **§5 `RefKind` + `signature_span` → `private-type-leak`** (callables-only v1).
6. **§6 ladder-as-data → `internal-only` generalized** (upgrades §3's fallback scope too).
7. **§7 `detected_origin`** — independent; any time after 1.
8. **§9, §10** — when dogfooding demands them; §9 explicitly after 1–6.

Analyses affected at each stage keep their existing conformance fixtures green *or* update
them in the same commit with the diff explained — a fixture change without an explanation
line in the commit message is a red flag, not a formality.
