# RFC 0017 — Plugin Platform: Second Pass

**Status:** Accepted (design), phased (§8) · **Depends on:** RFC 0003 (plugin system), RFC
0004 (cache), RFC 0013 (incremental patch), RFC 0015 (identity & installation), RFC 0016
(uniform component model) · **Ships:** pre-publication — every phase lands before kndo is
public, because §2's whole argument is that these changes are cheap now and ABI-visible later

## 1. The question this RFC answers

RFC 0016 closed every gap it set out to close, and judged each proposed API extension by one
bar: *a real, landed consumer in this repository*. That bar was right for building the
product — it kept speculative surface off a contract that will freeze at 1.0. It is the wrong
bar for operating a **platform**. kndo will be published; plugin authors will be third
parties whose needs cannot be enumerated by reading this codebase. Three of RFC 0016's own
"decided against, no consumer" outcomes (§7) and one of its honest scope cuts (§6's
patch-path bypass) look different under that criterion, and this RFC re-decides them
deliberately rather than by drift:

1. The **read surface** (`GraphView`) was widened only where a first-party plugin could
   delete code. A platform's read surface must instead be *complete over the graph's stable
   data model* — §2 makes that the design criterion and §5 derives the API from it.
2. The **incremental patch** stayed bypassed for any graph-mutating plugin — acceptable when
   plugins were rare, a standing performance cliff once installing plugins is normal. §3
   removes the bypass structurally, not with a trust-me flag.
3. The **WASM bridge re-instantiates its guest three times per round** — a documented cost
   artifact that already deformed one API (the content budget is keyed by path specifically
   to avoid triple-charging). Changing it after third-party components exist is an
   ABI-visible behavior change; changing it now is an implementation detail. §4 does it now.
4. The **author experience is product surface.** Internally kndo has a compliance suite,
   fixtures, and a dogfood gate; a third-party author has a prose doc. §7 ships the kit and
   turns the ABI compatibility promise from a sentence into a CI job.

`AdapterDescriptor.dependencies` — dormant since RFC 0016 §8 phase 0 — completes the adapter
side's parity under the same platform criterion (§6): a wrapper adapter (a Vue-style superset
language needing its base language's adapter present) is exactly the kind of third-party
composition the platform cannot foresee but must not preclude.

## 2. The platform criterion

**Read APIs are designed by closure over the data model; write APIs stay demand-gated.**
The asymmetry is deliberate and load-bearing:

- A *read* API can at worst return data the graph already holds. Withholding it doesn't
  protect correctness — it just forces plugin authors to re-derive graph facts badly (scan
  paths by hand, re-parse source with regexes) or abandon their idea. Every stable fact the
  graph commits to (paths, names, kinds, package topology, adapter-derived edges) is
  therefore queryable, natively and over WASM, without a per-item demand argument.
