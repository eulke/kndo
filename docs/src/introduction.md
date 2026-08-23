# kndo

**Find what your codebase no longer needs.**

Codebases accumulate waste faster than ever. Refactors leave orphaned files behind,
dependencies outlive the feature that needed them, copy-paste spreads, and AI coding agents
generate plausible code that nothing actually uses. Each leftover is cheap; the aggregate is
expensive — slower builds, slower onboarding, misleading search results, and agents that read
dead code as if it were live and compound the mess.

kndo is a **single, fast, multi-language static analyzer** focused on waste detection and
project health, not style. One binary, one optional config file, one output schema.

```console
$ kndo
◦ unused (dependency)  date-fns is declared but never imported
◦ unused (function)    src/billing/tax.ts#calcVAT is unreachable: nothing references it
▲ cyclic (file)        4 files form an import cycle (src/a.ts → src/b.ts → …)

health   82.4  B
```

## What makes it different

- **Fast enough to never skip.** Warm incremental runs finish in well under a second on large
  repositories, so kndo lives comfortably in a pre-commit hook. Analysis facts and graph
  snapshots are content-addressed and cached under `.kndo/cache`; a stale cache can only ever
  cost you a colder run, never a wrong answer.
- **Multi-language, one vocabulary.** JavaScript/TypeScript, Go, Rust, Java, Kotlin, Swift,
  JSON and CSS — analyzed together, reported together. A TypeScript file importing a CSS file
  is one graph. Every language reports the same categories, severities, and confidence tiers.
- **Honest about uncertainty.** Every finding carries a confidence level (`certain`,
  `probable`, `possible`). Dynamic constructs lower confidence instead of producing false
  "definitely dead" claims — the accusing rules aim for zero false positives, and where static
  analysis genuinely cannot know, kndo degrades toward silence, never toward accusation.
- **Changes are judged by their blast radius.** `kndo check --diff main` analyzes both sides
  of your change and reports what it *introduces* and what it *fixes* — including a distant
  symbol your edit just orphaned in a file you never touched.
- **Humans, CI, and agents are all first-class.** A readable terminal report, versioned JSON
  and SARIF, a sticky PR comment via the GitHub Action, and a token-frugal `--format agent`
  plus graph-navigation verbs designed for LLM agents.
- **Extensible, safely.** Framework conventions (routes, entry-point files, serialization
  hooks) come from plugins — built-in ones activate automatically when your manifests declare
  the framework, and third-party ones run as sandboxed WebAssembly components with no
  filesystem or network access of their own.

## The mental model

kndo builds one **project graph**: files, symbols, packages, declared dependencies, and the
edges between them (imports, references, framework conventions). Entry points — a manifest's
`main`/`exports`, a `func main`, a test file, a framework route — are **roots**. Reachability
from those roots, with confidence attached to every edge, is what most verdicts are computed
from: `unused` is "no root reaches this", `test-only` is "only test roots reach this",
`untested` is "production roots reach this, test roots never do".

Everything else in this book hangs off that model: the [findings](rules.md) are verdicts over
the graph, the [health score](health.md) is a weighted summary of them, the
[navigation verbs](navigation.md) let you (or your agent) walk the same graph interactively,
and [plugins](plugins.md) teach the graph the edges a framework hides.

## Where to start

- [Installation](install.md), then [Getting started](getting-started.md).
- [Findings reference](rules.md) explains every verdict kndo can reach, with limits stated
  honestly.
- [CLI reference](cli.md) documents every command, flag, format, and exit code.
- Running in CI? See [the GitHub Action](ci.md).
- Building with agents? See [For agents](agents.md) and [Graph navigation](navigation.md).
- Curious what your framework gets for free? See [Plugins](plugins.md).
