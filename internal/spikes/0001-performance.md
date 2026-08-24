# Spike 0001 — Warm-Run Budget Validation

**Date:** 2026-08-18 · **Status:** Done · **Validates:** RFC 0001 §5, ADR 0004, RFC 0008
**Code:** `spikes/perf/` (disposable; this report is the durable artifact)

## Setup

Synthetic repo: **5 000 TypeScript files, 37.7 MB** (~8 KB/file: ~9 imports, 15 interfaces,
30 functions each — realistic import locality). Synthetic graph: **250k symbols, 1.0M edges**
(50 syms/file, 4 edges/sym, small-world locality). Hardware: **4-core** Linux container
(conservative vs. developer laptops; numbers below are the second run, warm fs cache — first
run within 15%).

## Measured vs. budget

| Phase | RFC 0001 §5 budget (warm) | Measured | Verdict |
|-------|--------------------------|----------|---------|
| Discovery (stat-scan, no rehash) | 80 ms | 21.5 ms | ✅ 3.7× headroom |
| Extraction (20-file diff, sequential) | 100 ms | 42.1 ms | ✅ 2.4× |
| Graph load (mmap CSR, 5 MB, touch all pages) | 100 ms | **1.1 ms** | ✅ 90× |
| Analyses (recolor via BFS) | 150 ms | 8.6 ms | ✅ 17× |
| **Warm composite** | **500 ms** | **73.3 ms** | ✅ **6.8× headroom** |

Cold path (build-the-cache run): walk 12 + read 68 + blake3 6 + parse-all 3 038 + graph build 8
+ persist 35 + 2×full-BFS 19 ≈ **3.2 s** against the < 10 s budget. ✅

## Key findings

1. **The 500 ms contract is comfortably physical.** Even on 4 cores with everything sequential
   in the warm path, the composite is ~75 ms — the budget survives a 3–4× underestimate in
   extraction realism (see risks) and still holds on weaker hardware.
2. **mmap CSR load is effectively free (1 ms for 1M edges).** ADR 0004's zero-copy bet is
   validated — and even the fallback (bincode deserialize: 5.7 ms) fits the 100 ms line 17×
   over, so the cache format choice is not on the critical path of the promise.
3. **Full reachability is cheap enough that incrementality is a luxury, not a lifeline.**
   A complete 250k-symbol/1M-edge recolor costs 8–15 ms. The dirty-region machinery (RFC 0004
   §5) still matters for extraction (don't re-parse) but analysis-side, the §5 fallback
   ("recompute fully past 30% dirty") could fire on *every* run and stay in budget. This
   de-risks the trickiest code in RFC 0004: analysis incrementality can ship late (or
   partially) without endangering the contract.
4. **Parsing dominates the cold path** (3.0 s of 3.2 s; ~1 650 files/s/4-cores with a full-tree
   walk). Cold time scales linearly with repo size: a 50k-file repo extrapolates to ~32 s cold
   — over the 10 s aspiration at that scale, but cold runs happen once per clone; acceptable,
   and grammar-level extraction (queries instead of naive full walks) has known optimizations.
5. **blake3 hashes the entire 37.7 MB repo in 6 ms** — content addressing costs nothing;
   `--staged` hashing of a few files is unmeasurable.

## Risks the spike does NOT retire

- **Extraction realism:** the proxy visits every tree node; real extraction runs tree-sitter
  *queries* + fact construction, plausibly 2–3× the walk. Even 3× on the 20-file warm diff
  (~125 ms) fits, but M1's conformance fixtures must re-measure with real `FileFacts`.
- **Graph size realism:** 5 MB adjacency vs. a real snapshot with interned strings, spans,
  findings — likely 30–80 MB. mmap load scales with *touched* pages (stays ~free); bincode
  would scale linearly (still fits at ~50 ms). Watch at M2.
- **Resolution cost is unmeasured** (specifier → target over an index). Budgeted inside the
  150 ms analyses line; needs measurement when the resolver exists (M1).
- **Single-core CI runners:** warm composite is nearly sequential already (~75 ms holds);
  cold extrapolates to ~12 s — the M2 benchmark suite should include a `--threads 1` cold run
  to keep this honest.

## Decisions fed back into the docs

- RFC 0001 §5 budget table: **validated, unchanged** (annotated as spike-checked).
- RFC 0004: incremental analysis remains specified, but implementation may land as
  "full recolor always" first (finding 3) — correctness gates (`--no-cache` ≡ cached) unchanged.
- ADR 0004 (rkyv/mmap): confirmed; bincode remains an acceptable fallback for small artifacts.
