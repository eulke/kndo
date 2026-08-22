# Contract — `kndo-plugin-api` (WASM Component-Model ABI)

**Status:** Accepted, both v1s shipped · Normative for the WASM tier of ADR 0003 and RFC 0003
§§2–3. Code must match this document; changing either requires updating both in the same PR.

## 0. What this is

ADR 0003 splits extensions into two tiers: first-party adapters/plugins compiled into the
`kndo` binary, and third-party ones shipped as WASM components against a versioned ABI —
`kndo-plugin-api`. **Two independently-versioned WIT packages live under that one crate**, one
per native trait: `kndo:adapter@0.1.0` bridges `LanguageAdapter` (§§1–4 below),
`kndo:plugin@0.1.0` bridges `Plugin`'s four graph-mutation hooks (§5 below). Independent
versioning is deliberate (contracts/core-traits.md §6: "WASM ABI versioned independently") —
a breaking change to one package's shape never forces a lockstep bump of the other, and the
small vocabulary overlap between them (`file-class`, `root-kind`, `ref-kind`, `confidence`) is
duplicated rather than shared for the same reason.

Both packages cover only their v1 scope — real, working, and deliberately smaller than the
native trait's full surface; §2 and §5.2 each list their own cuts and why. `Plugin`'s
`ingest_coverage`/`suppress` hooks are not bridged by either package yet.

## 1. The adapter WIT world

`crates/kndo-plugin-api/wit/adapter.wit`, package `kndo:adapter@0.1.0`, world `adapter`:

```
export descriptor: func() -> adapter-descriptor;
export claim: func(path: string) -> option<file-claim>;
export extract: func(path: string, content: string) -> file-facts;
```

`adapter-descriptor`, `file-claim`, and `file-facts` are v1-scoped mirrors of the native
`AdapterDescriptor`/`FileClaim`/`FileFacts` (contracts/core-traits.md §2) — see the WIT file's
own doc comments for the field-by-field mapping and what each omission means. The three
functions are the whole world: **no host-import callbacks exist in v1** — a component never
calls back into the host. That is what lets the reference guest
(`examples/kndo-plugin-demo`) target plain `wasm32-unknown-unknown` with zero WASI: there is
nothing for it to import, so there is no ambient fs/net surface to sandbox *away* — the
target itself has none.

## 2. v1 scope cuts, and why

