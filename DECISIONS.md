
## 2026-08-31 — Presentation: --quiet, --verbose, --color, and the filter disposition

The last broadly-applicable v1-surface rows, rebuilt as the human render's own
options — they shape presentation and nothing else, so on a non-human format
(json/agent/sarif are byte-pinned contracts) the CLI says so on stderr and
changes nothing, and `--quiet --verbose` together is a refused invocation.

- **`--quiet` is the one-line contract**: the verdict line alone (findings
  count, or the staged/diff summary) — the exit code already carries the gate,
  which is exactly what a hook or script wants.
- **`--verbose` is the observability channel**: a `phases:` line rendered from
  the timings that deliberately live BESIDE the byte-identical report, never
  inside it. v1's other verbose effect — revealing `possible`-confidence
  findings — has no successor because v2 hides no confidence tier in the first
  place; there is nothing to reveal.
- **`--color auto|always|never`** in the universal spelling, resolved in the one
  merge site: flag > `NO_COLOR` (present and non-empty, no-color.org) > tty.
  The palette is semantic and small: severity words (error red, warning
  yellow, info cyan), the clean line green, and on a diff-mode health arrow
  the CURRENT side colored by DIRECTION — a fact derived from two measured
  ratios, compared exactly by cross-multiplication. A single score is never
  colored: judging 76 as "bad" would smuggle v1's letter bands back in through
  the palette. None of the three is a kndo.toml key — quiet/verbose are
  per-invocation moods and NO_COLOR is the persistent color preference; a
  config key waits for demand.
- **The query verbs' reach filter respells as `--reach`** (`kndo find x --reach
  production`), freeing `--color` for its universal meaning and reading better
  for what it filters. The contract field stays `color` — the envelope and
  schemas are untouched; only the CLI flag moved, before anything shipped.
- **SIGPIPE**: kndo's output is designed to be piped, so the CLI restores the
  default disposition at startup and dies silently with signal 13 under
  `| head`/`| grep -q` like every other Unix filter — v1's reasoning, adopted
  as v2's own judgment. The suite proves it the honest way: a fixture whose
  JSON output is ASSERTED to overflow a pipe buffer, then killed-by-SIGPIPE
  asserted with no panic on stderr. serve keeps error-propagation instead —
  a protocol conversation is not a filter, and it ends cleanly when its
  transport closes.

217 tests, 15 gates, clippy clean; smoke on the demo shows the staged arrow
`health 83.3 → 100.0` with the improved side green.
