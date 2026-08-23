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
  repos, so kndo lives comfortably in a pre-commit hook.
- **Multi-language, one vocabulary.** JavaScript/TypeScript, Go, Rust, Java, Kotlin, Swift,
  JSON and CSS — analyzed together, reported together. A TS file importing a CSS file is one
  graph.
- **Honest about uncertainty.** Every finding carries a confidence level. Dynamic constructs
  lower confidence instead of producing false "definitely dead" claims — the accusing rules
  aim for zero false positives, and the FP hunt across real repos (express, gin, ripgrep,
  junit4, moshi, Alamofire) is part of the release process.
- **Changes are judged by their blast radius.** `kndo check --diff main` reports everything
  your change *affects* — including a distant symbol your edit just orphaned.
- **Humans, CI, and agents are all first-class.** A readable terminal report, versioned JSON
  and SARIF, a sticky PR comment via the GitHub Action, and a token-frugal `--format agent`
  plus graph-navigation verbs designed for LLM agents.

## Where to start

- [Installation](install.md), then [Getting started](getting-started.md).
- [The rules](rules.md) explains every verdict kndo can reach.
- Running in CI? See [the GitHub Action](ci.md).
- Building with agents? See [For agents](agents.md).
