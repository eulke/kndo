# Adapter Spec — Rust

**Status:** Draft · **Implements:** `LanguageAdapter` (contracts §2) · **Milestone:** M5
**Grammar:** tree-sitter-rust

The third language, and the first where kndo analyzes *itself* — the dogfood ceiling. Rust
stresses the contract in ways JS/TS and Go did not: the file graph is built by `mod`
declarations rather than imports, visibility is a four-step keyword ladder rather than a
binary, a package (crate) has *two* kinds of roots (lib API and bins), and macros expand
arbitrary code the static graph cannot see. Every stance below is written against those.

## 0. The one Rust-shaped idea

**The module tree IS the file graph.** In JS, any file may import any file; in Go, a package
is a directory. In Rust, files participate only when a `mod` chain from the crate root
declares them — `src/lib.rs` says `mod a;`, `a.rs` says `mod b;`, and only then does
`src/a/b.rs` exist as far as the compiler cares. Two consequences the adapter is built on:

1. `mod foo;` is an **import** in kndo's model — the parent file's certain `ImportsFile`
   edge to the child (`foo.rs` or `foo/mod.rs`). A file no `mod` chain reaches is dead to
   the compiler, and kndo's reachability reproduces that verdict for free through these
   edges — no special "orphan module" machinery.
2. `use` paths resolve **through the same tree**: `crate::` starts at the crate root
   (`src/lib.rs`, else `src/main.rs`), `self::` at the current file's module directory,
   `super::` one module up. All of it is path arithmetic over the discovered file set —
   no compiler queries, per RFC 0002.

## 1. Claiming & classification

| Claim | Files |
|-------|-------|
| Language `rust` | `**/*.rs` |
| Manifests | `Cargo.toml` (workspace and package alike). `Cargo.lock` is **not** claimed |
| Role `test` | `tests/**` (integration tests), `benches/**`, `examples/**` — **package-relative**: declared as `package_test_dirs` on the descriptor and matched by core assembly against the dir of the owning `Cargo.toml`, not path-globally |
| Role `tooling` | `build.rs`, `.cargo/**`, `xtask/**` (the de-facto task-runner convention) |
| Origin `generated` | first-64-lines markers via the toolkit's `ContentMarkers` (`@generated`, `Code generated`, `Automatically generated`) — covers bindgen/prost/tonic banners. Build-script output lives in `OUT_DIR`, outside the tree — no stance needed |
| Origin `vendored` | `vendor/**`, `third_party/**` (toolkit universal list) |

`examples/**` is `test` deliberately: an example consumes the public API from outside like a
test does, and a symbol alive *only* through its own demo is exactly the `test-only` verdict
— dead API kept warm by its own showcase.

Package-relative matters because these are Cargo *target* conventions: they bind to the
manifest beside them. A workspace-excluded crate whose sources happen to live under some
ancestor's `examples/` or `tests/` tree is owned by its **own** `Cargo.toml`, and its
`src/` is ordinary production code — matching the segment anywhere in the path misroled
every such nested crate wholesale.

**Sub-file test regions.** Rust tests are whole files (the table above) *or* blocks inside
production files — so role-by-path alone cannot separate production from test code, and the
adapter must know the **spans** that belong to test constructs. Extraction records them as
`FileFacts::test_spans` (contracts §2): the extent — gating attributes included — of every
outermost `#[cfg(test)]` item (typically `mod tests { … }`), every `#[test]`/`#[bench]`
function, and the whole file under a `#![cfg(test)]` inner attribute. Items *inside* a
recorded region add nothing (the outermost extent covers them). The file's claimed role does
not flip; consumers act at span granularity:

- declarations inside a region become in-source **test roots, derived by assembly** from
  span containment (contracts §2 — the spans are the *single* producer-side declaration;
  extraction emits no per-declaration Test roots, so the two representations cannot drift).
  Reachability colors from those derived seeds, and the core exempts test-rooted symbols
  from `test-only`/`untested` (a test reachable only from tests is a test);
- `crap` and health's symbol tallies **skip** region-contained symbols — the exact exemption
  test files get (a gnarly test helper is not untested production code); `duplicate`
  deliberately does *not* skip them — it fingerprints test files too, so inline test clones
  remain findings;
- `dependency_hygiene` treats an import **sited** inside a region as a test-role usage: a
  `[dependencies]` crate consumed only under `#[cfg(test)]` is a `test-only` dependency that
  belongs in `[dev-dependencies]`;
- the **out-of-line gated module** — `#[cfg(test)] mod tests;` pointing at `src/tests.rs`, a
  whole-file test the path claim cannot see — is handled at assembly (contracts §2, phase
  2.55): the `mod` import originates inside a test region, and a file reached *only* by
  test-gated module links is demoted to test role (any production-sited `mod` link vetoes).

