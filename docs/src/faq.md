# FAQ

## Why is my health score a C?

`kndo health` folds nine category penalties into 0–100 ([the formula](health.md)). The
dominant one on most repositories is `crap` — complexity × missing coverage. If you have
coverage reports, drop the lcov file at `coverage/lcov.info` and kndo ingests it; with none,
every complex function counts as fully untested. `--by-package` shows where the weight sits.

## Does "unused" mean I can delete it?

For `certain`-confidence findings, yes — that is the design bar, and the message states the
evidence. Before a big deletion, `kndo impact <selector> --if-deleted` shows everything that
becomes unreachable with it (and which dependencies you can drop from the manifest while
you're there). For a *library* (publishable package), your public surface is treated as
consumed by definition: exported API reachable through your entry points is never "unused"
just because nothing in-repo calls it.

## How does kndo handle reflection / DI / dynamic dispatch?

By degrading toward silence: dynamic constructs make things *live-possible*, never
*dead-possible*. Known reflective contracts are modeled explicitly per language (test
discovery, serialization hooks, Swift protocol witnesses, `@Override` dispatch), framework
conventions come from [plugins](plugins.md), and what a plugin knows to be externally
consumed (FFI, serialization, an SDK surface) it marks via annotations. Arbitrary computed
names (`Class.forName(prefix + name)`) are honestly unresolvable — code alive only that way
needs a [suppression](suppressions.md) or a plugin.

## My tests run the compiled binary — why is everything "untested"?

`untested` is *static* reachability: a test that spawns your binary as a subprocess (or hits
a server over the network) imports nothing, so no edge exists. That's a
[documented limit](rules.md#untested). The accurate signal for that testing style is
ingested coverage — wire lcov output into `coverage/lcov.info` and judge test blind spots
through [`crap`](rules.md#crap) instead; suppress or baseline the static `untested` findings
if they're noise for you.

## Monorepos?

Workspaces are first-class: package topology from the manifests, per-package library/app
mode, per-package dependency attribution ([`undeclared`](rules.md#undeclared) doesn't excuse
package A because sibling B declares the dep), `version-skew` across members, package-level
rollups ("this whole workspace member is dead"), `deep-import` on declared package surfaces,
and `kndo health --by-package`.

## Why did a finding disappear after I added a test?

`test-only` and `untested` are reachability facts: the moment a test reaches code, its
test-blind-spot finding resolves; if production code is only reachable from tests, it flips
to `test-only` — each verdict states exactly one thing, so fixing one can legitimately
surface the other.

## Why doesn't the exit code change when a plugin reports findings?

Plugin findings are advisory by default — installing a plugin must never break a build. Opt
the ones you trust into the gate with
[`[plugins.gate]`](configuration.md#pluginsgate).

## Is my code sent anywhere?

No. kndo is a local static analyzer: no network, no telemetry, and it never executes the
analyzed project. Plugins run inside a WebAssembly sandbox with no filesystem or network of
their own; even `kndo plugin install` only talks to the GitHub release you name.

## Can I trust the cache?

Yes — by construction. Cache entries are content-addressed: `--no-cache` (or deleting
`.kndo/cache/`) can only make a run slower, never change its findings, and reports are
byte-identical across warm/cold runs and thread counts. If you ever see otherwise, that's a
bug worth reporting.

## What's the fastest way to see why kndo thinks something is alive?

`kndo trace <selector>` prints a concrete root-to-node path with the weakest edge called out
— usually the one import you forgot existed. `kndo describe <selector>` shows the full
picture: color, roots that reach it, degree, metrics, attached findings.

## How do I silence one finding without hiding real problems?

A [`kndo:allow` pragma with a reason](suppressions.md#inline-suppressions), directly above
the declaration. It's scoped, visible in review, and self-cleaning: when the finding it
acknowledges goes away, the pragma itself is flagged `stale` so it never outlives its
purpose.
