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
# max-health-drop = 0.0                  # the largest health DROP a change may cause
# max-net-findings = 0                   # the largest allowed `new − fixed`

# [delta.budget]                         # finer tolerances, by group or category
# defect = 0                             # absolute: `fixed` never pays for these
# duplicate = 2

# [[rule]]                               # per-path overrides
# paths = ["examples/**"]
# skip = ["unused"]

# [[externally-invoked]]                 # entry points only your framework knows about
# markers = ["Controller", "Bean"]       # annotations/attributes/decorators, by name
# paths = ["src/main/java/**"]           # optional scope; omit to apply project-wide

# [plugins.gate]                         # opt plugin findings into the exit-code gate
# "github.com/acme/some-plugin" = "warning"        # gate this plugin's rules, capped at warning
# "github.com/acme/some-plugin/noisy-rule" = "off" # per-rule override wins
```

> **What the engine reads today:** **`[analysis]`** (`skip`, `min-confidence`),
> **`[analysis.duplicate]`** (`min-tokens`), **`[analysis.crap]`** (`threshold`),
> **`[performance]`** (`threads`), **`[[rule]]`**, **`[[externally-invoked]]`**,
> **`[plugins.gate]`**, **`[plugins.<id>]`** (`report`, `max-age`), and
> **`[delta]`**/**`[delta.budget]`** are all live.
> Still documented-but-unwired: **`[project]`** (discovery is gitignore-aware
> automatically; scoping it from config doesn't exist yet) — kndo prefers an honestly
> inert commented section over half-applied configuration. This page will always state
> exactly which keys are live.

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

Budgets for diff modes (`--staged`, `--diff`), judging the *change* rather than the debt.
They compose with `--fail-on` by OR: a run exits 1 when findings reach the severity
threshold **or** any budget is exceeded.

- **`max-health-drop`** — the largest health-score decrease a change may cause. Measured as
  a drop, so a change that *improves* health measures negative and passes any limit.
- **`max-net-findings`** — the largest allowed `new − fixed` count.

Three things about the section as a whole:

- **Writing `[delta]` at all is the opt-in.** With no section, nothing is evaluated and the
  JSON envelope carries no `budget` block — which is how a consumer tells "every budget
  held" from "nobody set one". A project that never opts in cannot change exit code because
  budgets exist.
- **Inside the section, the strict ratchet is the default.** Writing `[delta]` with only
  `max-net-findings` leaves `max-health-drop` at `0.0`.
- **Advisory findings never count.** A plugin finding without a `[plugins.gate]` opt-in is
  excluded from every budget, exactly as it is from `--fail-on`: installing a
  finding-emitting plugin must not move your gate.

### `[delta.budget]`

Finer tolerances, keyed by **group** (`defect`, `waste`, `risk`, `hygiene`, `convention`) or
by **category** (`duplicate`, `unused`, …). Each value is the largest number of *new*
findings of that kind a change may introduce:

```toml
[delta.budget]
defect = 0        # never a new defect
duplicate = 2     # up to two new clones
```

These are **absolute, not net**: `fixed` findings compensate only inside `max-net-findings`.
`defect = 0` means zero new defects even if the same change fixes ten others — otherwise a
change could trade a repaired typo for a fresh security defect and call it even.

### `[[rule]]`

Per-path overrides: each entry names `paths` globs and the `skip` list applying under them —
the way to, say, exempt `examples/**` from `unused` without a pragma in every file. Skipped
findings are counted in the report's `suppressed.config`; a finding also covered by an
inline pragma counts as `inline` instead (pragmas match first, so config can never make a
working pragma look stale). `stale` itself can't be skipped — the audit of your
suppressions stays visible by design.

### `[[externally-invoked]]`

Some code is called from outside your source entirely, and no amount of analysis will find
the call. A Spring `@Controller` is instantiated by classpath component scanning and its
methods are dispatched by URL; a JUnit `@AfterEach` is called by the runner; a ByteBuddy
`@Advice.OnMethodEnter` body is inlined into instrumented bytecode; a Koin `@Scoped`
annotation is read by an annotation processor that lives in a different repository. kndo is
*right* that nothing in your code references them — and reporting them `unused` or
`test-only` is still wrong.

`[[externally-invoked]]` is how you say so, once, for a whole class of declarations:

```toml
[[externally-invoked]]
# Spring wires these by component scan and calls them through the dispatcher.
markers = ["Component", "Configuration", "Bean", "Controller", "RestController",
           "Service", "Repository", "ControllerAdvice", "SpringBootApplication"]
paths = ["src/main/java/**"]     # optional; omit and the rule applies project-wide

[[externally-invoked]]
# JUnit calls the lifecycle hooks; nothing in the source names them.
markers = ["Test", "BeforeEach", "AfterEach", "BeforeAll", "AfterAll"]
```

- **`markers`** — annotation names (Java, Kotlin), attribute paths (Rust), attributes
  (Swift), decorators (JS/TS), matched against what the adapter read off the declaration.
  Write them the way your source writes them: for `@Advice.OnMethodEnter`, either
  `"Advice.OnMethodEnter"` or `"OnMethodEnter"` matches. Required and non-empty.
- **`paths`** — optional globs scoping the rule to some files. Omitted means project-wide.

**This is not a skip.** A matched declaration becomes a real production entry point, so
everything it reaches comes alive with it and every analysis keeps judging all of it
normally — an untested controller still reports `untested`, a duplicated one still reports
`duplicate`. That is the difference from `[[rule]] skip`, which would silence the genuine
findings in those files along with the false ones. Nothing is counted as suppressed, because
nothing was suppressed: the graph was simply told the truth about where execution enters.

kndo never guesses these for you and ships no list of framework names: knowing that
`@Controller` means Spring would mean learning frameworks, and the analysis core is built to
stay ignorant of even the *languages* it analyzes. What it does is match the strings you
supply. A marker you never configure is inert.

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

### `[plugins.<id>]`

Per-plugin options. Live today for the coverage ingesters (a bare key names a built-in
without its `kndo:` prefix; quoted full ids also work):

```toml
[plugins.coverage-lcov]
report = "packages/*/coverage/lcov.info"  # string or array; globs allowed
max-age = "30d"                           # or "12h", or a bare integer (days)
```

- **`report`** — where this plugin's report(s) live. **Replaces** the descriptor's
  well-known paths (explicit config wins; list the well-known one too if you want both).
  Globs cover monorepos with one report per package. Invalid globs are diagnostics, not
  crashes.
- **`max-age`** — per-plugin freshness override for the 7-day default; an older report is
  ignored with a diagnostic.
- Other plugins' option tables (`[plugins.nextjs] app-dir = …`) parse as inert until their
  subsystems exist — same posture as every documented-but-unwired section.

## What is *not* configuration

- **Suppressing individual findings** — that's a reviewable
  [inline pragma or the baseline](suppressions.md), deliberately kept in code and in a
  committed JSON file where diffs are seen, not in config where they'd silently accumulate.
- **Which plugins run** — presence and activation rules decide
  ([Plugins](plugins.md#activation-when-does-a-plugin-run)); there is no enable/disable list
  to drift out of sync.
- **Entry points and dependency scopes** — your manifests already say this; kndo believes
  them.
