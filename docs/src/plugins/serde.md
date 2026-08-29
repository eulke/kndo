# `kndo:serde` — serde conventions plugin

**Status:** Normative for the built-in `kndo:serde` plugin ·
**Implements:** `annotate_symbols`, `contribute_edges` ·
**Crate:** `crates/kndo-plugin-serde` · **Convention set versioned against:** serde 1.x

## 1. An honest scope statement

serde's traits are third-party, so the two mechanisms that normally model nameless
dispatch both stop short of them, deliberately:

- The Rust adapter's machinery-trait list covers the *stdlib*
  only — teaching the language adapter about an ecosystem crate would be exactly the
  coupling the adapter/plugin split exists to prevent.
- The core's implement-dispatch fan-out is keyed off resolved `Implement`
  edges, and an `impl Serialize for T` names a trait declared outside the repo — the
  reference resolves to nothing, so no fan-out fires.

Yet a hand-written `impl Serialize for T` behaves exactly like a language hook:
`serialize` is invoked by serde's machinery whenever `T` is serialized, and **never by
name from user code**. Without this plugin, every such member reads as production-reachable
(the trait-impl dispatch root keeps it alive) but test-unreached — a permanent `untested`
false family (ripgrep: `jsont.rs`, `globset/serde_impl.rs`).

The plugin closes the gap with the framework counterpart of the contract's
`Declaration::implicitly_invoked` flag: `AnnotationSink::mark_implicitly_invoked`. A marked
member inherits its owner's reachability colors at `Probable` — a `Serialize` impl on a
test-covered type stops reading as a test blind spot. Nothing serde-specific enters the
core: the plugin contributes the *fact*, the
machinery-dispatch rule already knew what to do with it.

`#[derive(Serialize)]` needs none of this — derived impls produce no declarations, so
there is nothing to accuse.

## 2. Detection & the members marked

**Activation:** `ManifestDependency("serde")` — a repo that doesn't depend on serde never
runs the plugin (and never pays the graph-cache bypass).

**No file access at all** (`requested_file_access: []`). The plugin is a curated table and a
single call to `AnnotationSink::mark_machinery_impls`, which matches the table against
`SymbolNode::implements` — the trait whose `impl` block declares each member, extracted by
the Rust adapter. The trait reduces to its base name there, so
bare (`impl Serialize for Glob`), qualified (`impl<'a> serde::Serialize for Message<'a>`) and
generic (`impl<'de> Visitor<'de> for GlobVisitor`) forms all land identically:

| Trait names | Members marked |
|--------------|----------------|
| `Serialize` | `serialize` |
| `Deserialize` | `deserialize`, `deserialize_in_place` |
| `DeserializeSeed` | `deserialize` |
| `Visitor` (serde::de) | `expecting`, `visit_*` |

A same-named *local* trait would over-mark — at `Probable`, silence-direction only, and
gated on the manifest actually depending on serde; recorded, accepted.

**History, because it is the architectural point.** This plugin used to request `**/*.rs`,
gate on a member-name pre-check, and run its own single-line scan of impl headers — it
re-parsed a grammar the adapter had already parsed, because the adapter reduced "which trait
declares this member" to a single `implicitly_invoked` bool and threw the name away. Parsing
Rust is language knowledge and belongs to the adapter; knowing what `Serialize` means is
tool knowledge and belongs here. Giving the fact a name in the contract put each half where
it goes and left this plugin as its table. Verified equivalent: the serde repository's
findings are byte-identical across the change.

## 3. The second gap: a function named only by an attribute

`#[serde(skip_serializing_if = "usize_is_zero")]` is a call site. The function's only caller
is code serde's derive macro generates, which exists in no source file — so to plain
reachability the field carries a string and the function is dead. kndo reported exactly that
about its own tree, and the tree was changed to avoid the finding: an envelope field became
`Option<usize>` purely so no named predicate would be needed. A tool whose own source is
shaped around what it cannot see is the failure this closes.

**The fact is the adapter's, the meaning is this plugin's.** The Rust adapter records
`FileFacts::string_attr_args` — attribute head, key, literal, and the declaration decorated —
and stops. It cannot go further and the measurement says why: serde alone writes 482
identifier-shaped `key = "literal"` pairs, 248 of whose values collide with a real declaration
in the crate, and only 176 sit under a key serde resolves as a path. An adapter treating a
collision as a reference would contribute **72 keep-alive edges in one crate to close one real
case**, and each one silences a true finding. Telling `skip_serializing_if = "f"` from
`rename = "f"` requires knowing what serde is.

So the table lives here, and it is the whole of what the plugin knows:

| Key | Value names |
|---|---|
| `skip_serializing_if` | a predicate function |
| `serialize_with` · `deserialize_with` · `with` | a function or module |
| `default` | a function producing the value |
| `getter` | a method on the remote type |
| `bound` · `remote` · `try_from` · `into` · `from` | a type |

Everything else serde writes between quotes is data — `rename`, `rename_all`, `tag`,
`content`, `crate`, `expecting` — and the plugin ignores it. `remote` is in the table because
its value names a type; it looked like noise in the first measurement and is not, which is
exactly the sort of detail only serde's own plugin can be right about.

**The edge runs from the decorated declaration to the named item**, `Probable`. That direction
is the true one: the generated impl belongs to the type, so the function runs exactly when
that type is serialized. Field attributes are attributed to the enclosing struct or enum,
which is the same statement — fields are not declarations, and the impl is the type's.

**Scoped to the declaring file**, because a `PluginTarget` is `{path, symbol}` and an
attribute knows only a name. `crate::util::is_zero` or `chrono::serde::ts_seconds` resolves to
nothing and is dropped, counted in `kndo doctor`'s dropped record. Reaching the cross-module
case needs a project-wide target form — a contract change, not something to approximate here.

## 4. Mechanism & recorded limits

Marks resolve through the plugin round's qualified member selector (`Owner.name`), land in
the graph's plugin partition (`ProjectGraph::plugin_implicitly_invoked` — sorted,
snapshot-round-tripped, discarded and re-derived by the incremental patch like every plugin
contribution), and reachability's machinery-dispatch rule reads them alongside the
adapter's own declaration flags. Two sources, one rule.

- **Native-only for now:** the WIT ABI `annotate-symbols` surface does not carry the
  mark — an ABI candidate. The attribute-string fact it now also reads is *not* in that
  position: `attr-strings-in` is an additive import like `symbol-implements`, so a
  third-party plugin can build the same convention over its own framework's attributes.
  External WASM plugins cannot emit it yet, though they *can* read
  the fact it keys off: `symbol-implements` is an additive import.
- **`Serializer`/`Deserializer` implementors** (the format-crate side of serde) are out of
  scope: format crates are the machinery, their methods are called by serde's *generated*
  code paths in ways this convention set doesn't model. Revisit against a real format-crate
  corpus.
- Other ecosystems with the same shape get their own plugins on the same channel — this spec
  deliberately covers serde alone. `kndo:rkyv` and `kndo:wasmtime` ship as siblings
  ([rkyv.md](rkyv.md), [wasmtime.md](wasmtime.md)) and share not one line of serde vocabulary: what
  they share is `mark_machinery_impls`, which is the core's, and the fact it reads, which is
  the adapter's.
