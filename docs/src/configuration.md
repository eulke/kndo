# Configuration

kndo is **zero-config by design**: your manifests are the configuration. Entry points,
workspaces, dependency scopes, library-vs-app mode — all read from `package.json`, `go.mod`,
`Cargo.toml`, `pom.xml`, `build.gradle(.kts)`, `Package.swift`. `kndo.toml` exists for the
handful of decisions only you can make.

`kndo init` writes the template below at the project root. Everything is optional, and every
setting ships with the default shown — an empty (or absent) `kndo.toml` is a fully working
configuration.

```toml
# kndo.toml — everything here is optional; every setting already has the default shown.

# [project]
# roots = ["src", "packages/*"]          # default: auto (git ls-files minus ignores)
# exclude = ["**/generated/**"]

# [analysis]
# skip = []                              # categories or category:subject, e.g. ["unused:enum-member"]
# min-confidence = "possible"            # report floor; raise to "probable" to hide the
#                                        # speculative tier (--verbose always shows everything)

# [analysis.duplicate]
# min-tokens = 50

# [analysis.crap]
# threshold = 30

# [performance]
# threads = 0                            # 0 = physical cores; --threads flag wins

# [delta]                                # diff-mode gate budgets
# max-health-drop = 0.0
# max-net-findings = 0

# [[rule]]                               # per-path overrides
# paths = ["examples/**"]
# skip = ["unused"]

# [plugins.gate]                         # opt plugin findings into the exit-code gate
# "github.com/acme/some-plugin" = "warning"        # gate this plugin's rules, capped at warning
# "github.com/acme/some-plugin/noisy-rule" = "off" # per-rule override wins
```

> **What the engine reads today:** **`[analysis]`** (`skip`, `min-confidence`),
> **`[analysis.duplicate]`** (`min-tokens`), **`[analysis.crap]`** (`threshold`),
> **`[performance]`** (`threads`), **`[[rule]]`**, and **`[plugins.gate]`** are all live.
> Still documented-but-unwired: **`[project]`** (discovery is gitignore-aware
> automatically; scoping it from config doesn't exist yet) and the **`[delta]`** budget
> gate — kndo prefers an honestly inert commented section over half-applied
> configuration. This page will always state exactly which keys are live.

## Key by key

### `[project]`

- **`roots`** — directories to analyze. Default: automatic discovery of the whole project
  tree, honoring `.gitignore` (so `node_modules/`, `target/`, build output never enter the
  graph).
- **`exclude`** — glob patterns to drop from discovery on top of the ignore rules.

### `[analysis]`

- **`skip`** — verdicts to disable outright, as categories (`"duplicate"`) or
  category-subject pairs (`"unused:enum-member"`) using the same vocabulary as
  [suppressions](suppressions.md).
- **`min-confidence`** — the report floor. Default `"possible"`: every tier is reported.
  Raise it to `"probable"` (or `"certain"`) to hide speculative findings by default;
  `--verbose` always shows every tier regardless of the floor, and `stale` findings (the
  suppression audit) are never floored. The floor drops findings from the report — it is a
  display posture, not an acknowledgment, so nothing is counted as suppressed.

### `[analysis.duplicate]`

- **`min-tokens`** — the structural-clone floor: callables with fewer normalized tokens don't
  participate in clone detection. Default `50`, which is also the hard minimum — extraction
  fingerprints nothing smaller, so lower values clamp to `50` (with a diagnostic).

### `[analysis.crap]`

- **`threshold`** — the CRAP score above which a [finding fires](rules.md#crap).
  Default `30`.

### `[performance]`

- **`threads`** — worker threads; `0` means physical cores. The `--threads` flag and the
  `KNDO_THREADS` variable always win over the file. Thread count never changes output, only
  speed.

### `[delta]`

Budgets for diff modes (`--staged`, `--diff`), judging the *change*:

- **`max-health-drop`** — the largest health-score decrease a change may cause.
- **`max-net-findings`** — the largest allowed `new − fixed` count.

### `[[rule]]`

Per-path overrides: each entry names `paths` globs and the `skip` list applying under them —
the way to, say, exempt `examples/**` from `unused` without a pragma in every file. Skipped
findings are counted in the report's `suppressed.config`; a finding also covered by an
inline pragma counts as `inline` instead (pragmas match first, so config can never make a
working pragma look stale). `stale` itself can't be skipped — the audit of your
suppressions stays visible by design.

### `[plugins.gate]`

By default, findings emitted by plugins are **advisory**: rendered, baselineable,
suppressible, but never able to move the exit code — installing a plugin can't break your
build. `[plugins.gate]` is the opt-in:

```toml
[plugins.gate]
"github.com/acme/kndo-deprecations" = "warning"        # gate every rule of this plugin
"github.com/acme/kndo-deprecations/noisy-rule" = "off" # except this one (per-rule wins)
"kndo:nextjs" = "error"                                # built-ins gate by their id
```

- The key is a plugin coordinate, or `coordinate/rule` for one rule. Per-rule entries beat
  per-plugin ones.
- The value is the gate cap: `"error"`, `"warning"`, `"info"`, or `"off"`. A gated finding's
  severity is `min(declared, configured)` — configuration can **lower** a rule's declared
  severity, never raise it.
- A malformed `kndo.toml`, or a malformed gate value, is reported as a diagnostic and the
  gate entry is ignored — never a crashed run.

## What is *not* configuration

- **Suppressing individual findings** — that's a reviewable
  [inline pragma or the baseline](suppressions.md), deliberately kept in code and in a
  committed JSON file where diffs are seen, not in config where they'd silently accumulate.
- **Which plugins run** — presence and activation rules decide
  ([Plugins](plugins.md#activation-when-does-a-plugin-run)); there is no enable/disable list
  to drift out of sync.
- **Entry points and dependency scopes** — your manifests already say this; kndo believes
  them.