- A *write* API extends what a component can make kndo believe, and every extension carries
  the zero-false-positive burden (RFC 0005). Writes stay conservative: the only widening this
  RFC makes (§5.4's file-target edges) comes with an explicit containment rule.

Two contract rules keep the complete read surface correct and free:

**R1 — Plugins read the adapter graph, never each other.** Every query answers from
adapter-derived data (`Provenance::Adapter`) plus the plugin's own inputs; contributions from
other plugins are invisible. This is RFC 0003's existing "plugins cannot consume each other's
output" rule extended to the new queries — without it, results would depend on registration
order and composition, and determinism (identical runs regardless of which other plugins are
installed) would silently break. A plugin sees the same world alone or alongside fifty
others.

**R2 — Indexes are lazy.** Reverse-edge and call-site indexes build on first use, once per
round, O(edges) — a plugin that never queries them costs nothing, and the zero-plugin run
costs exactly what it costs today. No budget applies to graph queries: they touch memory
already loaded, never disk.

## 3. Incremental patch with plugins: strip & re-run — Landed

RFC 0016 §6 folded plugin identity into the snapshot key but left `try_patch` (RFC 0013)
bypassed whenever a graph-mutating plugin is registered, because composing a plugin's hook
output with a partial re-derivation was an unproven claim. The proof turns out not to be
needed — the contributions can be discarded and re-derived instead:

1. **Every plugin contribution is strippable.** Contributed edges carry
   `Provenance::Plugin(id)`; `externally_consumed` is a standalone, wholly plugin-derived
   vector. Neither is entangled with adapter data.
2. **`classify_file` is patch-stable.** It is path-only (`&ProjectPath` + current class, no
   content), and the patch already guarantees the path set is unchanged — so the class
   overrides baked into `FileNode.class` (the one plugin effect with no provenance tag)
   cannot differ from a full rebuild's, *provided the plugin set itself is unchanged*.
3. **The patch path already rebuilds everything a plugin round needs** — the per-file symbol
   name tables, the file index, and it holds the `DiscoveredTree` for `ContentView`.

So: the plugin round factors out of `assemble_from_source` into one function used by both
build paths. `try_patch` strips all `Provenance::Plugin` edges, clears
`externally_consumed`, applies the source patch exactly as today, then re-runs the round
against the patched graph. Plugin hooks are deterministic functions of (graph, discovered
content); both are current post-patch, so the result is byte-identical to a full rebuild —
and RFC 0013's patched ≡ full-rebuild equivalence gate now proves that *with plugins
registered*, mechanically, not by argument.

**One new guard:** the snapshot records the registered plugin set's identity digest (the same
id/version/content-hash fold RFC 0016 §6 put in the graph key), and the patch only proceeds
when it matches the current set — closing the `classify_file` hole in (2): a changed plugin
set means the baked-in class overrides may be stale, so that run full-rebuilds once (which
the snapshot key would have forced anyway).

Cost: patch as today + one plugin round (O(files + symbols) + budget-capped content reads) —
milliseconds. The performance story becomes uniform: a plugin-bearing project pays for its
plugins' hooks, never again for their *presence*.

## 4. One guest instance per round

The plugin bridge instantiates its guest before each hook — three instantiations per plugin
per round. It was the simplest correct thing when zero external plugins existed, and it
already shaped an API: the content budget is keyed by distinct path, not by call,
specifically because the bridge re-fetches the same glob set three times.

This RFC changes the contract to **one instantiation per plugin per round**: the three
graph-mutation hooks run against the same instance, in declaration order
(`contribute-roots`, `contribute-edges`, `annotate-symbols`). What a guest may now observe:

- **State persists across the three hooks of one round.** A guest may compute something in
  `contribute-roots` and reuse it in `contribute-edges`. This is a widening — code written
  for the old contract (stateless request/response, which is what `wit-bindgen` produces by
  default) behaves identically.
- **State never persists across rounds or runs.** The instance is dropped when the round
  ends. No plugin can accumulate cross-run memory; determinism per round is unchanged.

