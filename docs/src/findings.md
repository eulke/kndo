# Findings

A finding has an **id**, a **category**, a **severity** (`error`, `warning`,
`info`), a **confidence** (`certain`, `probable`, `possible`), a **subject** — a
file, a symbol in a file, a dependency declared by a manifest, or a package —
and a **message** stating the evidence. The id is a function of category and
subject alone, so it survives edits, re-renders and reformatting; suppressions
and the baseline key on it.

Severity says how much it matters to the gate; confidence says how complete the
evidence is. `certain` findings rest on evidence the adapter extracted from the
code itself; `probable` ones rest on a convention (a test-file name, a config
name) or on graph shape where execution data would be stronger.

## Categories

| Category | Subject | Severity | What it means |
|---|---|---|---|
| `unused` | file, symbol, dependency | warning | No root reaches the file; nothing in the project uses the declaration; a declared dependency no claimed file imports. Always `certain`: reachability outranks convention. |
| `test-only` | file, dependency | info | Production-looking code that only tests keep alive: no production or tooling root reaches it, a test does. A production-scope dependency only tests import. |
| `untested` | function (with coverage), file (without) | info | Production-reachable code no test exercises. With an ingested report: the function's records say it never ran (`certain`). Without: the file is reachable from no test root (`probable`); anything a test imports counts as exercised. |
| `crap` | function | info | Change Risk Anti-Patterns: `cc² × (1 − coverage)³ + cc` at or above the threshold (30, configurable). Needs both a complexity stream and an ingested coverage report; otherwise it abstains. |
| `duplicate` | file, function | info | Byte-identical files, or functions whose winnowed fingerprints are equal (renames and re-valued literals included). The first copy in path order is canonical; every other copy is the finding. Each granularity has a floor: a function of fewer than 60 normalized tokens, or a file with fewer than 200 bytes outside its comments (an empty marker, a stub, a license header over one clause), is identical without having been copied and is not judged. |
| `cyclic` | file | warning | An import cycle, one finding per strongly connected component, anchored at its first participant with the shortest loop in the message. Judged only where the language declares cycles a hazard (JavaScript and TypeScript, Python); tolerated elsewhere. |
| `internal-only` | symbol | info | Declared wider than it is used: every use sits in the declaration's own file, and the language has a narrower visibility to demote to (`pub` to private in Rust, `export` dropped in TypeScript, package-private in Java). |
| `private-type-leak` | symbol | warning | An exported callable whose signature names a private type consumers cannot name. |
| `undeclared` | dependency | warning | A reached file imports a package no manifest from its own up to the root declares; the build works through hoisting or a transitive dependency and breaks on a clean install. `certain` only for an unconditional import statement. |
| `unresolved` | file | error | A relative import that points at no file, in a directory the graph knows. Without this category a broken path would surface as `unused` somewhere else. Bare specifiers are never judged here. |
| `version-skew` | dependency | info | One dependency declared with diverging version requirements across the project's manifests. Peer requirements are exempt (contracts, not pins). |
| `stale` | file | warning | A `kndo:allow` that suppresses nothing — unless its category was not judged this run. |

`deep-import` is a reserved category with no analysis behind it: it is deferred
until a corpus repository shows a production-role deep import worth judging,
and it appears in no report.

Extensions report under `ext:<coordinate>/<rule>` (for example
`ext:acme:framework/routes`). Extension findings are advisory — `info` at most
— and do not count toward health, because an extension's evidence is the
extension's claim, not the graph's.

## Abstentions

When an analysis cannot judge, the report says so instead of guessing, with a
scope: the whole run (`no coverage report ingested this run`, `no test root
anchors any file in this graph`) or a set of manifests (files of that
manifest's ecosystem that no adapter claims and that could carry its imports —
a `.vue` component in a JavaScript package). An abstention never lowers
health, and a suppression of an unjudged category never reads as stale.

## Diagnostics

Parse errors, an unreadable file, a plugin rejected at load: the `diagnostics`
block lists them with a path and a level. A parse error degrades that file's
evidence; it never fails the run.
