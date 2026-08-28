# `kndo:rkyv` — rkyv conventions plugin

**Status:** Normative for the built-in `kndo:rkyv` plugin ·
**Implements:** `annotate_symbols` (RFC 0003 §2) ·
**Crate:** `crates/kndo-plugin-rkyv` · **Convention set versioned against:** rkyv 0.8

## 1. An honest scope statement

Same shape as `kndo:serde` (plugins/serde.md §1), same two mechanisms stopping short: rkyv's
traits are third-party, so the Rust adapter's stdlib machinery list excludes them and the
core's implement-dispatch fan-out never fires against a trait declared outside the repo.

What makes rkyv worth its own plugin is the **`with` adapters**. A `#[derive(rkyv::Archive)]`
needs no plugin at all — the derive writes the impl, and a generated impl leaves no
hand-written member to lose. But a bridge for a type rkyv does not natively support is
hand-written, and the only thing that ever calls it is the code `#[rkyv(with = W)]` expands
to. Dispatch through generated code is invisible to source extraction by construction
(detection-gaps.md §2), so the whole bridge reads as production-reachable and test-unreached.

kndo's own `kndo-core/src/rkyv_support.rs` is the case: `SmolStrAsString` and `TypeExprAsFlat`
are exactly such bridges, and the file carried a `kndo:allow-file untested` pragma until this
plugin existed. Deleting that pragma is this plugin's field test.

## 2. Detection & the members marked

**Activation:** `ManifestDependency("rkyv")`.

**No file access** (`requested_file_access: []`) — the plugin is its table plus one
`AnnotationSink::mark_machinery_impls` call against `SymbolNode::implements`.

| Trait names | Members marked |
|--------------|----------------|
| `Archive` | `resolve` |
| `Serialize` | `serialize` |
| `Deserialize` | `deserialize` |
| `ArchiveWith` | `resolve_with` |
| `SerializeWith` | `serialize_with` |
| `DeserializeWith` | `deserialize_with` |

The `*With` trio is the load-bearing half. The bare trio is here because a hand-written
`impl Archive for T` is legitimate rkyv even where the derive is the common path.

`Serialize`/`Deserialize` are also serde's trait names, and nothing distinguishes them at
this layer. A project depending on both marks the same member from both tables — which is
harmless by the direction rule: marking only ever keeps a member alive alongside its owner,
never accuses it. Recorded, accepted; a *narrower* answer would require knowing which crate a
trait path resolves to, which is real type resolution, not a convention.

## 3. Recorded limits

- **`#[rkyv(with = W)]` attribute reading is not modelled.** The attribute names a type whose
  impls this plugin already marks through their trait, so reading it would add nothing today.
  It would matter for a `W` with no hand-written impl at all, which has no observed case.
- Native-only, same as `kndo:serde` (plugins/serde.md §3).