The per-path budget keying stays — it is the right semantics regardless (a component's read
scope shouldn't depend on how many hooks look at the same file) — but its original
motivation dissolves. wasm-abi §5 documents the new lifecycle; the compliance suite grows a
test proving state visibly carries between hooks in one round and visibly resets across
rounds. Doing this *before* publication is the point: today it is an internal change with an
internal test; after third parties ship components, it would be a behavior migration.

## 5. The complete read surface

Derived by closure over what the graph stably holds (§2), exposed natively on `GraphView`
and over WASM as **additive imports with new record types** — existing WIT records are
frozen (growing a record is a breaking change in the component model; adding imports is the
same forward-compatible evolution `read-file` already used, declared in wasm-abi §8).

### 5.1 Files and symbols, completed

`wasm-file-info` (path/role/origin) and `wasm-symbol-info` (name/kind/exported/member-of)
stay as they are. New imports return the rest of what `FileNode`/`SymbolNode` commit to:

```
file-details:   func(path: string) -> option<wasm-file-details>
                  // language, unit, package root (as a path), test-span count
symbol-details: func(path: string, symbol: string) -> option<wasm-symbol-details>
                  // visibility rung name, span
```

Natively these are already reachable (`files()` yields `&FileNode`); the imports close the
WASM gap. Internal IDs stay off the surface everywhere, as always.

### 5.2 Package topology

```
packages:   func() -> list<wasm-package-info>       // manifest path?, name?, root dir
package-of: func(path: string) -> option<wasm-package-info>
```

Natively: `GraphView::packages()` / `package_of(&ProjectPath)` over the graph's
`PackageNode` table (RFC 0011 §3's total ownership — every file maps to exactly one).
First-party proof: `kndo:express`'s app roots *are* the graph's package manifests — its
path scan deletes; `kndo:nextjs` consumes the `package.json` half of its anchors and keeps
its `next.config.*` scan (correctly: a Next config is not a package boundary and core will
not pretend it is).

### 5.3 Edges

```
imports-of:    func(path: string) -> list<string>          // ImportsFile, outgoing
importers-of:  func(path: string) -> list<string>          // ImportsFile, incoming
references-to: func(path: string, symbol: string) -> list<wasm-ref-site>
                 // from-path, from-symbol?, ref-kind, confidence
```

Natively: same three on `GraphView`, backed by R2's lazy reverse index. Answers obey R1:
adapter-provenance edges only. Results are path/name-shaped and sorted (determinism), never
edge indices.

### 5.4 Call-site facts — structured, adapter-extracted

The RFC 0016 §5 detections that stayed out (`res.render` → template edges, route-string →
page edges) stayed out for a *mechanism* reason: reading claimed source through the content
channel is a boundary violation, and — worse — re-scanning source with regexes under a
200-file budget is bad engineering when tree-sitter already parsed every file. The right
mechanism: **adapters extract one generic, ecosystem-blind fact** — call sites whose
argument is a string literal:

```
FileFacts.string_call_args: Vec<(callee_dotted_path, literal, span)>
// e.g. ("res.render", "index", span) · ("app.get", "/users", span) · ("require", "./x", span)
```

The graph persists it; `GraphView::string_call_sites_in(path)` (native) and
`call-sites-in: func(path)` (WASM) expose it. Plugins interpret: `kndo:express` derives
`res.render("x")` → an edge to the matching file under its views directory; `kndo:nextjs`
derives `<Link href>` / `router.push` literals → edges to the matching page file. Zero
re-parsing, no budget interaction, cache-native (facts cache), and the adapter stays
framework-blind — it extracted "a call with a string-literal argument," not "an Express
route". JS-TS implements it first (the consumers' language); the field is optional per
adapter, defaulting empty like `test_spans`. Costs one `GRAPH_SCHEMA_VERSION` bump and a
JS-adapter `facts_schema_version` bump — one clean cache invalidation.

**File-target plugin edges, and the containment rule.** Template/page edges target files,
which the plugin edge sink today rejects (References requires a symbol target). The sink
widens to accept file targets, mapped to a plugin-owned file-to-file edge — and the rule
that keeps the zero-FP bar intact gets written into RFC 0005 alongside it: **plugin edges
are liveness evidence, never architecture evidence.** They feed reachability (rescuing a
file or symbol from `unused` — a false positive here only *suppresses* findings, the safe
direction) and are ignored by `cyclic` and every other analysis that would *create* a
finding from an edge's existence. `cyclic` already consumes only `ImportsFile`; the rule
makes that a contract, not an accident.

### 5.5 What deliberately stays out

The content channel keeps its budget and its outside-the-graph posture — §5.4 removes the
pressure to bend it. `classify_file` still takes no `ContentView` (its once-per-file-across-
all-components execution model hasn't changed, and content-based classification remains
adapter territory via `detected_origin`). And no query exposes another plugin's
contributions (R1) — that boundary is what keeps the platform deterministic.

## 6. Adapter dependencies, evaluated

`AdapterDescriptor.dependencies` (dormant since RFC 0016 §8 phase 0, riding the WIT wire
since §4) gains RFC 0015 §3's exact semantics, mirroring `compose_plugins`:

- **Co-activation fixpoint, global tier only**: an *active* global adapter activates every
  dependency present in the global directory, to any depth. Project-local and compiled-in
  adapters stay unconditional, exactly like plugins.
- **Co-installation**: `kndo plugin install` closes over adapter dependencies the same way
  it closes over plugin dependencies (the probe already carries them; the closure is
  kind-agnostic).
- **Doctor parity**: `ResolvedAdapter` gains `dependencies`; `AdapterResolution` gains
  `missing_dependencies`; `kndo doctor` renders both, mirroring the plugin sections.

The motivating shape is the wrapper adapter — a `.vue`/`.astro`-style superset language whose
own extraction degrades without its base language's adapter present. Proof against real
components, not mocks: a second example crate (`examples/kndo-adapter-wrapper-demo`)
declaring `dependencies = [<the demo adapter's id>]`, exercised by an activation-fixpoint
integration test. **Out of scope, stated:** cross-kind dependencies (a plugin depending on an
adapter or vice versa) — that is a coordinate-namespace unification with its own questions;
nothing here precludes it.

## 7. The author kit, and compatibility as a CI fact

- **`kndo plugin verify <component.wasm>`**: runs the public half of the compliance suite
  locally — loads the component, checks the descriptor (id shape, reserved-namespace
  rejection, activation rules well-formed), drives every hook against a small fixture
  project, and reports what the component contributed. The same checks CI runs on kndo's own
  examples, packaged for an author's inner loop; no wasm toolchain knowledge beyond "build a
  component" required.
- **A starting template**: an authoring skeleton (manifest, WIT bindings, one hook, one
  test) documented in `docs/plugins/authoring.md` — the distance from "idea" to "component
  that passes `verify`" is the ecosystem's activation energy.
- **The compat matrix**: wasm-abi §8 promises a v1 component works against every
  v1-compatible host *indefinitely*. That becomes a CI job: committed, pinned, pre-built v1
  components (the demo adapter and hooks-demo plugin as built today) run against the HEAD
  host on every push. The promise breaks the build instead of the ecosystem.
- **Auditability**: `kndo doctor` reports per-plugin contribution counts (roots, edges,
  annotations) from the last run — the observable half of the threat model, which also gets
  written down explicitly: a malicious component can *lie about graph facts* (at worst
  suppressing findings); it cannot read outside its declared globs, cannot write, cannot
  reach the network.

## 8. Phases

1. **Patch with plugins (§3) — Landed.** `run_plugin_round` factored out of
   `assemble_from_source` and called by both build paths; `try_patch` strips
   `Provenance::Plugin` edges + `externally_consumed`, splices, re-runs the round, and
   refuses on a plugin-set digest mismatch (the digest and a plugin-diagnostics partition
   now live in the snapshot, format v2). Landing this surfaced and fixed a real RFC 0016 §6
   regression: snapshots never persisted `externally_consumed` — safe while no snapshot was
   ever written with plugins registered, silently dropping `annotate_symbols` exemptions
   (RFC 0005 §7) on every warm hit once writes became unconditional. Proven by
   `the_patch_re_derives_plugin_contributions_instead_of_bypassing` (a content-channel
   marker flip, invisible to every adapter guard, must surface through a patch — plus
   byte-equality against a scratch rebuild), `a_changed_plugin_set_refuses_the_patch_and_rebuilds`,
   and `externally_consumed_round_trips_through_the_snapshot`.
2. **Persistent instance (§4)** — behavior contract fixed before any new imports land, so
   §5's additions are born under the final lifecycle.
3. **Read surface (§5)** — call-site facts + packages + edges + details in one cycle: they
   share the schema bump, the WIT additions, and the wasm-abi documentation pass. The
   express/nextjs edge detections land here as the surface's first proof.
4. **Adapter dependencies (§6)**.
5. **Author kit + compat matrix (§7)**.

## 9. Explicitly out of scope

- **Plugin-contributed findings.** The most-demanded future capability — third-party *rules*
  — deliberately gets its own design cycle (RFC 0018, committed to as a draft): it touches
  the zero-false-positive promise itself and must be namespaced (`plugin:<coordinate>/<rule>`),
  severity-channeled, and attributed so it can never dilute the core guarantee. Nothing in
  this RFC's surface blocks it; §5's read completeness is its prerequisite.
- **Cross-kind dependencies** (§6's note).
- **Version constraints / a dependency solver** — RFC 0015 §3's rejection stands until a
  live ecosystem produces real pain.
- **A plugin registry.** Coordinates are already registry-shaped (RFC 0015 §2); GitHub
  releases remain the only channel for now.
