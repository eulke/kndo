# Getting started

## First run

From any project root:

```console
$ kndo
```

That is `kndo check` in full-scan mode: every adapter claims its files, the project graph is
assembled, and every rule runs. On a terminal you get the human report; when piped, stdout is
pure JSON (`kndo | jq .findings` just works). No configuration is required — manifests
(`package.json`, `go.mod`, `Cargo.toml`, `pom.xml`, `build.gradle`, `Package.swift`) tell kndo
what your entry points and dependencies are.

Exit codes: full mode defaults to `--fail-on none` (exploratory — a legacy repo's pre-existing
findings shouldn't fail a plain `kndo`), diff modes default to `--fail-on warning` (a gate
should gate). `0` clean · `1` findings at/above the threshold · `2` kndo itself could not run.

## Adopting on an existing codebase

Acknowledge everything that exists today, then keep new waste out:

```console
$ kndo baseline
kndo: baseline written — 412 findings acknowledged (.kndo/baseline.json)
```

Baselined findings stop appearing in reports and never fail a run. When one is actually fixed,
`kndo baseline --update` drops it — the baseline only ever shrinks on its own; it never grows
without you asking.

## The pre-commit gate

```console
$ kndo init --hook
```

writes `kndo.toml` and installs a pre-commit hook running `kndo check --staged --fail-on
warning`: it analyzes exactly what `git commit` would commit (the index), against HEAD, and
reports what your change introduces or fixes — including *derived* effects, findings your
change flips in files you never touched.

## Diff modes

```console
$ kndo check --staged          # index vs HEAD (what the pre-commit hook runs)
$ kndo check --diff main       # working tree vs merge-base(main, HEAD)
```

Both report `new` findings (with `introduced` vs `derived` origin) and `fixed` findings — the
reward loop: deleting dead code shows up as wins.

## Everyday commands

| Command | What it does |
|---|---|
| `kndo` / `kndo check` | full scan |
| `kndo health` | 0–100 score + per-category penalties (`--by-package` for monorepos) |
| `kndo doctor` | what kndo sees: adapters, cache state, plugins, config |
| `kndo baseline [--update]` | acknowledge current findings |
| `kndo find/describe/uses/used-by/trace/impact` | graph navigation ([For agents](agents.md)) |
