# Languages

Each adapter claims files by suffix, reads the ecosystem's manifests for roots
and dependencies, and extracts evidence — declarations, imports, references,
comments, per-function metrics — into one vocabulary the engine judges without
knowing which language it came from. The engine never names a language:
everything a language needs is declared on the adapter's spec, with a named
consumer in the engine.

| Adapter | Suffixes | Manifests and launchers | Roots |
|---|---|---|---|
| `kndo:js-ts` | `ts` `tsx` `js` `jsx` `mjs` `cjs` `mts` `cts` | `package.json`; `.github/workflows/*.yml`, `action.yml` | manifest entries (`main`, `module`, `browser`, `bin`, `exports`, `imports`), files handed to a runtime by npm scripts and by workflow or action steps (`node`, `tsx`, `ts-node`, `bun`, `deno`), test files, config files, shebangs |
| `kndo:rust` | `rs` | `Cargo.toml` | every cargo target as a unit entered through its own file (lib, bins, tests, benches, examples, build script), with `publish = false` read as "no consumer outside"; by dispatch rule: `#[test]`, `#[bench]`, `#[cfg(test)]`, `#[no_mangle]` and the other linkage attributes, `#[tokio::main]`-style entries; `#[allow(dead_code)]` and kin exempt |
| `kndo:go` | `go` | `go.mod` | `package main`, `_test.go`; a package is one unit |
| `kndo:java` | `java` | `pom.xml`, `build.gradle`, `build.gradle.kts`, `settings.gradle`, `settings.gradle.kts` | `main` methods, test sources, framework annotations through extensions; a `pom.xml` module is two units, its main set and its test set, the test set compiling against main and a friend of it; the test set is what the pom states (its own `<testSourceDirectory>`, else the nearest parent's along `<parent>`, plus the build helper's added test sources), and every file in it is a test root by the unit's kind |
| `kndo:kotlin` | `kt` | the same JVM manifests | `main` functions, test sources; `internal` reaches the unit and its friends |
| `kndo:python` | `py` | `pyproject.toml`, `requirements.txt`, `requirements-*.txt` | scripts, `__main__`, test files, entry points |
| `kndo:swift` | `swift` | `Package.swift` | executable targets, `@main`, test targets |
| `kndo:html` | `html` `htm` | — | a document is its own root; `<script src>` and `<link href>` are its references; an inline `<script>` or `<style>` is a region of JavaScript or CSS, read by that adapter |
| `kndo:css` | `css` `scss` | — | `@import`, `@use`, `@forward`, resolved with Sass partial and index conventions |

Two more extensions ship for Apple projects: `kndo:interface-builder` roots
the classes storyboards and xibs instantiate at run time, and `kndo:info-plist`
roots the principal class and app delegate an `Info.plist` names. Four
[coverage ingesters](health.md) read lcov, Cobertura, JaCoCo and Go
coverprofile reports.

## What each language declares

Beyond suffixes and manifests, an adapter declares the facts the engine's
judgments depend on — with a default that keeps the judgment silent where the
adapter says nothing:

- **Reach**: how far each declaration's name legally reaches, as the
  language spells it — its owner (a `private` member), its file (a top-level
  `private`, an ES declaration without `export`), its namespace or an ancestor
  of it (Java's package-private, a Rust item without `pub`, `pub(super)`), its
  unit (`internal`, `pub(crate)`) or the group of units one manifest aggregates (Swift's
  `package`), a directory (an exported Go name under `internal/`, fenced at
  that directory's parent), a namespace by name (Rust's `pub(in crate::a)`),
  its owner and the owner's subtypes (`protected`; Java's adds the package),
  its owner's exactly (a Rust trait item), or exported. The engine pools by
  the *effective* reach — the declared one after every owner above caps it,
  so a public member of a file-private class reaches the file and is never
  handed out by a published surface — and `describe` shows both.
- **The visibility ladder** for `internal-only`: the reaches a language can
  spell, narrowest first, each under that language's own word for it and,
  where a keyword exists for members or for top-level declarations alone,
  saying so — Java declares `private` (members), `package-private`,
  `protected` (members), `public`; Kotlin `private` twice (the class on a
  member, the file on a top-level declaration), `protected` (members),
  `internal`, `public`; Rust `private`, `pub(crate)`, `pub`;
  TypeScript `unexported` (top level) and `export`. The engine judges by the
  rung and reports by the word: a package-private member used only inside its
  class reads "`private` would suffice", a `pub(crate)` function used only in
  its file "`private` would suffice", and a language that spells nothing
  between the declared rung and the one the uses need — Go below its package,
  Python anywhere — gets no advice at all.
- **The published surface** for that analysis's Exported rung: whether a unit
  publishes every export (a jar, a crate, a Go package, a Python distribution
  — the default, under which an exported declaration is never advised to
  narrow) or only what its entries export (npm, where `main`/`exports` decide,
  so an `export` in a file no entry reaches is advised `unexported` when
  nothing else in the tree names it). Whether a unit publishes at all is the
  manifest's: a library does unless it says otherwise, an executable or a test
  set never. `used-by` shows an export kept by its unit's publication as
  `published`.