Every cut below is the same shape of decision this project makes elsewhere (CSS's deferred
selector extraction, JSON's non-source-language non-goals): ship the honestly-smaller thing
that's fully correct, rather than a bigger thing with a hidden gap.

- **No `claim_manifest`/`extract_manifest`/`resolve`.** The host bridge (`WasmAdapter`)
  answers all three itself without ever calling the guest: `claim_manifest` is always
  `false`, `extract_manifest` always returns `ManifestFacts::default()`, `resolve` always
  returns `Resolution::Unresolved` — the exact posture the JSON and CSS adapters already
  document for their own non-applicable trait methods (docs/adapters/json.md, css.md). A v1
  external adapter therefore has no manifest, no dependency graph, and no cross-file import
  resolution; `unused`/`test-only`/`untested` are real for it (declarations + references +
  roots is exactly what reachability consumes), but `cyclic`, `deep-import`, and dependency
  hygiene see nothing.
- **No `ResolveCtx` host-import callbacks.** `resolve()`'s real job needs `ResolveCtx`'s
  querying API (`contains`, `workspace_member`, `unit_files`, `files_in_dir`, `files_under` —
  contracts/core-traits.md §2), which only makes sense as **host-import** functions a
  component calls back into — the opposite data-flow direction from everything else in v1.
  Adding it is what a v2 needs to make `resolve()` real; deliberately deferred until an
  external adapter actually wants cross-file resolution (the same "don't build the
  mechanism before the demand" call RFC 0003 §6 makes for custom analyses).
- **No visibility ladder, no cycle policy, no `resolves_dependency_usage`.** The host fills
  in the same safe defaults CSS/JSON already use for a language with no such semantics: an
  empty visibility ladder (every declaration reports the widest level — the ladder's own
  conservative-mapping rule, contracts/core-traits.md §2), `Idiomatic` cycle tolerance at
  both levels, `resolves_dependency_usage: false`. A v1 external adapter is exempt from
  `internal-only`/`private-type-leak` (empty ladder ⇒ those analyses skip its files
  entirely, same rule as CSS/JSON) rather than risk a wrong ladder guess.
- **UTF-8 text content, not raw bytes.** `extract`'s `content` parameter is a WIT `string`
  (valid UTF-8 by construction), not `list<u8>` — simpler for v1, at the cost of an adapter
  for a language with non-UTF-8-safe source files not being expressible yet. Every launch
  language's grammar already assumes UTF-8 source in practice, so this has cost nothing so
  far.
- **`SymbolKind::Other(name)` isn't representable.** An adapter-specific facet (Rust's
  `"macro"`, Go's `"type"`) has nowhere to go in the v1 enum; a WASM adapter needing one
  today folds it into the nearest listed kind.

None of these are silent: every one is enforced by the host bridge never calling the guest
for the corresponding native method (§3), not by a guest-side promise the host has to trust.

## 3. The host bridge (`crates/kndo-plugin-api`)

`WasmAdapter::load(path: &Path) -> Result<WasmAdapter, LoadError>` loads an **already
componentized** `.wasm` file (component-model binary — see §6 for how one gets produced) and
returns a value implementing `kndo_core::adapter::LanguageAdapter` directly. From the
`Engine`'s side this is indistinguishable from a compiled-in adapter (ADR 0003: "the WASM ABI
is a generated bridge over [the native traits]") — it goes on the very same
`Vec<Box<dyn LanguageAdapter>>` `default_adapters()` returns.

**Fuel budget (RFC 0003 §3).** Every guest call runs under a fixed fuel allowance
(`FUEL_PER_CALL` in `host.rs`); a call that exhausts it or traps is caught and converted to a
conservative empty result — `None` from `claim`, or `FileFacts::default()` plus a `Warn`
diagnostic from `extract` — never a crashed `kndo check`. One misbehaving external adapter
degrades to silence for its own files, not a broken run for every other language in the
project. There is no wall-clock timeout in v1 (fuel is a deterministic proxy for it, same
spirit, cheaper to implement soundly); a real wall-clock epoch-deadline layer is future work
if fuel alone proves an insufficient proxy in practice.

**Sandbox.** No WASI is linked into the host's `Linker` at all — v1's world has no imports to
satisfy, so there is nothing to grant. This is stronger than a policy promise: a component
that somehow declared a WASI import would fail to *instantiate*, not silently receive
capabilities nobody meant to give it.

## 4. Discovery (`kndo::open`, RFC 0003 §3)

The distribution crate (`crates/kndo/src/lib.rs`) auto-discovers `.kndo/plugins/*.wasm`
relative to the project root on every `kndo::open` call — no `kndo.toml` entry needed, the
zero-config default RFC 0003 §3 already names. This section covers that project-local
directory; both `Plugin`s and (since RFC 0016 §4) `LanguageAdapter`s also auto-discover from a
global, per-machine directory, filtered by activation rules rather than unconditional — §4.1
for adapters, §5.5 for plugins. **One directory, two loaders, no naming convention**: every
discovered `.wasm` file is tried against both
`WasmAdapter::load` and
`WasmPlugin::load`; each fails to *instantiate* (not merely "doesn't look right") against a
component built for the other package's world, since wasmtime's own component type-checking
requires every world-declared export to be present with matching types. A component that
fails to load either way is skipped, not fatal to the run (§3's/§5.3's "one bad extension
doesn't take down the rest" posture, applied at load time as well as call time).
`kndo_plugin_api::WasmAdapter`/`WasmPlugin` never guess which ABI a `.wasm` file targets by its
name, path, or a magic byte prefix — the type system already answers that, so nothing else
needs to. Both loaders are feature-gated together (`external-adapters`, on by default) so an
embedder building a minimal static binary can drop the WASM runtime entirely
(`--no-default-features --features js,go,...`, ADR 0006).

Demo components shipped **in this repository** live outside the compiled product on purpose
(`examples/kndo-plugin-demo`, `examples/kndo-plugin-hooks-demo` — both excluded from the
workspace's own `members`, same convention as `spikes/perf`): "third-party" means never
statically linked, checked by keeping it structurally incapable of being one.

### 4.1 Identity, global installation & activation (RFC 0016 §4)

`WasmAdapter::load` rejects any component whose descriptor claims a `kndo:`-prefixed id
(`host.rs`, mirroring §5.1's identity binding for plugins) — the reserved namespace is not
claimable by an external component, full stop, independent of what any first-party adapter's
own id happens to be (none of them use the `kndo:` prefix; renaming them would only churn the
graph cache key — RFC 0016 §4's own note on why that's not worth doing).

Beyond `.kndo/plugins/`, `crates/kndo/src/lib.rs`'s `compose_adapters` also scans the same
**global** directory the plugin tier uses (§5.5 — `dirs::data_dir()/kndo/plugins`,
`KNDO_PLUGIN_DIR`-overridable): each candidate's `descriptor().activation` is evaluated against
the project root before it joins composition, reusing the exact `activation::activates`/
`ActivationRule` machinery §5.5 documents for plugins — `file-exists(glob)`/
`manifest-dependency(name)`, any single match activates, an empty list never self-activates
globally. Project-local and compiled-in adapters are unconditional either way, same as their
plugin-tier counterparts. Since RFC 0017 §6, `descriptor().dependencies` participates too:
an *active* adapter (any tier) activates every global candidate it names, transitively —
the same co-activation fixpoint the plugin tier runs, shared as one generic implementation
over kind-neutral candidate identities, and reported the same way (`missing_dependencies`
on `kndo::adapter_resolution`, reason-aware status — "active (dependency of X)" — on each
global candidate). `examples/kndo-adapter-wrapper-demo` is the reference wrapper adapter
proving the chain against real components.

**Claim priority.** With project-local, global, and compiled-in adapters all in play for the
same file extension, composition orders the final `Vec<Box<dyn LanguageAdapter>>` project-local
first, then active global candidates, then compiled-in — ties within a tier broken by
descriptor id — because `graph.rs`'s claim resolution takes the first adapter in that list
whose `claim()` returns `Some`. Auditing this while implementing it found the *actual*
pre-existing order was the reverse (compiled-in first, externals appended after): a
project-local adapter could never have won a contested extension against a built-in one. That
is corrected, not merely documented, by RFC 0016 §4.

`kndo::adapter_resolution`/`kndo::global_adapter_candidates` mirror `plugin_resolution`/
`global_plugin_candidates` (§5.5) exactly — `kndo doctor` renders both the composed set with
each adapter's `activation` rules shown, and a "global adapter candidates" section listing
every `.wasm` the global directory holds, activated or not.

`kndo plugin install <coordinate>` (RFC 0015 §4, `kndo::plugin_install`) accepts adapter
components too: `wasm_probe` tries the plugin loader, then the adapter loader, and whichever
accepts the bytes carries the descriptor identity binding checks against. No installer-side
distinction between the two kinds beyond that — checksum, identity, dependency closure, and
`plugins.lock` are all kind-agnostic.

## 5. The Plugin ABI (`kndo:plugin`)

### 5.1 The WIT world

`crates/kndo-plugin-api/wit/plugin.wit`, package `kndo:plugin@0.1.0`, world `plugin`:

```
import list-files: func() -> list<wasm-file-info>;
import symbols-in: func(path: string) -> list<wasm-symbol-info>;
import read-file: func(path: string) -> option<list<u8>>;

export descriptor: func() -> plugin-descriptor;
export classify-file: func(path: string, current: file-class) -> option<file-class>;
export contribute-roots: func() -> list<contributed-root>;
export contribute-edges: func() -> list<contributed-edge>;
export annotate-symbols: func() -> list<plugin-target>;
```

Unlike the adapter world, this one is **bidirectional** — `contribute-roots`/`contribute-
edges`/`annotate-symbols` need to *read* the graph, not just report facts about one file. Two
narrow host-import queries (`list-files`, `symbols-in`) mirror `kndo_core::plugin::GraphView`'s
own two methods exactly, rather than serializing the whole graph into every call: a guest only
pays for what it actually queries. The three "write" hooks return a `list<...>` of their
contributions in one call, the WASM analogue of filling `RootSink`/`EdgeSink`/`AnnotationSink`
via repeated `add()` calls collapsed into a single call-boundary crossing — cheaper, and it
keeps the imperative sink shape out of the wire format entirely. `classify-file` needs no
queries of its own (it only ever sees the one file it's asked about, mirroring the native
hook's own contract) and is called against a lightweight, view-less instance.

Every target is named, never addressed by an internal id — `plugin-target { path, symbol:
option<string> }`, same as `kndo_core::plugin::PluginTarget`; resolved host-side against the
same bare/qualified lookup tables `RawRoot`/`RawReference` resolve against, and an unresolvable
target is dropped silently (the same miss behavior the adapter ABI and the native `Plugin`
trait both already have).

`plugin-descriptor` also carries `activation: list<activation-rule>` — `variant activation-rule
{ file-exists(string), manifest-dependency(string) }`, the machine-checkable counterpart to
`detection`'s human-readable prose. `descriptor()` is the *only* call the host makes before
deciding whether a globally installed plugin even joins composition (§5.5); a project-local
`.kndo/plugins/*.wasm` file never has this field consulted at all.

Two RFC 0015 fields ride the same record: `id` is the plugin's *coordinate* (its fetchable
source, `github.com/<owner>/<repo>`; the `kndo:` namespace is reserved for built-ins, and the
host **fails the load** of any external component claiming it — same skipped-not-fatal handling
as an instantiation error), and `dependencies: list<string>` names coordinates of plugins whose
conventions are part of this one's (install closure + activation implication, RFC 0015 §3 —
never versions, ordering, or data flow).

**`read-file` (RFC 0016 §5's content channel, landed).** Scoped to `requested-file-access`:
the host prefetches every discovered path matching the descriptor's declared globs, budget-
charged and byte-read through the run's `ContentView` (`kndo_core::plugin::ContentView`) exactly
as a native plugin's own `.read()` calls would be, *before* instantiating each round's guest —
the guest can't make a host round-trip of its own choosing mid-call, so `read-file` on the guest
side is a lookup into that owned snapshot, not a live filesystem call. Budget accounting is
keyed by path, not by call: a component's read scope shouldn't depend on how many hooks look at
the same file — a path already charged is served again for free within the round. (This keying
predates RFC 0017 §4's one-instance-per-round lifecycle, §5.3, which removed its original
triple-charge motivation; it stays because it is the right semantics regardless.) A path outside
the declared globs, or one the budget has cut off, comes back `none` — the same silent-miss
shape every other host-mediated lookup in this ABI already has.

### 5.2 v1 scope cuts, and why

- **No `ingest_coverage`/`suppress`.** `ingest_coverage` isn't wired on the native `Plugin`
  trait either — nothing to bridge until it's real. `suppress` went further: RFC 0016 §7
  evaluated it against real shipped components and decided cut, not merely deferred (RFC 0003
  §2) — it stays undeclared on both the native trait and this WIT package.
- **The frozen v1 records stay frozen; the read surface grew by imports instead (RFC 0017
  §5).** `wasm-file-info` (path/role/origin) and `wasm-symbol-info`
  (name/kind/exported/member-of) never gain fields — growing a record is a breaking change in
  the component model. Everything else the graph stably holds arrives through the additive
  imports `packages`/`package-of`, `file-details`/`symbol-details`,
  `imports-of`/`importers-of`/`references-to`, and `call-sites-in` (each with its own new
  record type — `wasm-package-info`, `wasm-file-details`, `wasm-symbol-details`,
  `wasm-ref-site`, `wasm-call-site`, `wasm-span`). All answer from the same
  pre-instantiation snapshot as `list-files`/`symbols-in`, sorted and deterministic, and from
  **adapter-derived data only** (RFC 0017 §2's rule R1): no plugin ever observes another
  plugin's contributions, which is what keeps runs identical across plugin compositions. The
  snapshot clone grows accordingly — bounded `O(files + symbols + edges + content bytes)`
  per round.
- **`SymbolKind::Other(name)`/`CssRule`/`CssVariable` aren't representable** — same cut as
  §2's adapter-side one; the host bridge folds them into `variable` rather than fabricate a
  wire value.
- **No fuel-budget layer around individual host-import calls** — the *whole* hook call
  (guest logic plus every `list-files`/`symbols-in` round trip inside it) shares one fuel
  allowance, refilled per hook. A guest that queries in a tight loop pays for it out of the
  same budget its own logic does; there is no separate per-query cap.

None of these are silent: every one is enforced by what the host bridge (`plugin_host.rs`)
does and doesn't call or expose, not by a guest-side promise the host has to trust.

### 5.3 The host bridge

`WasmPlugin::load(path: &Path) -> Result<WasmPlugin, LoadError>` loads an already-componentized
`.wasm` file and returns a value implementing `kndo_core::plugin::Plugin` directly — same
"generated bridge" posture as `WasmAdapter` (ADR 0003), on the same `Vec<Box<dyn Plugin>>`
`default_plugins()`/`Engine::open_with_plugins` accept.

**Host state and the borrow problem.** `contribute_roots`/`contribute_edges`/`annotate_symbols`
run with a real `&GraphView<'_>` (and, since RFC 0016 §5, a real `&ContentView<'_>`) borrowed
for the duration of one `assemble_from_source` call (graph.rs, RFC 0003 §2's "landed" note);
`wasmtime::Store`'s state type must be `'static`, so a live borrow can't sit inside it directly.
`WasmPlugin` resolves this by cloning exactly what `list-files`/`symbols-in` can answer, plus
every content-channel path the descriptor's globs match (`HostViewData`, built once per
graph-mutation round, not once per query), into the store's state rather than reaching for
raw-pointer plumbing across the FFI boundary — a bounded `O(files + symbols + content bytes)`
clone, once per round, and the resulting code has no `unsafe`.

**Guest lifecycle (RFC 0017 §4): one instance per graph-mutation round.** The bridge
instantiates the component when `contribute-roots` — the round's first hook in the world's
declaration order — is invoked; `contribute-edges` and `annotate-symbols` run against that
same instance, and it is dropped when `annotate-symbols` returns. Two consequences a guest
author may rely on, and one it must never rely on: guest state (statics, lazily built caches)
*persists across the three hooks of one round* — compute something in `contribute-roots`,
reuse it in `contribute-edges`; guest state *never survives into the next round or run* — the
drop is unconditional, success or trap; and a hook invoked out of order by a non-core host
gets a defensively fresh instance rather than another round's state. Stateless
request/response guests (what `wit-bindgen` produces by default) behave identically under
either lifecycle. Proven observable by the compliance suite's `staged_`/`fresh_` scenarios
against `examples/kndo-plugin-hooks-demo`.

**Fuel budget and sandbox** are the same posture and the same constant class as §3's adapter
bridge (`FUEL_PER_CALL` in `plugin_host.rs`), re-armed before *every* hook call — the per-call
budget semantics are unchanged by the shared instance; a heavy `contribute-roots` can't starve
`annotate-symbols`. An exhausted or trapped hook degrades to "this plugin contributed nothing
this round," never a crashed `kndo check`; no WASI linked, so a component declaring one fails
to instantiate rather than silently receiving capabilities.

### 5.4 Correctness: cache and patch bypass

Same rule as the native `Plugin`'s own graph-mutation hooks (contracts/core-traits.md §3): any
registered plugin — WASM or built-in — that declares `mutates_graph()` (a `kndo:plugin`
component always does: the world exports all four hooks, so `WasmPlugin` keeps the trait's
`true` default) participates in `assemble_from_source`'s cache-key folding (RFC 0016 §6).
Coverage-only plugins (`LcovPlugin`) declare `false` and were never part of either bypass.

**The snapshot cache is reusable, the incremental patch is not — landed asymmetrically, on
purpose.** `Plugin::content_hash()` (`WasmPlugin` overrides it to the blake3 hash of its own
component bytes, computed once at `load()`; a native plugin's default `None` relies on
`PluginDescriptor.version` as its trust boundary, same discipline `AdapterDescriptor
.facts_schema_version` already established) folds into the graph cache key alongside every
discovered file's content hash. A `ContentView` never answers a path outside that same
discovered set (§5.1), so any input a plugin's hooks — including its content-channel reads —
could react to was already part of the key. That makes the graph-snapshot fast path safe: a
snapshot written under one plugin's identity can only ever match a run with the identical
component (bytes and all, for WASM) over identical inputs. The incremental patch (RFC 0017
§3) covers the other fast path without needing the key-folding argument at all: every plugin
contribution is provenance-tagged, so the patch strips them, splices the source change, and
re-runs the full plugin round against the patched graph — byte-identical to a full rebuild by
the equivalence gate, guarded by a snapshot-stored plugin-set digest (a changed set
full-rebuilds once). Nothing a plugin contributes ever rides either fast path unrevised.

### 5.5 Global installation & activation (RFC 0003 §4)

Beyond project-local `.kndo/plugins/`, `crates/kndo/src/lib.rs`'s `activation` module also scans
a **global** directory — `dirs::data_dir()/kndo/plugins` (XDG data dir on Linux, Application
Support on macOS, `%APPDATA%` on Windows), overridable wholesale via the `KNDO_PLUGIN_DIR`
env var. This directory is not tied to any one project, so presence there can't be the opt-in
signal `.kndo/plugins/` gets to use — each candidate's `descriptor().activation` is evaluated
against the project root *before* the plugin joins composition at all:

- `file-exists(glob)` — at least one file under the project root matches (`glob` crate
  semantics, evaluated once at `kndo::open` time, not per-analysis-run).
- `manifest-dependency(name)` — any `package.json`/`Cargo.toml` under the project root declares
  a dependency by this name in any dependency section, not just the root's own
  (`kndo_core::discovery::find_files_named` — the same gitignore-aware walker `discover` itself
  uses, so `node_modules` etc. are excluded exactly like everywhere else in the product; Cargo's
  `-`/`_` interchangeability is honored). Root-only would have made every monorepo package a
  false negative for a dependency only *it* declares — not an acceptable v1 cut, since kndo's
  monorepo awareness is a first-class feature everywhere else (RFC 0012 §8/§10).

Any single matching rule activates the plugin; an **empty** `activation` list never
self-activates from the global directory (silence over a guess, the zero-false-positive
default) — such a plugin only ever runs if placed in a project's own `.kndo/plugins/` instead.
`LanguageAdapter` shares this exact mechanism since RFC 0016 §4 — §4.1 covers the adapter-side
specifics (identity, claim priority) this section doesn't repeat.

`kndo doctor` (`crates/kndo-cli/src/main.rs`'s `doctor_cmd`) reports both sides: `report.plugins`
(from `Engine::doctor`) for the final composed set, and `kndo::global_plugin_candidates(root)`
— a separate call, since `Engine` itself never sees a candidate that didn't activate — for
*every* `.wasm` file the global directory holds, each with `activated: bool` and its
`activation` rules rendered via `ActivationRule::describe`. A globally installed plugin whose
rule doesn't match isn't invisible; it shows up as inactive with the rule that didn't fire.

`kndo plugin install <coordinate>` (RFC 0015 §4, `kndo::plugin_install`) populates the global
directory from GitHub releases — checksum-verified, identity-bound (the fetched component's
descriptor id must equal the coordinate), dependency-closed, recorded in `plugins.lock` beside
the `.wasm` files. Hand-copying a file in still works and is still the project-local tier's
only mechanism; `kndo plugin list` shows such files as hand-installed rather than hiding them.

## 6. Producing a component

(The full author-facing walkthrough — project setup, descriptor fields, testing shape,
versioning/maintenance — is [docs/plugins/authoring.md](../plugins/authoring.md); this section
is only the componentization mechanics.)

A third-party author needs a real component-model `.wasm` binary, not a plain core module.
Two ways, both documented rather than assumed, for either package:

- `cargo component build` (the `cargo-component` tool) — the ecosystem-standard path.
- The `wit-component` crate directly, as a library, with **no extra tool install** —
  `wit_component::ComponentEncoder::default().module(&core_wasm_bytes)?.encode()?`. This is
  exactly what `kndo-plugin-api`'s own compliance tests do to build both
  `examples/kndo-plugin-demo` and `examples/kndo-plugin-hooks-demo` fresh on every run — it
  works with zero WASI imports to satisfy (§2/§3, §5.2/§5.3), which is true of any
  v1-conformant adapter or plugin by construction.

## 7. Compliance

The suites below build their demo component fresh from source and componentize it in-process
on every run — testing today's guest source against today's host. The one deliberate
exception is the compat matrix (last entry), whose whole point is *committed, pinned* binary
components:

- `crates/kndo-plugin-api/tests/compliance.rs` — drives a `WasmAdapter` directly against a
  hand-built `Engine`.
- `crates/kndo-plugin-api/tests/plugin_compliance.rs` — drives a `WasmPlugin` directly against
  a hand-built `Engine` and its own minimal `LanguageAdapter`, exercising all four hooks
  (including the `list-files`/`symbols-in` round trip) with a baseline run proving the
  assertions aren't vacuous; also proves the two ABIs reject each other's components
  (`each_abi_rejects_a_component_built_for_the_other`) — the mechanism §4's discovery design
  depends on.
- `crates/kndo/tests/external_adapter.rs` and `crates/kndo/tests/external_plugin.rs` — go
  through the full product composition (`kndo::open`, `.kndo/plugins/` discovery included), the
  same code path `kndo-cli` uses for every command; `external_plugin.rs` drops *both* an
  adapter and a plugin component into the same `.kndo/plugins/` directory, proving §4's
  single-directory sort actually works end to end, not just at the loader level.
- `crates/kndo/tests/global_plugin_activation.rs` — same full-product composition, but through
  `KNDO_PLUGIN_DIR` (§5.5): one `#[test]` opens two temp projects against the same globally
  installed plugin — one without, one with the file that satisfies its `file-exists` rule —
  proving activation is genuinely conditional, not just wired and always-on.
- `crates/kndo/tests/global_adapter_activation.rs` (RFC 0016 §4) — the adapter-side mirror of
  the above, plus a claim-priority assertion: with the same component placed both project-local
  and in the global tier for one project, `kndo::adapter_resolution` must list the project-local
  copy first — proving §4.1's corrected composition order, not just that both tiers activate.
- `crates/kndo/tests/plugin_install_probe.rs` (RFC 0015 §4, extended by RFC 0016 §4) — a real
  component through `kndo::plugin_install::wasm_probe`; one case per kind proves the probe's
  plugin-then-adapter fallback reaches identity binding for both, not just plugins.
- `crates/kndo/tests/adapter_dependency_implication.rs` (RFC 0017 §6) — two real components
  in the global tier; satisfying only the wrapper's activation rule must activate the adapter
  it depends on (`ImpliedBy`), all the way to that adapter's findings actually firing.
- `crates/kndo-plugin-api/tests/compat_matrix.rs` (RFC 0017 §7) — the ABI compatibility
  matrix: the two reference components **pre-built and committed** under `tests/compat/`,
  loaded and hook-driven against the HEAD host with no wasm toolchain in the loop. This is
  §8's "a v1 component keeps working indefinitely" promise as a build-breaking CI job (its
  own named job in `ci.yml`, plus the ordinary workspace test run). Pre-1.0, a WIT change
  that breaks the pinned binaries is legal (authoring.md §7) — the rebuild of `tests/compat/`
  in the same commit is the explicit, reviewable record that a break happened.

`kndo plugin verify <component.wasm>` (RFC 0017 §7) packages the public half of this for
plugin authors: the exact discovery loaders, a descriptor report with lint-grade warnings,
and a real fixture-project check reporting what the component contributed.

## 8. Versioning

Each WIT package version (`kndo:adapter@0.1.0`, `kndo:plugin@0.1.0`) and the corresponding
section of this document change together, independently of each other (§0). A breaking v2 of
either package (the adapter side's `resolve()` host-import callbacks or byte-content; the
plugin side's `ingest_coverage`, or per-query fuel) is a new package version, not a silent
reinterpretation of `0.1.0` — a component built against a v1 package must keep working
against a v1-compatible host indefinitely. (The "richer `GraphView` surface" this paragraph
once listed as a breaking-v2 example turned out not to need one: RFC 0017 §5 grew it entirely
through additive imports with new record types — §5.2 above — the same evolution shape as
`read-file`.)

**Both RFC 0016 §8 phase 0 reservations are now landed**, additively, exactly as reserved:

- **`kndo:plugin`'s `read-file` host import (RFC 0016 §5).** One added import,
  `read-file(path) → option<list<u8>>` (§5.1/§5.3 above). A component built against the
  pre-§5 world simply never calls it, and the host still answers every existing import
  identically.
- **`kndo:adapter`'s component-descriptor fields (RFC 0016 §4).** The `adapter-descriptor`
  record gained `activation: list<activation-rule>` (wired and read — §4.1) and
  `dependencies: list<string>` (initially riding the wire unevaluated; RFC 0017 §6 later
  gave it RFC 0015 §3's exact co-activation semantics in the global tier, through the same
  fixpoint plugins use — the wire shape never changed). No `version` field landed — §4.1's
  own note explains why one was never needed.
  A component built against the pre-§4 world has neither field; the host reads them as empty,
  the same value the dormant reservation always implied.

Neither changed a byte of previously shipped behavior — both are the freeze committing to an
evolution *path* it had already declared, landing on schedule.

## 9. Threat model

Written down explicitly (RFC 0017 §7) because the tool is published and components come from
anywhere. What a malicious or buggy component **cannot** do, by construction:

- **Read outside its grant.** No filesystem, no environment, no clocks, no network: the WASM
  sandbox has no WASI world at all — every byte a component sees arrives through a host
  import. The content channel (RFC 0016 §5) serves only files matching the component's own
  declared `requested_file_access` globs, from the already-discovered, gitignore-filtered
  tree, under a per-round byte budget whose cutoff is surfaced as a diagnostic.
- **Write anything.** There is no write-shaped import. Hook outputs are *claims about the
  graph*, applied by the host under the sink vocabulary (§5.1) — no new node/edge kinds, no
  finding creation, no file mutation.
- **Hang or exhaust the host.** Every hook call runs under a wasmtime fuel budget, re-armed
  per call (RFC 0017 §4); an exhausted or trapping call is dropped like any other component
  error — skipped, never fatal to the run.
- **Impersonate.** Reserved-namespace ids fail the load (§4.1/§5.5); the installer's identity
  binding refuses a component whose descriptor id differs from the coordinate it was fetched
  from, and the lockfile pins the checksum (RFC 0015 §4).

What a malicious component **can** do — the residual risk, stated honestly: **lie about graph
facts.** A false root, edge, annotation, or `classify_file` override suppresses findings that
should have fired (it cannot *create* false findings: plugin evidence is liveness-only, RFC
0005 §1, and file-target edges are consumed by reachability alone). The mitigations are
visibility, not prevention: contributions are provenance-tagged in the graph, and `kndo
doctor` reports the per-plugin audit record from the last run — id, roots, edges, annotations
(`plugin contributions (last recorded run)`), so "this plugin exempted 400 symbols" is a
line in a report, not an invisible bias. Installing a component remains a trust decision at
exactly that scope: the worst case is quieter output, never exfiltration or code execution.
