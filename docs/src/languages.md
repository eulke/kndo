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
| `kndo:rust` | `rs` | `Cargo.toml` | crate roots (`main.rs`, `lib.rs`, bins, examples, tests, benches); by dispatch rule: `#[test]`, `#[bench]`, `#[cfg(test)]`, `#[no_mangle]` and the other linkage attributes, `#[tokio::main]`-style entries; `#[allow(dead_code)]` and kin exempt |
| `kndo:go` | `go` | `go.mod` | `package main`, `_test.go`; a package is one unit |
| `kndo:java` | `java` | `pom.xml`, `build.gradle`, `build.gradle.kts`, `settings.gradle`, `settings.gradle.kts` | `main` methods, test sources, framework annotations through extensions |
| `kndo:kotlin` | `kt` | the same JVM manifests | `main` functions, test sources |
| `kndo:python` | `py` | `pyproject.toml`, `requirements.txt`, `requirements-*.txt` | scripts, `__main__`, test files, entry points |
| `kndo:swift` | `swift` | `Package.swift` | executable targets, `@main`, test targets |
| `kndo:html` | `html` `htm` | — | a document is its own root; `<script src>`, `<link href>` and inline module imports are its references |
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

- **The visibility ladder** for `internal-only`: the reaches a language can
  spell, narrowest first, each under that language's own word for it — Java
  declares `private`/`package`/`public`, so a package-scoped name used only in
  its own file has a rung to fall to. The engine judges by the rung and reports
  by the word, which is why the advice says "declared `package`-scoped" rather
  than naming the engine's own vocabulary. An adapter still on the older
  spelling declares the scope tokens that have somewhere narrower to go
  (`crate` in Rust, `module` in Kotlin and Swift, `export` in TypeScript —
  where dropping the keyword is checked by the compiler).
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
