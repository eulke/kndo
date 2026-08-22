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
directory; `Plugin`s (not `LanguageAdapter`s) also auto-discover from a global, per-machine
directory, filtered by activation rules rather than unconditional — §5.5. **One directory, two
loaders, no naming convention**: every discovered `.wasm` file is tried against both
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

## 5. The Plugin ABI (`kndo:plugin`)

### 5.1 The WIT world

`crates/kndo-plugin-api/wit/plugin.wit`, package `kndo:plugin@0.1.0`, world `plugin`:

```
import list-files: func() -> list<wasm-file-info>;
import symbols-in: func(path: string) -> list<wasm-symbol-info>;

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

### 5.2 v1 scope cuts, and why

- **No `ingest_coverage`/`suppress`.** Neither is wired to any analysis yet on the *native*
  `Plugin` trait either (RFC 0003 §2's "landed" note) — nothing to bridge until they're real.
- **`GraphView` exposes `files()`/`symbols_in()` only, not the full `ProjectGraph`.**
  `wasm-file-info` carries `path`/`role`/`origin`; `wasm-symbol-info` carries `name`/`kind`/
  `exported`/`member-of`. `language`, `unit`, `test-spans`, and every edge-level fact are not
  surfaced — the same "conservative v1, grow on real demand" cut the adapter ABI's descriptor
  makes, not a structural limit of the bidirectional design.
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
run with a real `&GraphView<'_>` borrowed for the duration of one `assemble_from_source` call
(graph.rs, RFC 0003 §2's "landed" note); `wasmtime::Store`'s state type must be `'static`, so a
live borrow can't sit inside it directly. `WasmPlugin` resolves this by cloning exactly what
`list-files`/`symbols-in` can answer (`HostViewData`, built once per graph-mutation round, not
once per query) into the store's state rather than reaching for raw-pointer plumbing across the
FFI boundary — a WASM plugin already forces a full graph rebuild every run (§5.4), so one more
bounded `O(files + symbols)` clone alongside that full rebuild is proportionally small, and the
resulting code has no `unsafe`.

**Fuel budget and sandbox** are the same posture and the same constant class as §3's adapter
bridge (`FUEL_PER_CALL` in `plugin_host.rs`): an exhausted or trapped hook degrades to "this
plugin contributed nothing this round," never a crashed `kndo check`; no WASI linked, so a
component declaring one fails to instantiate rather than silently receiving capabilities.

### 5.4 Correctness: cache and patch bypass

Same rule as the native `Plugin`'s own graph-mutation hooks (contracts/core-traits.md §3): any
registered plugin — WASM or built-in — that declares `mutates_graph()` (a `kndo:plugin`
component always does: the world exports all four hooks, so `WasmPlugin` keeps the trait's
`true` default) makes `assemble_from_source` skip both the graph-snapshot cache hit and the
incremental patch, full-rebuilding every run. Neither reuse path re-invokes a plugin's hooks
(WASM or native), so serving either to a plugin-bearing project would silently miss whatever
the plugin contributes. Coverage-only plugins (`LcovPlugin`) declare `false` and leave both
fast paths intact.

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
This whole mechanism is `Plugin`-only today: `LanguageAdapter` has no `activation` field, so a
globally installed adapter isn't something this pass adds (RFC 0003 §3/§4).

`kndo doctor` (`crates/kndo-cli/src/main.rs`'s `doctor_cmd`) reports both sides: `report.plugins`
(from `Engine::doctor`) for the final composed set, and `kndo::global_plugin_candidates(root)`
— a separate call, since `Engine` itself never sees a candidate that didn't activate — for
*every* `.wasm` file the global directory holds, each with `activated: bool` and its
`activation` rules rendered via `ActivationRule::describe`. A globally installed plugin whose
rule doesn't match isn't invisible; it shows up as inactive with the rule that didn't fire.

Not yet built: an install/registry command (`kndo plugin install …`) — getting a `.wasm` file
into the global directory is still a manual copy.

## 6. Producing a component

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

Four suites, all building their demo component fresh from source and componentizing it
in-process on every run (no binary checked into the repo):

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

## 8. Versioning

Each WIT package version (`kndo:adapter@0.1.0`, `kndo:plugin@0.1.0`) and the corresponding
section of this document change together, independently of each other (§0). A breaking v2 of
either package (the adapter side's `resolve()` host-import callbacks or byte-content; the
plugin side's richer `GraphView` surface, `ingest_coverage`/`suppress`, or per-query fuel) is a
new package version, not a silent reinterpretation of `0.1.0` — a component built against a v1
package must keep working against a v1-compatible host indefinitely.
