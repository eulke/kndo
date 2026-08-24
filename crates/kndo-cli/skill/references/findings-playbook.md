# Findings resolution playbook

Prescriptive procedure for resolving kndo findings. Follow it as written — the goal is that
any agent, in any session, resolves the same finding the same way.

A finding's `category` is the verdict; `subject_kind` says what it landed on (function,
file, directory, dependency, …); `group` drives triage order; `severity`
(`error|warning|info`) drives gating; `confidence` (`certain|probable|possible`) drives how
much verification you owe before acting. When a verdict holds uniformly, kndo reports the
**widest node once**: one `unused` directory finding means the whole subtree is safe to
remove — do not re-derive per-file conclusions.

## Protocol

1. **Work groups in order: defect → waste → risk → hygiene → convention.** Defects are
   broken promises (fix first); waste is deletion (shrinks the code others operate on); risk
   is hardening; hygiene is bookkeeping; convention (plugin findings) is advisory unless the
   project gated it.
2. **Within a group, work `certain` before `probable` before `possible`.**
3. **Confidence sets the verification you owe before editing:**
   - `certain` — apply the recipe directly (dead is always certain: kndo never claims
     "unused" on dynamic evidence).
   - `probable` — run the category's pre-check first; proceed when it confirms.
   - `possible` — verify with `trace`/`used-by` before touching anything. If verification is
     inconclusive (a dynamic or wildcard edge you cannot rule out), prefer a
     suppression-with-reason over a speculative deletion.
4. **One finding id, one commit-sized change.** Fix, then verify:
   `kndo check --diff <base> --format agent` must list the id under `fixed:`. Ids are stable
   across reformatting (they never hash line numbers), so the assertion is exact.
5. **Re-check between groups.** Fixes cascade: deleting an unused function can render its
   callees unused (new `unused` findings — fix them too), or resolve other findings. The
   diff check surfaces the cascade; keep looping until `new:` is empty.
6. **Never fabricate a resolution.** If a finding cannot be fixed within the task's scope,
   say so and leave it — do not suppress it to make the report clean.

## Fix vs suppress

Decide with this tree, in order:

1. **Is the finding factually wrong?** (Production reaches the code through a channel kndo
   cannot see: reflection with computed names, subprocess invocation, out-of-band entry
   points.) → Inline `kndo:allow <category> <reason>` pragma **with a reason** — see
   [suppressions-and-baseline.md](suppressions-and-baseline.md). Prove it first: a
   `kndo trace <sel>` that finds no path plus your own evidence of the dynamic channel.
2. **True finding, in scope?** → Fix it per the recipe below.
3. **True finding, accepted debt / out of this task's scope?** → Leave it. Bulk
   acknowledgment is `kndo baseline` — a human-reviewed decision, not something to run
   casually mid-task.
4. **A whole path that is not yours** (generated output, vendored code, examples)? → a
   `[[rule]]` paths+skip entry in `kndo.toml`.

Hard rules:

- **Never suppress `defect`-group findings** (`undeclared`, `version-skew`,
  `private-type-leak`, `unresolved`) without explicit human sign-off — they are broken
  promises, not style opinions.
- **Never suppress `crap`** — its two levers (test or simplify) are always available.
- Prefer deleting to suppressing: the finding is usually right.
- If the same suppression recurs for a framework reason ("route handlers are not unused"),
  the right fix is a kndo plugin that contributes the root — flag that to the human instead
  of scattering pragmas.

## Per-category recipes

Each recipe has four fields. **Means**: what the verdict says. **Pre-check**: the navigation
command that confirms it (mandatory below `certain` confidence, cheap insurance otherwise).
**Recipe**: the fix, applied exactly. **Verify**: always the same —
`kndo check --diff <base> --format agent`, assert the id under `fixed:`, then handle
anything new — so it is only spelled out where more is needed.

### unused

- **Means** (waste · warning · always certain): unreachable from every production, test, and
  tooling root.
- **Pre-check**: `kndo impact <sel> --if-deleted` — collect the full removal set
  (`newly_unreachable`, `freed_dependencies`) so you delete once, not five times.
- **Recipe**: delete it — the symbol and its now-dangling `export`/`import` lines; a file
  finding means the whole file; a directory finding means the whole subtree (the rollup only
  fires when every file under it is unused). For a dependency, remove the manifest entry.
  Also delete everything in `newly_unreachable` and remove `freed_dependencies` from the
  manifest in the same edit.
- **Note**: library public API is never reported unused, so do not "protect" exports by
  guessing — if it is reported, in-repo evidence says it is dead.

### test-only

- **Means** (waste · info): reachable, but only from test roots — it ships with no
  production consumer.
- **Pre-check**: `kndo used-by <sel>` — confirm `by_color.production == 0`; the entries list
  the test files involved.
- **Recipe**: decide by intent, in this order: (a) if it is abandoned production code, delete
  it **together with the tests that enshrine it** (tests of dead code are not coverage, they
  are life support); (b) if it is genuinely a test utility, move it into the test/fixtures
  area. For a dependency: move it from prod scope to the dev scope in the manifest.

### untested

- **Means** (risk · info): production-reachable code that no test reaches, even
  transitively (static reachability, not execution).
- **Pre-check**: `kndo describe <sel>` for size/degree — pick the highest-leverage entry
  point to test, not necessarily the flagged leaf.
- **Recipe**: add a test that imports and exercises it (directly or transitively). The
  finding resolves the moment any test reaches it. Do not write an assertion-free import
  just to silence it — exercise real behavior.
- **Note**: code driven only as a subprocess or over the network reads as untested here even
  when coverage reports disagree; that is a legitimate suppression case (tree step 1).

### internal-only

- **Means** (waste · info): declared visibility wider than any real usage requires; the
  message names the tightest sufficient rung.
