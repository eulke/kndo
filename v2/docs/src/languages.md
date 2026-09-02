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
| `kndo:rust` | `rs` | `Cargo.toml` | crate roots (`main.rs`, `lib.rs`, bins, examples, tests, benches), `#[cfg(test)]` |
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

- **Visibility rungs** for `internal-only`: which scope tokens have a narrower
  rung to demote to (`crate` in Rust, `package` in Java, `module` in Kotlin
  and Swift, `export` in TypeScript — where dropping the keyword is checked by
  the compiler).
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
- **Evidence streams**: comments (for `kndo:allow`) and per-function metrics
  (for `duplicate` and `crap`), each declared so their absence is typed and an
  analysis abstains instead of guessing.

## Cross-language reach

One graph: an HTML page reached from a manifest reaches the script it loads,
which reaches the stylesheet it imports; a Swift file reached through a
storyboard is production-reachable; a Go package's files are one unit. Packages
are owned by the nearest manifest, and health can be split per package.

Files no adapter claims (images, data, unknown suffixes) are discovered but
never judged; a claimed file that fails to parse degrades to a diagnostic.
