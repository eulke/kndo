# `kndo:wasmtime` — wasmtime component conventions plugin

**Status:** Normative for the built-in `kndo:wasmtime` plugin ·
**Implements:** `annotate_symbols` ·
**Crate:** `crates/kndo-plugin-wasmtime` · **Convention set versioned against:** wasmtime 30+

## 1. An honest scope statement

`wasmtime::component::bindgen!` generates a trait per WIT interface and one per world's
imports; the host implements them by hand. **Nothing in the repo ever calls those methods** —
the caller is generated glue that runs when a *guest* calls out — so the entire host surface
reads as unreachable. This is a real limit of source-only analysis, and the exact thing a
conventions plugin exists to answer.

kndo's own `kndo-plugin-api/src/plugin_host.rs` is the case, and carried a
`kndo:allow-file untested` pragma until this plugin existed.

## 2. Detection & the members marked

**Activation:** `ManifestDependency("wasmtime")`.

**No file access** (`requested_file_access: []`) — its rule plus one
`AnnotationSink::mark_machinery_impls` call against `SymbolNode::implements`.

Unlike every other conventions plugin, the traits are matched by **shape, not by name**:

```
Host…            one per WIT interface, and `Host<Resource>` per resource type
…Imports         one per world (`{World}Imports`)
```

`bindgen!` derives those names from the project's own WIT, so a fixed list is impossible *in
principle* here rather than merely incomplete. That shape IS wasmtime's convention, which is
what keeps this a plugin for one tool rather than a guess about WASM in general.

**Every member of a matched impl block is marked**, with no member predicate — a generated
host trait exists only so the guest can call it, so its whole surface is machinery-invoked by
construction. This is the plugin the per-impl-block fact matters most for: a host state type
also implementing `Debug` keeps its `fmt` unmarked, because `implements` is recorded per
declaration and not per type.

## 3. Recorded limits

- **A hand-written trait whose name starts with `Host`** over-matches. Direction rule again:
  keep-alive only, and gated on the manifest depending on wasmtime.
- **The guest side is not modelled.** A component's *exports* are reached from the host
  through generated glue too; no observed case yet, and the export surface is already rooted
  as a library boundary in the projects examined.
- Native-only, same as `kndo:serde`: the WIT ABI's `annotate-symbols` surface doesn't carry
  `mark_machinery_impls`'s call yet, so a third-party WASM plugin can't reproduce this marking
  outside the native plugin path.