- **Pre-check** (only needed below certain): `kndo used-by <sel>` — confirm all references
  sit within the suggested scope.
- **Recipe**: narrow the declaration to exactly the rung the message suggests
  (`pub(crate)`, `private`, `internal`, `fileprivate`, …). Do not narrow further than
  suggested.

### private-type-leak

- **Means** (defect · warning in libraries, info in apps): a public callable's signature
  references a type its consumers cannot name.
- **Pre-check**: `kndo used-by <owner-sel>` on the leaking API to see who consumes it.
- **Recipe**: prefer exporting the referenced type (widening the type keeps the API); narrow
  the API instead when the type is genuinely an implementation detail and the API's own
  audience is internal. Choosing has API-design consequences — if the symbol is a published
  library surface, confirm direction with the human.

### deep-import

- **Means** (risk · warning · one finding per consumer-package → provider-package pair): a
  package imports another's internal files, bypassing its declared exports surface.
- **Pre-check**: none needed — the message lists the touched files (capped, with elision).
- **Recipe**: per touched file, exactly as the message computes: if the imported thing is
  also reachable through the provider's public surface, switch the import specifier to the
  public path; if it is genuinely internal, either add the subpath to the provider's
  declared surface or extract the shared code — pick the smaller change, and treat widening
  a provider's public surface as an API decision to confirm with the human.

### undeclared

- **Means** (defect · warning): a file imports a package its own manifest never declares —
  works only through hoisting/transitive luck, breaks on clean installs.
- **Pre-check**: `kndo describe dep:<name>` (scopes, importing files) to pick the right
  scope.
- **Recipe**: declare the dependency in the **importing package's own manifest** (in a
  monorepo, a sibling's declaration does not count), in the scope matching real usage —
  prod if production code imports it, dev if only tests/tooling do. Pin the version
  consistently with the rest of the workspace. If the import itself is vestigial, delete the
  import instead.

### version-skew

- **Means** (defect · warning): the same dependency declared with diverging version
  requirements across workspace manifests.
- **Pre-check**: none — the message lists every declaring manifest and requirement.
- **Recipe**: align all declarations to one requirement — normally the newest already in
  use, unless a manifest pins lower for a stated reason (then ask the human). Use the build
  tool's catalog/central version management if the workspace has one. Then run the package
  manager's install/sync so lockfiles regenerate **via tooling, never by hand**.

### cyclic

- **Means** (risk · warning where hazardous — JS/TS module-init-order bugs are real; one
  finding per cycle, anchored at its most-referenced node): two or more files/packages form
  an import cycle. The `evidence:` lines carry the shortest loop.
- **Pre-check**: the evidence chain IS the path; `kndo uses <node> --edges imports` on
  participants if you need more context.
- **Recipe**: break the shortest loop shown, preferring in order: (a) extract the piece both
  sides need into a new module both import; (b) invert one dependency (move the import
  behind an interface/callback the other side registers); (c) merge the files only if they
  are genuinely one module. Cut the weakest-confidence or most-peripheral edge, not the hub.

### duplicate

- **Means** (waste · info): byte-identical files, or same-language callables identical up to
  formatting/renames (clone group; every instance listed as evidence).
- **Pre-check**: read every listed instance before touching any — near-twins may differ
  deliberately.
- **Recipe**: keep one canonical copy — the one in the most shared/upstream location — and
  make the other sites use it (re-import for code, delete extra copies for identical
  files), preserving each call site's behavior. If instances differ semantically despite
  structural identity (deliberate parallel implementations), suppression-with-reason is
  legitimate: duplication is info-severity precisely because it is sometimes intended.

### crap

- **Means** (risk · warning): CRAP score = complexity² × (1 − coverage)³ + complexity above
  threshold (default 30) — complex and under-covered.
- **Pre-check**: the message carries complexity and coverage; `kndo used-by <sel>` tells you
  how load-bearing it is.
- **Recipe**: two levers, in this order: (1) **cover it** — coverage crushes the score
  cubically and is usually the cheaper move; add tests through its public entry point; (2)
  **simplify it** — extract decision-heavy branches into small testable functions. Re-run
  your coverage tooling so kndo re-ingests the report. Never suppress.

### stale

- **Means** (hygiene · info · certain): a `kndo:allow` pragma that is not doing its job —
  unknown category, bound to no declaration, targeting `stale` itself, or matching nothing
  anymore.
- **Recipe**: delete the pragma (or re-aim it, when the message shows a did-you-mean or a
  binding problem). Safe by construction: deleting a stale pragma can never resurrect a
  finding.

### unresolved

- **Means** (defect · reserved): registered in the vocabulary but **no current analysis
  emits it**. If one ever appears, treat it as an import that resolves to nothing: fix the
  import path or the missing declaration, and do not suppress.

### plugin:* (convention group)

- **Means**: a third-party verdict `plugin:<coordinate>/<rule>` — advisory by default; it
  gates only if `kndo.toml` `[plugins.gate]` opts it in.
- **Recipe**: the finding's own `message` is the instruction — follow it as written. If a
  gated plugin finding conflicts with the code's evident intent, raise it to the human
  rather than suppressing; for advisory ones, fix when in scope, otherwise leave.

## Worked loop

```text
kndo check --format agent                 # 1. worklist with ids
# → 1. [kndo-a3f81c92e5d4] unused function src/billing/tax.ts:41 calcLegacyTax
kndo impact src/billing/tax.ts#calcLegacyTax --if-deleted   # 2. pre-check → removal set
# edit: delete calcLegacyTax + its export + newly_unreachable + freed deps
kndo check --diff main --format agent     # 3. verify
# → assert [kndo-a3f81c92e5d4] under fixed:, review anything under new:
```