## 2. Extraction

**Declarations** — `fn`, `struct` (+ named fields? no — fields are not independent liveness
units in v1), `enum` + variants (variants as `EnumMember` members of the enum, RFC 0012 §3),
`union`, `trait` (+ its method signatures as members of the trait), `impl` methods and
associated consts/types (members of the **self type**'s name; `impl Trait for T` methods are
members of `T` and additionally emit an `Implement` reference to `Trait`), `const`,
`static`, `type` aliases, `macro_rules!` (kind `Macro` — the contract's expansion-symbol
kind: visibility-scope analyses skip it as a subject and treat references attributed to it
as expansion-site-wide, core-traits.md §1). Inline modules
(`mod x { … }`) **flatten**: their items extract at file level, undecorated — file ≈ module
is this adapter's standing approximation, stated once here and leaned on everywhere.
`mod foo;` (the file-declaring form) is an import, not a declaration (§0).

**Visibility ladder** (RFC 0012 §6), declared on the descriptor:

```
[ Module@own "private", Module@parent "pub(super)", Package "pub(crate)", Public "pub" ]
```

- no `pub` → level 0, on the **`Module` rung anchored at the file's own unit**. Rust privacy is
  module-and-descendants, and that is now sayable: it used to be approximated as `File` scope
  because the ladder had no rung for a subtree, and the approximation is what made
  `private-type-leak` accuse `flags::parse::lookup` for naming `flags/mod.rs`'s private `Flag`
  — a type every module under `flags` can spell perfectly well.
- `pub(crate)` → level 2 (`Package` scope — crate = kndo package, exactly).
- `pub(self)` → level 0 — it IS `private`, spelled long.
- `pub(super)` on an item **inside an inline mod** → level 0, not exported: `super` of an
  inline mod is a module within this same file, so under file ≈ module the item never
  leaves the file (widening it to the crate rung fabricated `internal-only` on ripgrep's
  `mod convert { pub(super) fn … }` — M6 residuals).
- Top-level `pub(super)` → level 1, its own rung: the **`Module` rung anchored at the PARENT
  unit**. It used to be widened into `pub(crate)` because nothing sat between one file and one
  package, and that widening was what kept `private-type-leak` gated — the model could not tell
  tokio's `task::state::unset_waker` (whose sibling caller genuinely cannot name the
  `state.rs`-private `UpdateResult`) from the harmless inverse. Both rungs are `Module` and
  differ only by ANCHOR, which is the honest shape: in Rust every visibility but `pub` is a
  module subtree, and what distinguishes them is how high the subtree is rooted.
- `pub(in path)` → level 2, still widened to `pub(crate)`: this adapter does not resolve the
  path to a unit key yet. Widening only ever silences an `internal-only`, never accuses (7
  occurrences across the whole of tokio, for scale).
- `pub` → level 2. `exported` = any `pub*` form except `pub(self)` and
  inline-mod `pub(super)`.

**Unit:** the file's own MODULE, keyed by its path with the conventional file names folded
into the directory they stand for — `src/graph/mod.rs` and `src/graph/assemble.rs` are
`…/src/graph` and `…/src/graph/assemble`, and `src/lib.rs` is `…/src`. Rust's resolution unit
IS the module, and under file ≈ module (§0) a module is a file, so **every key names exactly
one file**.

That degeneracy is why this is not the Go machinery in disguise. Rust files still never resolve
each other's names implicitly — a one-file unit table is the file's own table, which the ladder
already consults first, so no name resolves anywhere it did not before. What the key buys is
the one thing a per-file table cannot express: **twins**. `#[cfg(target_os = "macos")] fn
socket_dir` beside `#[cfg(not(…))] fn socket_dir` is two declarations of one name, and the
single-slot bare table kept one and silently dropped the other, leaving it with no incoming
edge and a false `unused` on code every other build compiles. Twins are tracked per unit
(RFC 0012 §8), so a language with no unit key had none. `#[path]` and inline `mod x {}` break
the path convention, the same standing approximation the rest of the adapter makes.
`unit_name`: also `None` (qualified references resolve through import aliases instead).