- **Ignores**: the paths the language's own tool never compiles — a Go file
  whose name begins with `_` is in no package, Go's `vendor` holds copies of
  other modules, npm never builds `node_modules`, and the interpreter's
  `site-packages` holds installed packages, never the project's. A file
  under one is discovered, so an import pointing at it is not broken, and
  never claimed: no evidence, no unit, no verdict — and a manifest under one
  (a dependency's `package.json`, a vendored module's `go.mod`) declares
  nothing about the project. The rule is the tool's own, measured: a Go
  `testdata` or `_`-prefixed directory is not one, since `./...` skips it
  but an import compiles it (gin imports its `testdata/protoexample` from
  three test files); an output directory a JVM or Cargo build writes is not
  one, since a package may be named `target` or `build`; and what a manifest
  excludes is that unit's membership, not the tree's.
- **Includes**: where a language pastes one file's content into another
  (Rust's `include!`), the import says so and the engine reads it as sight: the
  including file sees every name the included one declares, whatever reach it
  carries, and nothing is handed out — what the includer never names is still
  dead.
- **Mounts**: where a language names its namespaces by ATTACHING files to
  each other rather than by a clause each file writes, the import that does it
  says so — Rust's `mod x;` makes `x.rs` the child namespace `x` of the
  mounting module's. The engine reads the tree that results: a name private to
  a module is readable in every file mounted under it, an address that climbs
  (`pub(super)`, `pub(in crate::a)`) names the node it climbs to, and the
  mount's own reach fences everything below it, so a `pub` item of a privately
  mounted module is nameable in the mounting namespace and nowhere else — off
  the unit's published surface, and judged like any other bounded name.
- **Embedded regions**: the spans of a file written in another language.
  HTML reports each inline `<script>` — a module, or a classic script — and
  each inline `<style>` as a region of JavaScript or CSS, and the engine hands
  it to that adapter, which reads it as it reads a file, in the page's
  coordinates: a function the inline module never calls is the page's
  `unused` finding, its imports resolve as JavaScript's (an extensionless
  `./src/util` finds `util.ts`, a bare name is a package), and an `@import`
  in the inline style is the stylesheet's edge. A classic script's top-level
  declarations are the page's globals — reachable from every other script and
  handler attribute on it — and stay alive; a script whose type is data (an
  import map, JSON, a template) is no region at all.
- **Cycle tolerance** for `cyclic`: a hazard in JavaScript, TypeScript and
  Python (initialization order bites at run time), tolerated in Rust, Go, Java,
  Kotlin and Swift (the compiler or the package model makes cycles benign).
- **Dependency identity**: how an import specifier names a declared dependency
  — the package name (`lodash/fp` is `lodash`), the crate root, the module
  path — and the platform's own modules that are never a dependency to
  declare (Node's built-ins; Go's standard library by the rule that its import
  paths have no dot in their first segment).
- **Dependency scoping**: whether the ecosystem's manifests separate
  production from development declarations (npm, Cargo) or not (Go).
- **Importers outside the claim**: file families that carry the ecosystem's
  imports without being its source — `.vue`, `.svelte`, `.astro`, `.mdx`,
  `.html`, `.css`, … for JavaScript — so a dependency judgment abstains when
  such files exist and no adapter reads them, rather than accuse.
- **Dispatch rules**: what the language's markers — attributes, annotations,
  decorators, pragmas — mean. An adapter reports every marker as evidence,
  path and arguments as written; its spec's rules say which ones root an entry
  of which color and which exempt a declaration from `unused`. Rust's rules
  make `#[test]`, `#[tokio::test]`, `#[bench]` and a `cfg(test)` gate Test
  roots, `#[no_mangle]`, `#[global_allocator]` and their kin Production roots,
  and honor `#[allow(dead_code)]`, `#[expect(dead_code)]` and the `unused` and
  `warnings` groups as exemptions — lexically scoped, and a file-level
  `#![allow(dead_code)]` is reported as a diagnostic so the silence is visible.
  `used-by` shows a dispatched root as `dispatch:<color>` and an exemption as
  `exempt`.
- **Relations**: the types a declaration promises to be (`extends`,
  `implements`, a protocol conformance). The engine reads one stream both ways:
  a member whose owner promised a type declaring the same name is a WITNESS and
  is kept — no call site can be required to exist, because every caller holds
  the supertype — and a member some subtype declares is OVERRIDDEN, so
  `internal-only` never advises narrowing it. `used-by` shows the first as
  `witness`.
- **Signatures**: what the language reads beyond the identifier to tell
  same-named declarations apart, as it spells it — Java's parameter types
  `(int, List<String>)`. It is part of a declaration's address, so an overload
  has its own finding identity and its own query address; a language that
  states none (Kotlin and Swift, until their adapters do) gets `#2` on the
  second of two the engine cannot otherwise split.
- **Evidence streams**: comments (for `kndo:allow`), per-function metrics
  (for `duplicate` and `crap`), markers (for dispatch), relations (for
  witnesses) and qualifiers — whether each reference was written ON something,
  which is how `internal-only` tells `queue.head` from somebody else's local
  named `head`. Each is declared, so its absence is typed and an analysis
  abstains or keeps its pre-stream answer instead of guessing.

## Cross-language reach

One graph: an HTML page reached from a manifest reaches the script it loads,
which reaches the stylesheet it imports; a Swift file reached through a
storyboard is production-reachable; a Go package's files are one unit. Packages
are owned by the nearest manifest, and health can be split per package.

Files no adapter claims (images, data, unknown suffixes) are discovered but
never judged; a claimed file that fails to parse degrades to a diagnostic.
