# kndo

kndo reads a repository the way a build does — from its entry points outward —
and reports what nothing reaches, what only tests keep alive, what no test
exercises, what is declared wider than it is used, and what the manifests get
wrong. It covers TypeScript and JavaScript, Rust, Go, Java, Kotlin, Python,
Swift, HTML and CSS in one run, across one dependency graph, so a stylesheet
reached only through a page reached only through a script is still reached.

Three properties hold everywhere:

- **Deterministic.** The same tree produces byte-identical output, at any
  thread count, on any machine, cached or not. Two runs that differ are a bug,
  and the test suite gates on it.
- **Evidence, never guesses.** Every finding names its subject, its category,
  a severity, and a confidence — `certain` when the evidence is complete,
  `probable` when a convention supplied it. When an analysis cannot judge — no
  coverage report, no test roots, a file family no adapter claims — it says
  so in the report as an *abstention* instead of accusing.
- **A ratio, not a score.** Health is how much of the judged graph the
  findings implicate: two counted integers, no weights, no clock, unaffected
  by how much you baseline.

```text
$ kndo
warning unused scripts/orphan.ts: no root anchors this file and no reachable file imports it
1 finding
health 90.0 · implicated 1 of 10 · unused 1
abstained: untested — no test root anchors any file in this graph
abstained: crap — no coverage report ingested this run
```

The rest of this book is the shipped surface: [installing](install.md) the
binary, the [command line](cli.md), what each [finding](findings.md) means,
how [health](health.md) is measured and how coverage reports sharpen it,
[configuration](configuration.md), [suppressions and the baseline](suppressions.md),
running kndo in [CI](ci.md), reading the graph with the
[navigation verbs](navigation.md) and from [agents](agents.md), what each
[language adapter](languages.md) reads, and how to write an
[extension](extensions.md).
