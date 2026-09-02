# The command line

`kndo` with no verb is `kndo check`. Every verb takes an optional project root
(positional for the analysis verbs, `--root` for the query verbs) and defaults
to the current directory.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | the run completed and nothing reached the gate |
| 1 | findings at or above `--fail-on` (default `warning`) |
| 2 | kndo could not run: a refused configuration, an unreadable root, a broken `--diff` reference |

`kndo health` always exits 0 — it is a measurement, not a gate. The query
verbs exit 1 when an input was not found and 0 otherwise.

## Analysis verbs

```text
kndo check [OPTIONS] [PATH]      Analyze a project and report its findings (the default)
kndo health [OPTIONS] [PATH]     Project health only — the same measurement, as one block
kndo baseline [OPTIONS] [PATH]   Accept the current findings as the baseline future runs diff against
kndo init [--hook] [PATH]        Write the kndo.toml template (and, with --hook, a pre-commit gate)
kndo doctor [PATH]               What kndo sees here: composition, config, cache, baseline
```

Options shared by `check`, `health` and `baseline`:

| Option | Meaning |
|---|---|
| `--no-cache` | ignore and bypass the on-disk caches for this run |
| `--threads <N>` | worker threads (default: all cores; the output is identical at any count) |
| `--fail-on <level>` | lowest severity that fails the run: `error`, `warning` (default), `info`, `never` |

Options of `check`:

| Option | Meaning |
|---|---|
| `--format <fmt>` | `human`, `json`, `agent` or `sarif`; unset, `KNDO_FORMAT` decides, else `human` on a terminal and `json` when piped |
| `--staged` | analyze what `git commit` would commit, against `HEAD` |
| `--diff <ref>` | analyze the worktree against `merge-base(<ref>, HEAD)` |
| `--only <categories>` | judge only these categories (comma-separated, repeatable) |
| `--skip <categories>` | judge everything except these categories |
| `--quiet` | human render: print the verdict line only |
| `--verbose` | human render: also print the run's phase timings |
| `--color <when>` | color the human render: `auto` (default: on a terminal unless `NO_COLOR` is set), `always`, `never` |

`--only` and `--skip` narrow *judgment*, not display: an unselected analysis
never runs, its suppressions cannot read as stale, and health follows — skip
`unused` and health is absent, not padded. `kndo health --by-package` adds the
per-package split to the terminal render; the JSON envelope always carries it.

`--staged` and `--diff` are two full analyses over two pinned trees, composed:
the report lists the findings the change introduces, the ones it fixes
(`fixed`), and both trees' health (`health` and `base_health`). Both trees read
and warm the project's cache — entries are content-addressed, so a pinned tree's
unchanged files hit exactly where the worktree's do — and the base side's result
is kept under its git tree id, so the next run against the same `HEAD` (or the
same merge-base) reads it back instead of analyzing it again. When everything
is staged and no untracked file is in sight, the worktree is the index, and
`--staged` judges it in place — nothing is materialized at all. Neither mode
touches your worktree; `--no-cache` turns the caches off for both sides.

## Formats

- **human** — one line per finding (`severity category path[:line]: message`),
  the count, the health line, and the abstentions. The default on a terminal.
- **json** — the full envelope, schema `kndo-v2/m6`: `run`, `health`,
  `base_health`, `findings`, `fixed`, `baselined`, `abstained`, `suppressed`,
  `plugins`, `diagnostics`. The default when stdout is not a terminal. The
  schema ships with the repository (`schemas/report.schema.json`) and is what
  the GitHub Action reads.
- **agent** — the token-thrifty render for models, described in
  [Agents](agents.md).
- **sarif** — SARIF 2.1.0 for code-scanning uploads, with line regions.

`kndo | head` behaves like any filter: the binary restores the default `SIGPIPE`
disposition.

## Query verbs

```text
kndo find <pattern>...        Search the graph for nodes by name (exact > prefix > substring)
kndo describe <selector>...   One node in full: reach, keepers preview, findings on it
kndo uses <selector>...       What a node depends on: imports and referenced names, resolved
kndo used-by <selector>...    What keeps a node alive — the deletion question, with sites
kndo trace <selector>...      Why is this alive: the shortest root-to-node path
kndo impact <selector>...     What transitively depends on a node; --if-deleted simulates the removal
kndo explain <finding-id>...  Everything behind one finding id: the finding and its subject described
```

They share one option set — `--root`, `--limit`, `--kind`, `--reach`,
`--roots`, `--to`, `--if-deleted`, `--format` — explained in
[Navigation](navigation.md). Each input gets its own result, so a batch of
selectors is one process and one analysis.

## Environment

| Variable | Effect |
|---|---|
| `KNDO_FORMAT` | the report format when `--format` is not given |
| `NO_COLOR` | disables color in the human render (the `--color` flag wins) |

Precedence for anything that can also live in `kndo.toml`: flag, then
`KNDO_FORMAT`, then the file, then the built-in default.
