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
| Role `test` | `tests/**` (integration tests), `benches/**`, `examples/**` |
| Role `tooling` | `build.rs`, `.cargo/**`, `xtask/**` (the de-facto task-runner convention) |
| Origin `generated` | first-64-lines markers via the toolkit's `ContentMarkers` (`@generated`, `Code generated`, `Automatically generated`) — covers bindgen/prost/tonic banners. Build-script output lives in `OUT_DIR`, outside the tree — no stance needed |
| Origin `vendored` | `vendor/**`, `third_party/**` (toolkit universal list) |

`examples/**` is `test` deliberately: an example consumes the public API from outside like a
test does, and a symbol alive *only* through its own demo is exactly the `test-only` verdict
— dead API kept warm by its own showcase.

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
[ File "private", Package "pub(crate)", Public "pub" ]
```

- no `pub` → level 0. True Rust privacy is module-and-descendants; under file ≈ module that
  is `File` scope. Descendant files reaching a parent's private item is real Rust and will
  read as a `possible`-confidence resolution miss, not a false `unused` — accepted, rare.
- `pub(crate)` → level 1 (`Package` scope — crate = kndo package, exactly).
- `pub(self)` → level 0 — it IS `private`, spelled long.
- `pub(super)` on an item **inside an inline mod** → level 0, not exported: `super` of an
  inline mod is a module within this same file, so under file ≈ module the item never
  leaves the file (widening it to the crate rung fabricated `internal-only` on ripgrep's
  `mod convert { pub(super) fn … }` — M6 residuals).
- Top-level `pub(super)` / `pub(in path)` → level 1, deliberately **widened**: `super` of
  the file's own module leaves the file, and mapping these down to `File` would fabricate
  `internal-only`/leak findings; widening to the crate rung only ever silences an
  `internal-only`, never accuses. Recorded as the conservative direction — with one known
  residual on the other side: `private-type-leak` can pair a widened `pub(super)` subject
  with a genuinely narrower type from the *parent* module (ripgrep's
  `parse.rs#lookup(… dyn Flag)`) and accuse where real Rust visibility is coherent.
  Accepted until rungs are module-relative rather than file-relative.
- `pub` → level 2. `exported` = any `pub*` form except `pub(self)` and
  inline-mod `pub(super)`.

**Unit:** `None`. Rust files never resolve each other's names implicitly — everything
travels through `use` or a qualified path — so Go's `unit` machinery stays off.
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
| `mod foo;` | specifier `self::foo`, `side_effect_only: true` — the file-linking edge (§0); `#[path = "…"]` on the `mod` overrides the conventional location with the literal path |
| `extern crate name;` | specifier `name` (resolves as a dependency/stdlib like any bare first segment) |

`ImportKind::Relative` for `crate::`/`self::`/`super::` paths, `Package` for bare-first-
segment paths. All `use` edges are `certain` — Rust has no bundler ambiguity.

**Dynamic constructs** — macros, with a deliberately bounded stance:

| Construct | Effect |
|-----------|--------|
| `name!(…)` invocation | a `certain` reference to `name` (keeps `macro_rules!` alive), plus a scan of the token tree: `a::b` token runs are reconstructed and routed through the body-path rule — `print!("{}", render::render_query(x))` binds `render_query` through the `use`-established qualifier exactly as it would outside the macro — and lone identifier tokens stay plain reads (`format!("{}", user)` keeps `user`'s referents alive). String literals inside token trees are scanned for Rust 2021 **inline format captures**: `format!("v{VERSION}")` reads `VERSION` (`{{` escapes and positional `{}`/`{0}` contribute nothing). **No wildcard per macro** — that would drown every Rust file in `possible` edges |
| `include!("lit")` / `include_str!` / `include_bytes!` with a literal | `probable` file edge |
| `env!("CARGO_BIN_EXE_<name>")` / `option_env!` | `invoked_executables` fact: Cargo's own handshake for "this test executes the workspace binary `<name>`" — the core resolves the name through the manifest's named bins (§4) and emits an `InvokesFile` edge, RFC 0005 §1's invoked-program rule. Only the documented prefix, only a literal |
| `#[no_mangle]` / `#[export_name]` / `pub extern "C" fn` | in-source `Production` root at `probable` — an FFI consumer exists outside the graph |
| `#[test]` / `#[bench]` | a recorded test region (§1) — assembly derives the `certain` `Test` root from span containment; extraction emits no root of its own |
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
`(owner, member, yields)` fact, dispatch-reduced, `Self` resolved to the owner. Member
accesses whose base is typed but whose own type is not locally knowable emit a one-hop
dotted POINTER qualifier — `low.context_separator.into_bytes()` → `LowArgs.context_separator`,
`Builder::new().opt(x)` → `Builder.new` — which the core resolves hop by hop through the
facts (and also credits the *yielded type* with a Read from the site: consuming a value
through a field IS a use of its type). Fact-first ordering: pointer, then the TypeEnv's
direct type, then the opaque receiver (duck fallback).

**Payload unwrapping (`yields_params`)**: a parameterized annotation also records its type
arguments, in order (`Result<ConfiguredHIR, Error>` → params `[ConfiguredHIR, Error]`;
`Box`/`Rc`/`Arc` are looked through), and an unwrapping operation marks its pointer hop
with `?N` — the index of the parameter it extracts. The try operator extracts the success
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
file_cycles:    Idiomatic   (info)
package_cycles: Idiomatic   (info)
```

Module-level cycles inside a crate (`a` uses `b`, `b` uses `a`) are legal, common, and
compile fine — RFC 0005 §8's own example of Idiomatic. Package cycles are **not
`Impossible`**, and this is deliberate: cargo forbids cyclic `[dependencies]`, but
`[dev-dependencies]` cycles are legal and idiomatic (a crate dev-depending on a sibling
that depends on it, for integration tests), and kndo's package edges are derived from file
imports — which include test files. Declaring `Impossible` would suppress a real, visible,
legal structure; `Idiomatic` reports it at `info`, which is what it deserves.

## 6. Known hard cases & stances

| Case | Stance |
|------|--------|
| Macros defining items (`macro_rules!` expanding to `pub fn`) | invisible to the static graph; the *macro* stays alive via invocation references, its expansion products do not exist as symbols. Documented recall bound, same class as JS's property-assignment limit |
| Proc-macro crates | ordinary lib crates; `#[derive(X)]` references keep them used |
| Doc-tests (``` blocks in ///) | not parsed in v1 — they exercise the public API, which library mode already roots for publishable crates; for private crates a doc-test-only symbol reads `unused`, accepted |
| Trait-object / generic dispatch (`dyn Trait`, `T: Trait`) | `Implement` references from impls + the member fallback keep implementations alive when the trait is used — the RFC 0012 §3 dispatch rule, unchanged |
| `Deref` coercion method calls | member fallback (`probable`/`possible`), by construction |
| Re-exports of foreign crates (`pub use serde::…`) | dependency stays used; no file alias (nothing in-repo to alias) |
| `#[path]` on `mod` | honored, literal |
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
