# Suppressions and the baseline

Two ways to take a finding off the report, for two different situations.

## `kndo:allow` — a verdict in the code

A pragma inside any comment, in any language the adapters read comments from:

```text
kndo:allow <category>[, <category>…] [-- reason]        this line, or the next
kndo:allow-file <category>[, <category>…] [-- reason]   the whole file
```

```ts
// kndo:allow unused -- loaded by the plugin host through its manifest, not imported
export function activate() { … }
```

Line scope covers findings whose subject starts on the pragma's line or the
line after it (for a multi-line comment, the line after its last line). A
suppressed finding is a human verdict overriding the analysis: it leaves the
report entirely and implicates nothing in [health](health.md). The report's
`suppressed` block keeps the totals per category.

An allow that suppresses nothing is itself a finding — `stale`, a warning —
because dead configuration is still dead code. With one carve-out: an allow
whose category was not judged this run (its analysis abstained, or you
narrowed the run with `--only`/`--skip`) is not stale. Turning an analysis off
must not flip every allow of it to stale and back.

## The baseline — debt acknowledged

```sh
kndo baseline
```

accepts the current findings and writes them to `.kndo/baseline.json`; from
then on `kndo check` lists only what is new, reports how many were `baselined`,
and lists in `fixed` any baselined finding that no longer fires. Commit the
file; it is small, and it is the team's record of what it has agreed to carry.

The baseline hides findings from the listing and from the gate — **never from
health**. Baselining everything must not read as getting healthier; the ratio
still counts every implicated subject, so the number in the summary is the
tree's, not the backlog's.

Finding identity is what makes both mechanisms stable: a finding's id
(`kndo-` and twelve hex digits) is a function of its category and subject,
never of its line or its message, so it survives edits above it and
re-renders.

## Which to use

- The finding is wrong for a reason the code can state (a framework reaches
  it, a reflection site names it): `kndo:allow` with the reason. Better still,
  if the reason is a framework, an [extension](extensions.md) that states the
  fact once for every file.
- The finding is right and you are not fixing it now: the baseline.
- The finding is right and you are fixing it: fix it.
