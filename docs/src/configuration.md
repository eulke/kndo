# Configuration

`kndo init` writes `kndo.toml` at the project root. Everything in it is optional
and every key has a living consumer — a key the engine does not read is not in
the template, and an unknown key **refuses the run** (exit 2) so a typo is never
silently ignored.

```toml
# kndo.toml — invocation defaults for this project.
# Flags beat KNDO_FORMAT beats this file beats built-in defaults.
# An unknown key refuses the run: a typo is never silently ignored.

[check]
# Lowest severity that fails the run: error | warning | info | never.
#fail-on = "warning"

# Report format when neither --format nor KNDO_FORMAT says:
# human | json | agent | sarif.
#format = "human"

# Judge only these categories (or use `skip` for the complement).
#only = ["unused"]
#skip = ["duplicate"]

[analysis.crap]
# CRAP score (complexity² × (1 − coverage)³ + complexity) at or above which a
# function is a finding; the metric's own line is 30.
#threshold = 30
```

There is one merge site: a flag beats `KNDO_FORMAT`, which beats the file,
which beats the default. `kndo doctor` prints the configuration as parsed; a
broken file is doctor's diagnosis, never its crash.

## The pre-commit hook

```sh
kndo init --hook
```

also writes `.git/hooks/pre-commit` running `kndo check --staged`: the commit
is gated on what it would commit, against `HEAD`, with the same `fail-on` as
everywhere else. An existing hook is left alone with a note to add the line
yourself.

## What is not configuration

Which files are analyzed is decided by the tree: `.gitignore` and `.ignore`
are honored exactly as git and ripgrep honor them, and nothing else — no global
excludes, no `.git/info/exclude`, no ignore files above the root — so two
checkouts of one tree discover the same files. What a language's own tool
never compiles — Go's `vendor` copies, npm's `node_modules`, the interpreter's
`site-packages` — is discovered and never judged, because the adapter
declares it (see [Languages](languages.md)); add an `.ignore` for the rest git
tracks but kndo should not judge (a fixture corpus, a frozen copy). Hidden
entries are skipped, except a dot-directory an adapter reads launchers or
manifests from (`.github/`, for the JavaScript adapter's workflow rule).

Entry points are not configured either: they come from manifests
(`package.json` entries and scripts, `Cargo.toml`, `go.mod`, `pom.xml`,
`Package.swift`, …), from launchers (workflow and action steps), from
conventions (test files, config files, shebangs), and from
[extensions](plugins.md) that know a framework. When something is reported
unused because only a framework reaches it, the fix is an extension or a
[suppression](suppressions.md) with the reason written down — not a list of
paths in a config file that no one can verify.