**References** — identifiers with `within` (enclosing fn/method, `Owner.name` form for
members, RFC 0012 §4); qualified paths emit `scope_context` = the immediate qualifier
segment (`helpers::run()` → `{name: run, scope_context: helpers}`), which the core matches
against import bindings/aliases (RFC 0012 §9); method calls (`x.foo()`) and field accesses
emit member references that land in the duck-typed member fallback (RFC 0012 §3) — Rust's
`Deref`-based method resolution is exactly the case that fallback exists for. Type positions
→ `TypeUse`; `impl Trait for T` → `Implement`. Attribute derives (`#[derive(Serialize)]`)
emit `TypeUse` references to each derive name at `probable` — that is what keeps a
derive-only dependency honest in dependency hygiene. **Attribute arguments** more generally
are token soup that may name real items: `a::b::C` runs are reconstructed and emitted like
body paths (`#[rkyv(with = crate::rkyv_support::SmolStrAsString)]` imports the module and
`TypeUse`-references the item; `#[derive(serde::Serialize)]` keeps `serde` used), at item
level and field/variant level alike — a struct alive only through a field attribute stays
alive. Lint-control and non-item attributes (`allow`/`warn`/`deny`/`forbid`/`expect`,
`doc`, `cfg`/`cfg_attr`) are excluded: their arguments are lint paths and config keys, and
`#[allow(clippy::x)]` must never invent a `clippy` dependency. **Trait items inherit the
trait's visibility** (the same language rule as enum variants): a `pub trait`'s methods sit
on the public rung, which is what lets the member fallback see their cross-file call sites
and what places them on a published crate's API surface.

**Imports** — `use` declarations, mapped to the contract like this:

| Form | Emission |
|------|----------|
| `use p::{X, Y as Z}` | specifier `p`, bindings `[X, {local Z, imported Y}]` |
| `use p::X` (single, brace-free) | specifier `p::X`, binding `[X]` — the resolver's two-step rule (§3) sorts out whether `X` was a module or an item; extraction never guesses by capitalization |
| `use p::*` | specifier `p`, no bindings, `opaque_namespace_use: true` → the core's `Wildcard` over the target's exports, which is the truth of a glob |
| `use p as q` | specifier `p`, `local_alias: q` (qualifier alias for `q::…` references) |
| `pub use …` | same as above + `reexported: true` — Rust re-export facades ride the same barrel machinery as JS, fixpoint-resolved (RFC 0013 §3b), multi-hop included |
| `mod foo;` | specifier `self::foo`, `side_effect_only: true`, `local_alias: foo` — the file-linking edge (§0); `#[path = "…"]` on the `mod` overrides the conventional location with the literal path. The alias is not decoration: it is how this file states "the name `foo` binds to that file", which is the producer side of the core's module hop (below) |
| `extern crate name;` | specifier `name` (resolves as a dependency/stdlib like any bare first segment) |

`ImportKind::Relative` for `crate::`/`self::`/`super::` paths, `Package` for bare-first-
segment paths. All `use` edges are `certain` — Rust has no bundler ambiguity.

**A brace member may be a submodule, and the core hops for it.** `use crate::internals::{attr,
check, Ctxt};` emits bindings for all three, but `check` names a *module*, not an item — nothing
in `internals/mod.rs` declares it, so the binding resolves to no symbol and `check::check(cx, …)`
used to bind nothing (`internal/detection-gaps.md` §8: this killed serde's whole `check_*`
family). The adapter emits nothing special for it; the resolution is core-side and
language-blind (RFC 0012 §9-bis): a name bound to a file that the *target's own* import table
binds again follows that one hop, and `internals/mod.rs`'s `mod check;` — with its
`local_alias` — is exactly that second binding. This is why the alias on `mod foo;` is
load-bearing, and why extending the specifier to `crate::internals::check` was the wrong fix: it
would teach the core that `::` joins path segments.

**Which imports are reconstructed.** Every import the adapter synthesizes from a use site —
attribute paths, body paths (both the rooted and bare-rooted branches), the root probes, and paths
inside macro token trees — carries `reconstructed: true`. A `use`, a `mod foo;` and an
`extern crate` do not: the file contains those. The flag is what stops a synthesized binding from
shadowing the file's own declaration and what keeps its qualifier from settling a miss
(RFC 0012 §9-quater); confidence cannot stand in for it, since a `crate`/`self`/`super`-rooted
synthetic import is `Certain` about where it resolves.

**And the adapter names every qualifier the core used to guess.** A body path with no `use`
behind it (`helpers::run()`, `kndo_core::discovery::find_files_named(..)`) reconstructs as a
synthetic module import, and that import carries `local_alias` — the segment the use site
actually qualifies by, which only something that knows what `::` joins can identify. The core
previously recovered it by splitting the specifier, its single piece of hardcoded language
syntax; that fallback is gone (RFC 0012 §9-ter). These imports are reconstructions, so they
carry `probable`/`possible` confidence, and the core reads that as "does not close the
namespace": a miss under such a qualifier keeps falling through to the in-scope/duck ladder,
where a real `use` statement's alias would settle. The braced form without `self`
(`use a::b::{X, Y}`) deliberately sets no alias — Rust does not bring `b` into scope, and the
extraction table above has always said so.

