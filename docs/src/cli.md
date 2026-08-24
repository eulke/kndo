# CLI reference

```text
kndo — find what your codebase no longer needs

usage: kndo [command] [flags]

commands
  check            analyze the project (the default: bare `kndo` = `kndo check`)
  health           health score with per-category breakdown (--by-package)
  baseline         acknowledge current findings (.kndo/baseline.json; --update to refresh)
  doctor           what kndo sees: adapters, cache, plugins, config
  plugin           install | list | remove | new | build | wit | verify
  init             write kndo.toml (--hook also installs the pre-commit hook)
  find|describe|uses|used-by|trace|impact   graph navigation verbs (JSON envelopes)
  query            batched navigation requests from stdin (one JSON per line)
```

General rules that apply everywhere:

- `kndo help`, `--help`, or `-h` anywhere on the line prints usage and exits `0` — asking for
  help never triggers an analysis run.
- `kndo --version` / `-V` prints the binary version and the output schema version:
  `kndo 0.1.0 (schema 1.0.0)`.
- Bare flags with no subcommand (`kndo --format json`) are an implicit `check`.
- Valued flags accept both spellings: `--diff main` and `--diff=main`.
- **Unknown flags and missing values are hard errors (exit `2`)**, never silently ignored — a
  typo'd `--fail-onn warning` silently un-gating CI would be worse than the friction of
  rejecting it.
- An unknown command exits `2` with the list of valid commands.

## kndo check

`kndo check` (or bare `kndo`) runs the full analysis and prints the report.

| Flag | Meaning |
|---|---|
| `--staged` | analyze what `git commit` would commit (the index) vs `HEAD` |
| `--diff <ref>` | analyze the working tree vs `merge-base(<ref>, HEAD)` |
| `--fail-on <sev>` | exit `1` on findings at/above: `error` \| `warning` \| `info` \| `none` |
| `--format <f>` | `human` \| `json` \| `agent` \| `sarif` |
| `--color <c>` | `auto` (default) \| `always` \| `never` |
| `--quiet` | one-line summary |
| `--verbose` | per-phase timing block and cache state |
| `--no-cache` | disable the facts/graph cache for this run (never changes findings, only speed) |
| `--threads <n>` | worker threads; `0` = physical cores (the default) |

`--staged` and `--diff` are mutually exclusive. Both need `git` and a repository; a base ref
that doesn't resolve is an error-level diagnostic and exit `2` — with a hint to
`git fetch` it first.

### Run modes

- **Full** (default): every finding in the tree. `--fail-on` defaults to `none`.
- **Staged** (`--staged`): the git index as the "after" tree, `HEAD` as "before". kndo
  snapshots the index via git's object database; it never touches your working tree or the
  real index.
- **Diff** (`--diff <ref>`): the working tree as "after", `merge-base(<ref>, HEAD)` as
  "before".

In staged/diff modes both trees are fully analyzed and the report contains only the
*difference*: `findings` carries new findings (each tagged `introduced` — inside the change —
or `derived` — flipped by it elsewhere), `fixed` carries findings the change removed, and the
health block carries before → after. `--fail-on` defaults to `warning` in these modes and
judges only the new findings.

### Exit codes

| Code | Meaning |
|---|---|
| `0` | clean — no findings at/above the `--fail-on` threshold |
| `1` | findings at/above the threshold |
| `2` | kndo could not do what was asked: usage error, unresolvable ref, broken project root, or any error-level diagnostic during the run |

Two details worth knowing:

