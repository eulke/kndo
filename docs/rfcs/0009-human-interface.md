# RFC 0009 — Human Interface: CLI Rendering & Visual Language

**Status:** Draft · **Depends on:** RFC 0005 (groups), RFC 0006 (commands), contracts §5 (Engine)

## 1. Scope & position

This RFC owns everything a human *sees* in the terminal. It binds only `kndo-cli`: the core
returns data (`RunResult`), the CLI renders it — the separation is contractual (contracts §5),
so any future frontend (LSP, GUI, `kndo serve`) can define its own presentation without
touching this document or the core. Machine formats (JSON/SARIF, and the LLM-oriented agent
format) are out of scope here — they are serialized core-side and schema-governed
(output-schema §9).

## 2. Design principles

1. **Scannable in two seconds.** The first line answers "am I fine?"; the layout answers "what
   do I fix first?" without reading everything. Triage order = group order (RFC 0005 rule 4).
2. **Quiet success.** A clean run prints one line (`kndo · clean · health 91 (A) · 214 ms`).
   No banners, no ASCII art, no emoji noise, no advertising. Silence is the reward.
3. **Semantic color, never decorative.** Color encodes exactly two things: the finding's group
   and delta polarity (new/fixed). If it's colored, it means something; if it means something,
   it's *also* expressed without color (§4) — color-blind users and CI logs lose nothing.
4. **Stable geometry.** Same columns, same order, same indentation every run — muscle memory is
   an interface. New information may append, never reshuffle.
5. **Every finding is actionable in place.** Each line carries its id (feeds `explain` and
   suppressions) and a `path:line` the terminal can make clickable. Dead ends are forbidden:
   truncation always names the command that shows the rest.

## 3. Visual vocabulary

| Semantic | Color | Prefix glyph (unicode / ASCII fallback) |
|----------|-------|------------------------------------------|
| group `defect` | red | `✗` / `x` |
| group `waste` | yellow | `◦` / `o` |
| group `risk` | magenta | `▲` / `^` |
| group `hygiene` | blue | `·` / `.` |
| delta `fixed` | green | `✓` / `+` |
| evidence / secondary | dim | `└` / `\`- ` |
| health up / down | green / red | `↑` / `↓` (`^`/`v`) |

- Category and confidence render as **text** (`unused`, `(probable)`) — never encoded only in
  color or glyph (principle 3).
- One accent color per line maximum; paths and messages stay in default foreground. kndo output
  should look calm next to a compiler's.

## 4. Capability degradation

Detection order, no configuration required:

1. **Rich TTY**: truecolor/256 + unicode → full vocabulary.
2. **Basic TTY**: 8/16 colors, or unstable width → same layout, basic colors, ASCII glyphs.
3. **No TTY / CI log / `NO_COLOR` / `--color never`**: plain text; glyph column keeps the ASCII
   fallbacks so grep-ability and meaning survive (`x`, `o`, `^`, `+`).
4. **`TERM=dumb`**: additionally no cursor tricks, no width fitting — plain lines only.

`--color auto|always|never` (default `auto`); `NO_COLOR` env always wins over `auto`.
Width: fit to terminal width with truncation-by-column-priority (message truncates before path;
id never truncates); below 60 columns, fall back to two-line-per-finding layout.

## 5. Layout grammar

One finding = one primary line, optional evidence lines, fixed column order:

```
<glyph> <category>[:<subject>] <path:line>  <message> [confidence] [id]
        └ <evidence>  (cause / kept-alive-by / cycle path…)
```

- Groups render as **sections** with a count header (`WASTE (51)`), in fixed group order; empty
  sections are omitted, not shown as zero.
- Within a section: sorted by severity, then path, then span — deterministic (RFC 0008 §4
  applies to rendering too).
- Rollup findings state their scope in the message (`directory unreachable — 14 files`), and
  `explain` unrolls them.
- Diff mode: `NEW (introduced by this change)` then `NEW (derived, in untouched code)` then
  `FIXED` blocks (`delta_origin` split, RFC 0004 §6), each internally in group order; the header
  line always shows counts and net (`3 new · 2 fixed · net +1`), followed by the health movement
  line and — when any `[delta]` budget is configured — the **budget block**: one line per rule
  with limit, measured value, and verdict glyph; failures append `over by N` (exactly how much
  to fix), and a health drop near a grade boundary appends the distance (`B (1.9 from C)`).
  The PASS/FAIL word closes the block — the gate is never mysterious.

  ```
  health   84.1 ──▶ 81.9   −2.2 ↓   B  (1.9 from C)
  budget   health-drop ≤ 1.0   −2.2  ✗   over by 1.2      FAIL
  ```
- Health block (in `kndo health` and full runs): score, grade, and per-category penalty bars
  built from `▁▂▃▄▅▆▇` (ASCII: `#` scaled) — a shape, not a chart; details stay tabular.

## 6. Streams, progress & verbosity

- **stdout** carries the report and nothing else; **stderr** carries progress and diagnostics.
  In `--format json`, stdout is pure parseable JSON — the discipline that makes piping safe is
  absolute, and it holds for human format too.
- Warm runs show **no progress at all** (they're done before a spinner would spin). Cold runs
  print a single self-overwriting stderr line (`indexing 3 412/5 210 files`), TTY-only — CI logs
  get one start line and one end line instead.
- `--quiet`: header line + exit code only. `--verbose`: adds `possible`-confidence findings,
  per-phase timings, and cache state. Neither changes *what* was analyzed (RFC 0006 flags do).
- Errors speak human: every `EngineError` renders as problem + probable cause + next command
  (`cache locked by pid 4211 — another kndo is running; retry or kndo doctor`). Never a bare
  Rust error chain outside `--verbose`.

## 7. Non-goals (1.0)

Interactive TUI (panes, filtering), themes/config for colors beyond `--color`, localization
(English only until the schema stabilizes), notification/sound hooks, markdown/HTML human
reports (the JSON feeds external renderers).

## 8. Open questions

1. Should `kndo health` render a sparkline of the last N snapshots (data exists in
   `.kndo/cache/`) or stay single-run until a real trend store lands post-1.0?
2. Glyph set on Windows legacy consoles (cmd.exe pre-Windows-Terminal): force ASCII always, or
   trust UTF-8 codepage detection?
