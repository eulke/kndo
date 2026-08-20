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
— dead API kept warm by its own showcase. `#[cfg(test)]` modules inside production files do
NOT flip the file's role (role is per-file); their `#[test]` functions become in-source test
roots (§2) and the reachability coloring takes it from there — a symbol only they reach
colors `test-only`, which is the truth.

## 2. Extraction

**Declarations** — `fn`, `struct` (+ named fields? no — fields are not independent liveness
units in v1), `enum` + variants (variants as `EnumMember` members of the enum, RFC 0012 §3),
`union`, `trait` (+ its method signatures as members of the trait), `impl` methods and
associated consts/types (members of the **self type**'s name; `impl Trait for T` methods are
members of `T` and additionally emit an `Implement` reference to `Trait`), `const`,
`static`, `type` aliases, `macro_rules!` (kind `Other("macro")`). Inline modules
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
- `pub(super)` / `pub(in path)` → level 1 as well, deliberately **widened**: mapping them
  down to `File` would fabricate `internal-only`/leak findings; widening to the crate rung
  only ever silences, never accuses. Recorded as the conservative direction.
- `pub` → level 2. `exported` = any `pub*` form.

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
| `#[no_mangle]` / `#[export_name]` / `pub extern "C" fn` | in-source `Production` root at `probable` — an FFI consumer exists outside the graph |
| `#[test]` / `#[bench]` | in-source `Test` root at `certain` — the runner is the consumer |
| `#[cfg(…)]` | **both branches kept**, always: kndo analyzes the source, not one compilation; over-approximating alive is the safe direction. Two cfg-gated same-name items collapse to last-wins in the symbol table (documented artifact, harmless for liveness) |

**Metrics** (`MetricsSyntax` as data): branches `if_expression`, `match_arm` (n-way match ≈
n−1 branches plus the base — counted per arm past the tree shape), `while_expression`,
`for_expression`, `try_expression` (`?` is an early-return branch), `&&`, `||`;
identifiers → `identifier`, `field_identifier`, `type_identifier`,
`shorthand_field_identifier`; literals → string/raw-string/char/byte/integer/float
literals; skipped → `line_comment`, `block_comment`. Winnowing parameters shared (toolkit).

**Suppressions** — `// kndo:allow …` via the toolkit scanner, like every language.

## 3. Resolution

The specifier grammar is Rust paths; every step is arithmetic over `ResolveCtx`'s known
file set:

1. **Anchor.** `crate::` → the owning package's crate root: the nearest ancestor directory
   with a `Cargo.toml` whose `src/lib.rs` exists (else `src/main.rs`; both existing prefers
   `lib.rs` — bins re-import through the lib in every workspace kndo has to care about).
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
