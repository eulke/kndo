# RFC 0013 — Incremental Graph Patch

**Status:** Accepted, implemented (same milestone) · **Depends on:** RFC 0004 (§4 step 4 is the accepted target this
implements), RFC 0008, RFC 0012 · **Related:** ADR 0004

## 1. Problem, with the M4.5 numbers

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

## 2. Design stance: derive the dirty set, never guess it

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

### 2.1 The dependency inventory

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

## 3. Prerequisite hardening — fix the foundation first

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

## 4. Persisted additions (graph schema 10)

| Addition | Why |
|---|---|
| `Edge.owner: FileId` | explicit ownership (§2 point 1); removal set = `owner ∈ C`, exact |
| per-file `surface_sig: [u8; 32]` | the §2.1 detector: blake3 over the file's span-normalized surface — adapter id + facts schema version + class(role, origin) + `unit` + `unit_name` + declarations `(name, kind, exported, visibility, member_of)` in order + imports `(specifier, bindings(local, imported), reexported, opaque_namespace_use, local_alias)` in order + in-source roots `(kind, target, confidence)` in order + dynamics `(narrowed_to)` in order. Spans excluded everywhere — bodies and positions move freely under the guard. Unclaimed files: no signature (nothing derived to guard) |
| per-file `unit_name: Option<SmolStr>` | qualifier defaults (RFC 0012 §9) currently live only in facts; targets' facts must not be re-fetched |
| per-file re-export alias table `Vec<(SmolStr, SymbolId)>` | the one table not derivable from `graph.symbols`; post-(b) it is order-independent state, safe to reuse |
| `graphs/latest` pointer (last written key) | a miss must find the previous snapshot; content-keyed filenames can't |

Everything else the patch needs is already derivable from the snapshot: bare/qualified/unit
symbol tables and the member table from `graph.symbols`; `library_root_files` from kept
manifest-owned `Root` edges; `role_root_files` from the file's own class; ladders, cycle
policies, packages, declared dependencies — stored since M3/M4.

## 5. The patch algorithm

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

## 6. Equivalence obligation & enforcement

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

## 7. Explicitly out, with reasons of record

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
- **Plugin contributions** (RFC 0004 §4 step 4c): the plugin system contributes nothing to
  the graph yet (only coverage, which is per-run by design, ADR 0005); `Edge.owner` is the
  hook their invalidation will use.

## 8. Rollout

Three commits, suite-green after each: (1) §3(a)+(b) hardening — canonical order and
fixpoint aliasing (no snapshot semantics touched); (2) schema 10 — §3(c)'s diagnostics
split plus the persisted additions: owner, signatures, unit_name, alias tables, latest
pointer (additive; the patch not yet active); (3) the patch path + equivalence suite +
measured numbers in the commit message. `--no-cache` remains
the operational escape hatch — no new knobs.