**Dynamic constructs** — macros, with a deliberately bounded stance:

| Construct | Effect |
|-----------|--------|
| `name!(…)` invocation | a `certain` reference to `name` (keeps `macro_rules!` alive), plus a scan of the token tree: `a::b` token runs are reconstructed and routed through the body-path rule — `print!("{}", render::render_query(x))` binds `render_query` through the `use`-established qualifier exactly as it would outside the macro — and lone identifier tokens stay plain reads (`format!("{}", user)` keeps `user`'s referents alive). String literals inside token trees are scanned for Rust 2021 **inline format captures**: `format!("v{VERSION}")` reads `VERSION` (`{{` escapes and positional `{}`/`{0}` contribute nothing). **No wildcard per macro** — that would drown every Rust file in `possible` edges |
| `include!("lit")` / `include_str!` / `include_bytes!` with a literal | `probable` file edge |
| `env!("CARGO_BIN_EXE_<name>")` / `option_env!` | `invoked_executables` fact: Cargo's own handshake for "this test executes the workspace binary `<name>`" — the core resolves the name through the manifest's named bins (§4) and emits an `InvokesFile` edge, RFC 0005 §1's invoked-program rule. Only the documented prefix, only a literal |
| `#[no_mangle]` / `#[export_name]` / `pub extern "C" fn` | in-source `Production` root at `probable` — an FFI consumer exists outside the graph |
| `#[test]` / `#[bench]` | a recorded test region (§1) — assembly derives the `certain` `Test` root from span containment; extraction emits no root of its own |
| `impl MachineryTrait for T` members | `implicitly_invoked` (RFC 0005 §1's machinery-dispatch rule) — the curated criterion is "the call site never writes the method's name": the fmt hooks (`Display`/`Debug`/numeric formats), `Drop`, `Default`, comparison + `Hash`, the operator-overload traits, `Index`/`Deref` sugar, `Iterator`/`IntoIterator`, `Future`, `FromStr`, `Error`. Composes with the trait-impl `Probable` Production root (which keeps hooks alive with zero owner usage): the flag lets them *inherit the owner's colors*, so a `Display` impl on a test-covered type stops reading as untested. Name-called trait methods (`.clone()`, `.into()`) and third-party traits (serde et al.) deliberately stay out — the duck fallback covers the former; a curated fact table beyond the stdlib is the recorded future source for the latter |
| `#[cfg(…)]` | **both branches kept**, always: kndo analyzes the source, not one compilation; over-approximating alive is the safe direction. Two cfg-gated same-name items collapse to last-wins in the symbol table (documented artifact, harmless for liveness) |

**Metrics** (`MetricsSyntax` as data): branches `if_expression`, `match_arm` (n-way match ≈
n−1 branches plus the base — counted per arm past the tree shape), `while_expression`,
`for_expression`, `try_expression` (`?` is an early-return branch), `&&`, `||`;
identifiers → `identifier`, `field_identifier`, `type_identifier`,
`shorthand_field_identifier`; literals → string/raw-string/char/byte/integer/float
literals; skipped → `line_comment`, `block_comment`. Winnowing parameters shared (toolkit).

**Suppressions** — `// kndo:allow …` via the toolkit scanner, like every language.

**Extraction refinements from the M6 FP hunt (ripgrep corpus):**
- A body-scoped `use` (inside a function) routes through the same `collect_use` extraction as
  item-level uses — walking it as expressions fabricated phantom package references from its
  intermediate segments (`use std::{fs::File, os::{fd::AsFd, …}}` → "fs"/"os"/"fd" as
  packages).
- Scoped items inside use lists register their bound tails as local qualifiers
  (`use crate::flags::{doc::version}` binds `version`, so `version::generate()` is that
  import's alias, never a phantom root import).
- A `use` inside a macro *invocation's* token tree keeps only its root as a side-effect import
  and skips through its `;` — inner segments are import structure, not expression paths.
- `macro_rules!` expansion templates (each rule's right-hand token tree) are scanned with the
  same reconstruction macro invocations get, attributed `within` the macro — ripgrep's
  `err_message!` calls `crate::messages::set_errored()` from a template, which was otherwise
  invisible and false-positived the callee as `unused`.
- `#[macro_use] mod x;` sets `opaque_namespace_use: true` on the mod import: it globs the
  child's macro namespace into crate scope — invocations anywhere reach its `macro_rules!`
  with no import for a binding to express, which is precisely RFC 0005 §1's wildcard truth.
  The per-macro no-wildcard stance (§2's table) is untouched; this is one edge per
  `#[macro_use]`, not one per invocation.

**When a bare path root is EVIDENCE of a crate (the field-audit pass, serde/alacritty
corpus).** A bare path (`mem::size_of`, `fmt::Write::write_fmt`) emits a root import so that
an unknown crate becomes the `Dependency` edge `undeclared` judges. That claim is only as good
as the file it came from, and five shapes make it worthless — in each the import is
**downgraded to `Possible`, never dropped**: `undeclared` ignores that tier so the accusation
disappears, while the edge survives so a *declared* crate reached only through such a path
still reads as used. (Suppressing them outright instead turned alacritty's `dirs` and `home`
into false `unused` dependencies — the downgrade is load-bearing, not cosmetic.)

| Shape | Why the root proves nothing | Field case |
|-------|----------------------------|------------|
| A glob import anywhere in the file (`use crate::lib::*;`) | a glob binds an open set of names extraction cannot enumerate without resolving the target | serde re-exports `mem`, `cmp`, `fmt`, `iter`, `net`, `slice` this way — all six accused of being phantom deps of `serde_core` |
| Inside a macro token tree (`quote!` body, `macro_rules!` right-hand side) | a template describes code that does not exist yet, under names the expansion invents | serde_derive's `_serde`, `__S`, `__D`, `__E`, `__A`, `__Field`, `__private`, `private2`, and `clippy::…` lint paths |
| The root names a **type** declared in this file | a type in scope shadows an extern-prelude crate; raw identifiers (`enum r#type`) defeat the "lowercase root ⇒ crate-shaped" test outright | serde's test suite, `r#type::r#struct` |
| The root is a name this file's own `use` statements bind to something else | `use serde::de::{self, …};` then `use de::Error;` re-qualifies that binding — an import binding shadows a crate of the same name | alacritty `alacritty/src/config/bindings.rs` |
| An inline `mod` declared in a **function body**, then `use`d below it | the flatten model puts inline-mod contents in this same file; nothing outside it is named | serde `test_annotations.rs`'s `mod desugared` |

The boundary for the macro rule is the token TREE, not the invocation: `serde_json::json!(…)`'s
own path is ordinary code at the call site and keeps probing, because it is a sibling of the
tree rather than inside it. Both emitters carry the gate — the single-qualifier root and the
deep-path root (`fmt::Write::write_fmt` is three segments, so only the deep one ever fires for
it; hardcoding `Probable` there kept serde's `fmt` accused after the shallow branch was fixed).

- Member chains inside macro token trees get the same receiver typing as body code:
  `write!(col2, "{}", flag.doc_short())` arrives as token soup, but the receiver's type is
  a declared fact — the reference carries qualifier `Flag` exactly as outside the macro
  (untyped receivers keep the raw name, the duck route; argument token trees are still
  scanned). Before this, every member call inside `write!`/`format!` degraded to a bare
  read that resolved to nothing — ripgrep's whole help generator was invisible.
- A path through a same-file **inline mod** (`convert::usize(…)` with `mod convert { … }`
  right there) emits a *bare* reference: extraction flattens inline-mod bodies, so the
  target is a same-file symbol — routing it through qualifier resolution had nothing to
  bind to and false-positived the target as `unused`.
- A qualified path whose qualifier is a **type** (`logger::Logger::init()`) additionally
  references the type itself (`Logger`, resolved through the segment before it): the
  traversal uses the type, and without the reference it read as file-local and
  `internal-only` advised narrowing it.
- A `crate::`/`self::`/`super::` path that crosses into type space
  (`crate::logger::Logger::init()`) splits at the first uppercase segment: the module
  prefix (`crate::logger`) becomes the import specifier binding the type (`Logger`), the
  type is referenced, and the trailing item resolves through the member table — an unsplit
  specifier resolved to no file and the whole chain read as dead.
- Every member of an `impl Trait for T` — methods AND associated types/consts (`type
  Output = …` in an `Add` impl) — roots at `Probable`: trait-impl items are consumed
  through the trait's dispatch machinery (`dyn`, generic bounds, operators, `for` loops),
  never by name, so the name-based fallback cannot see their consumption (the RFC 0012 §3
  dispatch rule made concrete; same stance as Swift's conformance witnesses). Twin impls
  (`impl Add for Stats` twice with different RHS) each root — the core lands a
  declaration-targeted root on every declaration sharing the selector.
- `#[global_allocator]` / `#[panic_handler]` / `#[alloc_error_handler]` root the item: the
  runtime is the consumer, same externally-invoked semantics as `#[no_mangle]`.

**Receiver typing (RFC 0012 §3-bis)** — a per-function `TypeEnv` built from language FACTS
visible in the file pins receiver types, so `args.matcher()` emits its TYPE as the
qualifier (`HiArgs`) and the core resolves the member in the type's home file at Certain,
exactly like the explicit `HiArgs::matcher` path. The sources, in order of certainty:

- `self` / `Self` → the enclosing `impl`'s self type (including `Self::assoc()` paths and
  `let x = Self::new()` initializers).
- Typed parameters — fn and closure alike — and annotated `let`s. The declared type is
  reduced to its dispatch base: `&`/`&mut` stripped, `Box/Rc/Arc<T>` auto-deref to `T`,
  `impl Trait`/`dyn Trait` to the trait, `Vec<T>` stays `Vec`.
- Struct-literal initializers (`let t = Widget { .. }` — certain) and `T::assoc(…)`
  initializers (`T::new()`, builders — the constructor convention), with the type flowing
  through call chains (`SearchWorkerBuilder::new().opt(x).build()` types every link).
- A name bound to CONFLICTING types anywhere in the function is dropped outright:
  ambiguity degrades to the duck-typed fallback (RFC 0012 §3), never to a guess.

Reliability bound, stated once: a wrong inference can only MISS (→ duck fallback, exactly
today's behavior — the core never settles on a receiver-typed qualifier) or hit a member
the named type genuinely declares — both degrade toward silence, never toward accusation.
**Member-type facts (the cross-file tier, `FileFacts::member_types`)** — every struct
field (named and tuple-positional), impl method return type, and associated const emits an
`(owner, member, yields)` fact, dispatch-reduced, `Self` resolved to the owner. Three more
producers, all of them *declared* facts rather than inference (RFC 0012 §3-ter):

- **Every top-level `fn`'s return type**, as an OWNER-LESS fact — "calling this evaluates to
  that". `let entry = parse_entry(..); entry.path` has no receiver to read a type off anything
  else, and without the fact the type `parse_entry` returns looks used only where it is declared.
  `Self` is left alone there: a free function has no impl around it, so the name simply resolves
  to nothing.
- **`#[derive(Default)]`** → `(T, default) → T`. The derive states that the impl exists and the
  trait's signature states what it returns — the same curated-stdlib knowledge
  `is_machinery_trait` carries, and no derive means no fact.
- **A `for` variable** binds `{iterable}?0` — the element is parameter 0 of the collection's
  declared type, which the projection marker below already expresses. Only a CHAIN qualifies:
  a local's own annotation kept just its base name, and `Vec` alone has no parameters to project.

A CALL initializer types its binding by naming the FUNCTION, never by guessing its return: a bare
callee binds its own name, and a module-qualified one binds the pointer that matches what the
path's reconstructed import puts in scope — the trailing name for a `crate`/`self`/`super`-rooted
path (the import binds it), `module.function` for a bare-rooted one (the import binds the module,
and the core resolves that base through the qualifier table). Where the path crosses into type
space first (`crate::plugin::RootSink::default`), the type is what ends up in scope, so the
pointer is `RootSink.default` — the same split `emit_path` makes for the import itself. Member
accesses whose base is typed but whose own type is not locally knowable emit a one-hop
dotted POINTER qualifier — `low.context_separator.into_bytes()` → `LowArgs.context_separator`,
`Builder::new().opt(x)` → `Builder.new` — which the core resolves hop by hop through the
facts (and also credits the *yielded type* with a Read from the site: consuming a value
through a field IS a use of its type). Fact-first ordering: pointer, then the TypeEnv's
direct type, then the opaque receiver (duck fallback).

**The language's own generics (`builtin_member_types`)** — `Result<T, E>::map_err` still yields a
`Result` over the same `T`; a `Vec<T>` iterates to its `T`. Those are facts about types whose
declaration lives in the standard library, so no file here can emit them and they are declared on
the descriptor instead. Two names in that table are this adapter's own choice and mean nothing to
the core, which sees them as ordinary members: `@element` is the hop an iteration takes (a
container that declares none simply does not type its loop variable — a map iterates to a tuple,
which the model cannot name, and silence beats a confident wrong element), and `@slice` names the
anonymous slice/array type so it can carry an `@element` like any other container. The table is
deliberately small and grows only on measured evidence, the same discipline as the machinery-trait
list — it is a curated set of facts, not a model of the standard library.

**Payload unwrapping (`yields`'s arguments)**: an annotation is recorded as a TREE, not a base
name plus a flat list — `Result<Vec<TreeEntry>, GitError>` keeps the `TreeEntry` two levels down,
which is what a one-level list threw away and could never give back (`Box`/`Rc`/`Arc` are still
looked through, and a lifetime or fn type holds its POSITION as `Unknown` so `?N` keeps indexing
the arguments as written). An unwrapping operation marks its pointer hop
with `?N` — the index of the argument it extracts, landing on a whole subtree. The try operator extracts the success
payload, parameter 0 (`?` is shorthand for `?0`): `let chir = self.config.build_many(x)?`
binds `chir` to `Config.build_many?`, resolved by the core through that parameter instead
of the wrapper. WHICH index an operation projects is this adapter's knowledge — the core
only follows the marker. The `?` is the language's own operator (a fact);
`.unwrap()`/`.expect()` are plain method names and stay unmodeled (a curated
stdlib-semantics fact table is the future step).
Initializer bindings are pointer-first too (`let b = Builder::new()` binds `Builder.new` —
the declared return is the fact, the constructor-name convention only backs mid-chain
bases), `self.field` types through the file's own field facts, and pointer depth caps at
four segments (deeper chains fall to the duck fallback).

Untracked (deliberately, each a future fact-source, not a patch): `.unwrap()`/`.expect()`
(stdlib method semantics), `match`/`if let` bindings (the scrutinee's payload type is not
local), untyped closure params, and multi-bound generics.

## 3. Resolution

The specifier grammar is Rust paths; every step is arithmetic over `ResolveCtx`'s known
file set:

1. **Anchor.** `crate::` → the owning package's crate root: the nearest ancestor directory
   with a `Cargo.toml` whose `src/lib.rs` exists (else `src/main.rs`; both existing prefers
   `lib.rs` — bins re-import through the lib in every workspace kndo has to care about).
   When the convention misses entirely, the manifest's **declared targets** decide
   (`WorkspaceMember::targets`, fed by `[[bin]] path` / `[lib] path`): among the owning
   member's target files whose directory contains the importing file, the deepest wins and
   its directory is the anchor — ripgrep's root manifest declares
   `[[bin]] path = "crates/core/main.rs"`, and without this every `crate::` path in that
   tree was unresolvable and the whole subtree read as dead.
   `self::` → the importing file's own module directory (a *directory-owner* file — `mod.rs`,
   `lib.rs`, `main.rs` — owns its directory; a named file `a.rs` owns child directory `a/`).
   `super::` → one module step up, iterated.
2. **Walk.** Each path segment `s` descends: `<dir>/s.rs`, else `<dir>/s/mod.rs`, then the
   next segment against `<dir>/s/`. All hits `certain`.
3. **Two-step tail.** If the full path resolves to a file, done (module import). Else retry
   with the last segment dropped — the tail was an item; the binding resolves it inside the
   target file. One rule, no name-shape heuristics.
4. **Bare first segment** (no `crate`/`self`/`super`): `std`/`core`/`alloc`/`proc_macro`/
   `test` → `Stdlib`; a workspace member name → `WorkspaceMember` (entry, or a subpath
   walked into its `src/` — deep imports recorded); a declared dependency → `Dependency`.
   Name normalization applies **at lookup, in one direction**: the crate ident's `_` may
   stand for a declared name's `-` (`serde_json` ↔ `serde-json`); the *declared* form is
   what gets returned, so dependency hygiene cross-references correctly.
5. Anything else → `Unresolved`.

**Stdlib** is a hand-maintained five-line list (`std`, `core`, `alloc`, `proc_macro`,
`test`) rather than an `xtask gen-stdlib` pipeline, with the reason recorded: this is the
extern-prelude set, a language-stability guarantee — not toolchain data that drifts with
versions the way `go list std` or `builtinModules` do. If Rust ever adds a sysroot crate,
it is a one-line, reviewed change.

## 4. Manifests & packages (Cargo.toml, RFC 0011)

- Identity: `package.name`; `publish = false` → private (app mode). A pure
  `[workspace]` manifest (no `[package]`) contributes topology only.
- Workspace: `workspace.members` globs (`workspace.exclude` honored).
- Dependencies: `[dependencies]` → `Prod`, `[dev-dependencies]` → `Dev`,
  `[build-dependencies]` → `Build`, `target.'cfg(…)'.dependencies` → flattened into the
  same three. Version req from the string or table form; path deps without `version` get
  `"*"`. Path/workspace deps still register by *name* — sibling resolution is by name
  through the workspace index, same as npm workspaces.
- Roots: `src/main.rs` and `src/bin/*.rs` (autobins) plus `[[bin]] path` entries →
  `Production`, certain, unconditionally (a binary is an entry point, period). The lib
  entry (`src/lib.rs` or `[lib] path`) → `Production` root **only when the package is
  publishable** (library mode, RFC 0011 §5); an unpublished crate's `pub` API must earn its
  keep through actual imports. `build.rs` → `Tooling` root.
- Executables (`ManifestFacts::executables`, RFC 0005 §1's invoked-program rule): every bin
  root also registers under its cargo-assigned **name** — the package name for
  `src/main.rs`, the file stem for autobins, the declared `name` for `[[bin]]` entries.
  This is the identity `env!("CARGO_BIN_EXE_<name>")` invokes it by (see §2), letting a
  test that *executes* the binary — never imports it — reach the binary's whole call tree.
- `resolved_entries`: the lib entry — what a sibling's `use this_crate::…` resolves
  through.
- `declares_surface: false`, always: Cargo has no `exports` map; a Rust crate's surface IS
  its `pub` items, and reaching into `crate_x::internal::y` is legitimate when it's `pub`.
  The `deep-import` gate stays closed **by design**, not by omission.
- `script_invoked_names`: none (no scripts mechanism; `cargo alias`/xtask are out of scope
  for hygiene evidence in v1).

## 5. Cycle policy — where Rust genuinely differs from Go

```
file_cycles:    Idiomatic   (emits nothing)
package_cycles: Idiomatic   (emits nothing)
```

Module-level cycles inside a crate (`a` uses `b`, `b` uses `a`) are legal, common, and
compile fine — RFC 0005 §8's own example of Idiomatic. Package cycles are **not
`Impossible`**, and this is deliberate: cargo forbids cyclic `[dependencies]`, but
`[dev-dependencies]` cycles are legal and idiomatic (a crate dev-depending on a sibling
that depends on it, for integration tests), and kndo's package edges are derived from file
imports — which include test files. `Impossible` would misstate the language (the cycle
is real and legal, not an artifact); `Idiomatic` states the honest stance — and per RFC
0005 §8, a tolerated cycle emits nothing: it is information, not a defect.

## 6. Known hard cases & stances

| Case | Stance |
|------|--------|
| Macros defining items (`macro_rules!` expanding to `pub fn`) | invisible to the static graph; the *macro* stays alive via invocation references, its expansion products do not exist as symbols. Documented recall bound, same class as JS's property-assignment limit |
| Proc-macro crates | ordinary lib crates; `#[derive(X)]` references keep them used |
| Doc-tests (``` blocks in ///) | not parsed in v1 — they exercise the public API, which library mode already roots for publishable crates; for private crates a doc-test-only symbol reads `unused`, accepted |
| Trait-object / generic dispatch (`dyn Trait`, `T: Trait`) | `Implement` references from impls + the member fallback keep implementations alive when the trait is used — the RFC 0012 §3 dispatch rule, unchanged |
| `Deref` coercion method calls | member fallback (`probable`/`possible`), by construction |
| Re-exports of foreign crates (`pub use serde::…`) | dependency stays used; no file alias (nothing in-repo to alias) |
| `#[path]` on `mod` | honored, literal — `#[cfg_attr(cond, path = "…")]` included, and **every** alternate on one declaration is recorded (serde_derive_internals relocates its whole module with two `cfg_attr`s; reading only the bare spelling left the mod resolving to a nonexistent `internals.rs`, which took the module dark and made the `pub use internals::*;` beside it read as an undeclared crate) |
| Bare `use <mod>::…` beside a `#[path]`-relocated `mod <mod>;` | the 2015-edition spelling of `self::<mod>`, expanded to the same declared locations — a local module shadows any extern crate sharing its name, so the relocation is the only reading |
| Same name, `mod x;` and `fn x` | legal Rust; the two-step resolver prefers the module file — accepted approximation |
| Editions | 2018+ path semantics assumed; edition 2015's bare crate-relative paths would read as dependencies → `Unresolved`, degrading to silence, never to a wrong accusation |
| `no_std` crates | nothing special — `core`/`alloc` are in the stdlib list |

## 7. Open questions

1. Should `xtask/**` tooling-role classification instead key off workspace metadata
   (`[workspace.metadata]`) rather than the directory convention? Convention chosen for v1;
   revisit if a real repo uses `xtask` as a lib name.
2. Field-level liveness (`struct` fields as members) — deferred; needs `Field` symbol
   emission plus read/write reference discrimination to be useful, and the noise risk is
   high until then.
3. `cargo metadata`-grade target discovery (`[lib] name`, `autotests = false`, …) — v1
   reads the manifest textually and honors the common keys; the exotic ones degrade to
   convention defaults, recorded here rather than silently.
