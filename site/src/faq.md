# FAQ

## Why is my health score a C?

`kndo health` folds nine category penalties into 0–100. The dominant one on most repos is
`crap` — complexity × missing coverage. If you have coverage reports (lcov et al), kndo
ingests them and the score reflects reality; with none, every complex function counts as
untested. `--by-package` shows where the weight is.

## Does "unused" mean I can delete it?

For `certain`-confidence findings, yes — that is the design bar, and the message says exactly
what evidence supports it. For a *library* (publishable package), your public surface is
treated as consumed by definition: exported API reachable through your entry points is never
"unused" just because nothing in-repo calls it.

## How does kndo handle reflection / DI / dynamic dispatch?

By degrading toward silence: dynamic constructs make things *live-possible*, never
*dead-possible*. Known reflective contracts get modeled explicitly (JUnit/XCTest discovery,
`Serializable` hooks, Swift protocol witnesses, `@Override` dispatch); what a plugin knows to
be externally consumed (serialization, FFI) it can mark via `annotate_symbols`.

## Monorepos?

Workspaces are first-class: package topology from the manifests, per-package library/app mode,
cross-package edges, `version-skew` across members, package-level rollups ("this whole
workspace member is dead"), and `kndo health --by-package`.

## Is my code sent anywhere?

No. kndo is a local static analyzer: no network, no telemetry, and it never executes the
analyzed project. Plugins run inside a WASM sandbox with no filesystem or network of their own.

## Why did a finding disappear after I added a test?

`test-only` and `untested` are reachability facts: the moment a test reaches code, its
test-blind-spot findings resolve; if production code is only reachable from tests, it flips to
`test-only` — each verdict states exactly one thing.
