# kndo

**Find what your codebase no longer needs.**

kndo is a multi-language static analyzer that builds a reference graph of your whole
project — across eight languages at once — and reports what nothing uses anymore:
dead code, unused dependencies, files no import reaches, code only tests keep alive,
duplicated logic, phantom dependencies, and the parts of your production code no test
exercises. It scores overall project health, runs fast enough to be a pre-commit hook,
and speaks JSON, SARIF, and an agent-oriented format so both humans and AI tools can
act on what it finds.

```console
$ kndo
kondo · 312 files · 6 packages · health 87

  unused      src/legacy/retry.ts — unreachable: no root or import reaches it
  untested    src/billing/proration.ts#applyCredit — production-reachable, no test reaches it
  undeclared  redis — imported but not declared in api's manifest
  …

12 findings · 3 warning · 9 info
```

## Highlights

- **One graph, eight languages** — JavaScript/TypeScript, Rust, Go, Java, Kotlin,
  Swift, JSON, and CSS/SCSS analyzed together, monorepos included.
- **Reachability, not regex** — findings come from resolving imports, references,
  visibility, and dispatch across files and packages. Dynamic dispatch, framework
  callbacks, and serialization machinery are modeled, so the noise stays low.
- **Thirteen finding categories** — from `unused` and `untested` to structural
  `duplicate` detection, dependency hygiene, cycles, and a coverage-aware `crap`
  metric for complex-and-untested functions.
- **Fast and incremental** — a warm cache re-analyzes only what changed;
  `kndo check --staged` gates a commit in well under a second on typical diffs.
- **CI-native** — a GitHub Action with a sticky PR comment, file annotations,
  SARIF for code scanning, and diff-aware budgets.
- **Extensible** — install WebAssembly plugins (or write your own with
  `kndo plugin new`) to teach kndo framework conventions and new languages.
- **Agent-friendly** — graph navigation verbs (`find`, `describe`, `uses`,
  `used-by`, `trace`, `impact`) answer structural questions with JSON envelopes.

## Install

Download a release binary from [GitHub Releases](https://github.com/eulke/kondo/releases),
or build from source:

```console
$ cargo install --path crates/kndo-cli
```

## Quick start

```console
$ cd your-project
$ kndo init          # optional: writes kndo.toml, offers a pre-commit hook
$ kndo               # full analysis, human-readable report
$ kndo check --staged --fail-on warning   # what a commit gate runs
$ kndo health --by-package                # the score, broken down
```

In CI:

```yaml
- uses: eulke/kondo/action@main
  with:
    fail-on: warning
```

## Documentation

The full documentation lives in [`docs/`](docs/src/SUMMARY.md) — installation,
every finding category, configuration, suppressions, CI setup, graph navigation,
plugin authoring, and per-language notes.

## License

Apache-2.0 — see [LICENSE](LICENSE).
