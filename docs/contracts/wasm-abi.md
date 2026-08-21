# Contract — `kndo-plugin-api` (WASM Component-Model ABI)

**Status:** Accepted, v1 shipped · Normative for the WASM tier of ADR 0003 and RFC 0003 §3.
Code must match this document; changing either requires updating both in the same PR.

## 0. What this is

ADR 0003 splits extensions into two tiers: first-party adapters/plugins compiled into the
`kndo` binary, and third-party ones shipped as WASM components against a versioned ABI —
`kndo-plugin-api`. This document is that ABI's concrete shape for **v1**: what a component
implements, what the host (`crates/kndo-plugin-api`) guarantees around it, and what is
deliberately out of scope.

v1 covers **adapters only** (`LanguageAdapter`, contracts/core-traits.md §2), and only its
*read* side — `descriptor`, `claim`, `extract`. `Plugin` hooks (RFC 0003 §2) over WASM, and
`claim_manifest`/`extract_manifest`/`resolve`, are not part of v1; see §2 for why and what
closing each gap would need.

## 1. The WIT world

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

- **No `Plugin` hooks over WASM yet.** `Plugin`'s hooks (`contribute_roots`,
  `contribute_edges`, `annotate_symbols`, …) take a read-only `GraphView` and write through
  typed sinks (contracts/core-traits.md §3) — a materially different, and materially larger,
  ABI surface than an adapter's three flat functions. RFC 0003 §3 already names external
  plugins as a WASM-component tier; this document doesn't retract that, it just hasn't been
  built. First real external-plugin demand should drive its shape, not a guess made here.
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
componentized** `.wasm` file (component-model binary — see §5 for how one gets produced) and
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
zero-config default RFC 0003 §3 already names. A component that fails to load is skipped, not
fatal to the run (§3's same "one bad extension doesn't take down the rest" posture, applied at
load time as well as call time). Feature-gated (`external-adapters`, on by default) so an
embedder building a minimal static binary can drop the WASM runtime entirely
(`--no-default-features --features js,go,...`, ADR 0006).

Demo adapters shipped **in this repository** live outside the compiled product on purpose
(`examples/kndo-plugin-demo`, excluded from the workspace's own `members` — same convention as
`spikes/perf`): "third-party" means never statically linked, checked by keeping it structurally
incapable of being one.

## 5. Producing a component

A third-party author needs a real component-model `.wasm` binary, not a plain core module.
Two ways, both documented rather than assumed:

- `cargo component build` (the `cargo-component` tool) — the ecosystem-standard path.
- The `wit-component` crate directly, as a library, with **no extra tool install** —
  `wit_component::ComponentEncoder::default().module(&core_wasm_bytes)?.encode()?`. This is
  exactly what `kndo-plugin-api`'s own compliance test does to build
  `examples/kndo-plugin-demo` fresh on every run (`crates/kndo-plugin-api/tests/compliance.rs`)
  — it works with zero WASI imports to satisfy (§2/§3), which is true of any v1-conformant
  adapter by construction.

## 6. Compliance

`crates/kndo-plugin-api/tests/compliance.rs` and `crates/kndo/tests/external_adapter.rs`
together are the compliance suite ADR 0003 calls for: the former drives a `WasmAdapter`
directly against a hand-built `Engine`; the latter goes through the full product composition
(`kndo::open`, `.kndo/plugins/` discovery included) — the same code path `kndo-cli` uses for
every command. Both build `examples/kndo-plugin-demo` from source and componentize it
in-process on every run (no binary checked into the repo), then assert a real `unused` finding
comes back correctly through the real reachability engine.

## 7. Versioning

The WIT package version (`kndo:adapter@0.1.0`) and this document change together. A breaking
v2 (adding `resolve()`'s host-import callbacks, `Plugin` hooks, byte-content, or any of §2's
other deferred items) is a new package version, not a silent reinterpretation of `0.1.0` — a
component built against v1 must keep working against a v1-compatible host indefinitely.
