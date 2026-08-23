# `kndo:serde` — serde conventions plugin

**Status:** Normative for the built-in `kndo:serde` plugin ·
**Implements:** `annotate_symbols` (RFC 0003 §2), content-channel-aware (RFC 0016 §5) ·
**Crate:** `crates/kndo-plugin-serde` · **Convention set versioned against:** serde 1.x

## 1. An honest scope statement

serde's traits are third-party, so the two mechanisms that normally model nameless
dispatch both stop short of them, deliberately:

- The Rust adapter's machinery-trait list (docs/adapters/rust.md §2) covers the *stdlib*
  only — teaching the language adapter about an ecosystem crate would be exactly the
  coupling the adapter/plugin split exists to prevent.
- The core's implement-dispatch fan-out (RFC 0005 §1) is keyed off resolved `Implement`
  edges, and an `impl Serialize for T` names a trait declared outside the repo — the
  reference resolves to nothing, so no fan-out fires.

Yet a hand-written `impl Serialize for T` behaves exactly like a language hook:
`serialize` is invoked by serde's machinery whenever `T` is serialized, and **never by
name from user code**. Without this plugin, every such member reads as production-reachable
(the trait-impl dispatch root keeps it alive) but test-unreached — a permanent `untested`
false family (ripgrep: `jsont.rs`, `globset/serde_impl.rs`).

The plugin closes the gap with the framework counterpart of the contract's
`Declaration::implicitly_invoked` flag: `AnnotationSink::mark_implicitly_invoked`
(RFC 0005 §1's machinery-dispatch rule). A marked member inherits its owner's reachability
colors at `Probable` — a `Serialize` impl on a test-covered type stops reading as a test
blind spot. Nothing serde-specific enters the core: the plugin contributes the *fact*, the
machinery-dispatch rule already knew what to do with it.

`#[derive(Serialize)]` needs none of this — derived impls produce no declarations, so
there is nothing to accuse.

## 2. Detection & the members marked

**Activation:** `ManifestDependency("serde")` — a repo that doesn't depend on serde never
runs the plugin (and never pays the graph-cache bypass).

**Candidate gating, cheapest check first:** a file is only ever read through the content
channel (`requested_file_access: **/*.rs`) when its symbol table already declares a member
serde's machinery could invoke — `serialize`, `deserialize`, `deserialize_in_place`,
`expecting`, `visit_*` — on a production-role Rust file. Everything else never touches the
channel.

**Impl-header detection** is a deterministic single-line scan of the read source (every
observed corpus shape writes the header on one line; a hand-wrapped header contributes
nothing — degrade toward silence, RFC 0002 §5). The trait matches by its path's last
segment, so bare (`impl Serialize for Glob`), qualified (`impl<'a> serde::Serialize for
Message<'a>`), and generic (`impl<'de> Visitor<'de> for GlobVisitor`) forms all land:

| Header names | Members marked |
|--------------|----------------|
| `Serialize` | `serialize` |
| `Deserialize` / `DeserializeSeed` | `deserialize`, `deserialize_in_place` |
| `Visitor` (serde::de) | `expecting`, `visit_*` |

A same-named *local* trait would over-mark — at `Probable`, silence-direction only, and
gated on the manifest actually depending on serde; recorded, accepted.

## 3. Mechanism & recorded limits

Marks resolve through the plugin round's qualified member selector (`Owner.name`), land in
the graph's plugin partition (`ProjectGraph::plugin_implicitly_invoked` — sorted,
snapshot-round-tripped, discarded and re-derived by the incremental patch like every plugin
contribution), and reachability's machinery-dispatch rule reads them alongside the
adapter's own declaration flags. Two sources, one rule.

- **Native-only for now:** the WIT ABI v1 `annotate-symbols` surface does not carry the
  mark — an ABI v2 candidate. External WASM plugins cannot emit it yet.
- **`Serializer`/`Deserializer` implementors** (the format-crate side of serde) are out of
  scope: format crates are the machinery, their methods are called by serde's *generated*
  code paths in ways this convention set doesn't model. Revisit against a real format-crate
  corpus.
- Other ecosystems with the same shape (an ORM's lifecycle hooks, a test framework's
  fixtures) get their own plugins on the same channel — this spec deliberately covers serde
  alone.