- **Advisory findings never move the exit code.** Plugin-contributed findings without a
  `[plugins.gate]` opt-in are advisory whatever their displayed severity — installing a
  finding-emitting plugin is safe by default. See [Plugins](plugins.md#plugin-findings).
- **An error-level diagnostic always forces exit `2`**, even if zero findings printed — an
  analysis that didn't run must never read as a clean pass.

When kndo's stdout pipe closes early (`kndo check | head`), kndo dies silently with
`SIGPIPE` like `grep`, `cat`, and `git` do (shells report this as exit 141) — no panic, no
stack trace.

## Output formats

Four formats, all rendering the same underlying result:

- **`human`** — the terminal report: group sections in triage order, one line per finding,
  evidence chains as indented `└` lines, a health block. Frontend-rendered; everything else
  below is byte-identical across frontends.
- **`json`** — the versioned machine envelope (`schema_version` uses semver; additions are
  minor bumps, breaks are major). The contract for CI and tooling: run metadata, `findings`,
  `fixed` (diff modes), `health`, `baseline`/`suppressed` counters, `diagnostics`.
- **`agent`** — a deterministic, token-frugal plain-text rendering for LLM agents. Same data
  as JSON, versioned independently (`agent-format 1` in its header). See
  [For agents](agents.md).
- **`sarif`** — SARIF 2.1.0 for GitHub code scanning and other SARIF consumers: `category`
  maps to the rule id, severity to SARIF level (`error`/`warning`/`note`), the evidence chain
  to `relatedLocations`, and confidence rides in `properties.confidence`.

Format resolution (first match wins):

1. `--format <f>` on the command line;
2. the `KNDO_FORMAT` environment variable, if non-empty;
3. auto-detect: `human` when stdout is a terminal, `json` when piped.

Color resolution for the human format: `NO_COLOR` (any value) always disables color;
otherwise `--color always`/`never` overrides; otherwise color follows terminal detection.
With color off, glyphs degrade to plain ASCII (`x`, `o`, `^`, `.`).

## Environment variables

| Variable | Effect |
|---|---|
| `KNDO_FORMAT` | default output format when `--format` isn't given (`human`, `json`, `agent`, `sarif`) |
| `KNDO_THREADS` | default thread count when `--threads` isn't given; `0` = physical cores |
| `KNDO_PLUGIN_DIR` | overrides the global plugin directory ([Plugins](plugins.md#where-plugins-live)) |
| `NO_COLOR` | disables terminal color everywhere |

Thread-count precedence: `--threads N` > `KNDO_THREADS` > physical cores. `0` from either
source means "physical cores" explicitly; `--threads 1` is fully supported (determinism
checks, debugging, noisy CI runners) and never changes the output — reports are byte-stable
across thread counts and cache states. A malformed `KNDO_THREADS` fails `check` (which owns
the flag) with a clear message; subcommands without their own `--threads` flag quietly fall
back to the default rather than fail on an env var they never asked about.

## kndo health

Runs a full analysis and presents it health-first: the 0–100 score, grade, trend versus the
previous run, and the complete per-category penalty table.

```console
$ kndo health --by-package
health   82.4  B   +0.4 ↑ from 82.0
  unused-symbols       ▃        −6.2  (47)
  unused-dependencies           −0.0
  ...
by package:
  @demo/web                 91.0  A
  @demo/api                 74.2  C
```

- `--by-package` adds the per-package breakdown (same penalties, grouped by owning package —
  never a different metric). Shown only when more than one package owns claimed files.
- `--format json` prints just the health object; `human`/`agent` render the table.
- Accepts `--no-cache`, `--threads`, `--quiet`, `--verbose`, `--color` like `check`.
- **Never a gate**: exits `0` regardless of the score (or `2` if the tree could not be
  analyzed at all). Gating belongs to `check --fail-on`.

See [Health & coverage](health.md) for the formula.

## kndo baseline

Snapshots the complete current finding set into `.kndo/baseline.json`.

- `kndo baseline` — creates the file; **refuses** to overwrite an existing baseline
  (exit `2` with a pointer to `--update`).
- `kndo baseline --update` — replaces the snapshot: acknowledged-and-still-present entries
  are kept, entries whose finding is gone are dropped.

Every baseline write after the first is an explicit `--update` in a reviewable commit — the
baseline never grows silently. See [Suppressions & baseline](suppressions.md#the-baseline).

## kndo doctor

Plain-text introspection (no `--format`): exactly what kndo sees for this project and why.

- **adapters** — each registered language adapter, its grammar version, file globs, and
  manifest globs; plus every globally installed adapter candidate with its activation rules
  and whether (and why) it activated, and any adapter dependency that is declared but absent.
- **plugins** — each active plugin with its detection prose, activation rules, dependencies,
  requested file access, and declared finding rules; plus global candidates that did *not*
  activate (with the rule that didn't fire), missing plugin dependencies, and the per-plugin
  contribution record from the last run (roots, edges, annotations, dropped targets) — so
  "this plugin exempted 400 symbols" is a visible line, not an invisible bias.
- **cache** — enabled/disabled, writability, facts entries and bytes, graph snapshots.
- **baseline** — present (with entry count) or absent.

## kndo init

Project scaffolding; performs no analysis.

- Writes `kndo.toml` (a fully commented template — every setting optional, defaults shown).
  If the file exists, it is left untouched.
- Adds `.kndo/` to `.gitignore` (idempotent; creates the file if missing).
- Prints the recommended pre-commit line. With `--hook`, installs
  `.git/hooks/pre-commit` running `kndo check --staged --fail-on warning` — but only when no
  pre-commit hook exists; if one does, kndo refuses (exit `2`) and prints the line for you to
  add yourself.
- Prints an advisory pointing at `kndo agents install` when the agent skill (below) isn't
  installed yet.

## kndo agents

Agent-integration scaffolding — one action today:

- `kndo agents install` — writes the embedded [agent skill](agents.md#the-kndo-skill) to
  `.agents/skills/kndo/` and links `.claude/skills/kndo` to it. Idempotent: re-running after
  a binary upgrade is the update flow (the installed files are kndo-owned and overwritten on
  drift, unlike `init`'s pre-commit hook). Refuses — exit `2` — rather than overwrite an
  unexpected `.claude/skills/kndo` that isn't already the expected symlink.

## kndo plugin

Two halves — using plugins and authoring them:

| Subcommand | What it does |
|---|---|
| `install <github.com/owner/repo[@tag]>` | fetch, checksum-verify, identity-check, and install into the global directory (plus dependency closure) |
| `list` | installed plugins, versions, and any hand-installed `.wasm` files |
| `remove <coordinate>` | uninstall (anything still depending on it shows in `kndo doctor` as a missing dependency — never an error) |
| `new <dir> [--adapter]` | scaffold a component crate with the ABI vendored |
| `build [dir]` | `cargo build` + componentize into `<name>.wasm` |
| `wit [plugin\|adapter]` | print the WIT world this binary was built against |
| `verify <component.wasm> [--project <dir>]` | load, lint the descriptor, and drive every hook against a fixture project |

See [Plugins](plugins.md) and [Writing a plugin](plugin-authoring.md).

## Navigation verbs and kndo query

`find`, `describe`, `uses`, `used-by`, `trace`, and `impact` are read-only graph queries with
their own flag set (`--kind`, `--color`, `--lang`, `--depth`, `--transitive`, `--edges`,
`--if-deleted`, `--all`, `--max-paths`, `--roots`, `--pair`, `--limit`, `--format`) and their
own exit-code convention: `0` ok, `1` selector/path not found, `2` malformed request or
error. `kndo query` batches many requests over one graph load, reading JSON Lines from stdin
(capped at 1000 requests per invocation, excess reported loudly).

Full treatment with request and response examples: [Graph navigation](navigation.md).
