# RFC 0018 — Plugin-Contributed Findings

**Status:** Draft (design only — nothing here is implemented, and no phase is scheduled; RFC
0017 §9 committed to this document, not to the capability) · **Depends on:** RFC 0003 (plugin
system), RFC 0005 (analyses & the zero-false-positive standard), RFC 0006 (CLI & gating), RFC
0015 (identity), RFC 0016/0017 (component model & read surface) ·
**Contract impact if accepted:** additive changes to contracts/output-schema.md §2/§6 and a
new plugin-world export in docs/contracts/wasm-abi.md §5

## 1. The capability, and why it is its own RFC

Everything plugins can do today shapes what the core analyses conclude — roots, edges,
classifications, annotations feed RFC 0005's own rules, and the *verdicts* stay kndo's.
The most-demanded next step for a published platform is the opposite: third-party **rules** —
"flag every `api.call('v1/…')` as deprecated", "this company's DI annotations must not be on
private classes", "translations missing from this catalog are findings". The read surface
those rules need already exists (RFC 0017 §5 made it complete over the graph's data model —
its stated prerequisite for this RFC). What does *not* exist is a way for a plugin's verdict
to reach the output.

This is deliberately not a bullet point on another RFC because it touches the one promise
everything else is built on: **core findings have zero false positives** (RFC 0005 §1). A
host cannot audit a third-party rule's precision — it can't even state what the rule *means*.
So the design problem is not "how do plugin findings get emitted" (a sink and a hook, §4) but
"how does an unauditable verdict coexist with an audited guarantee without diluting it". The
whole RFC is the containment design; the plumbing is an afternoon.

## 2. Design pillars

### 2.1 Namespace: `plugin:<coordinate>/<rule>`

A plugin finding's `category` is `plugin:<plugin coordinate>/<rule-name>` — e.g.
`plugin:github.com/acme/kndo-deprecations/v1-api`. Three properties, all load-bearing:

- **Structural distinguishability.** A consumer (human, CI, agent, SARIF ingester) can
  partition core findings from plugin findings with a prefix test, no registry lookup. The
  bare categories of output-schema §6 remain exactly the set the zero-FP statement covers;
  the statement itself gets rescoped in prose to "categories without the `plugin:` prefix" —
  which changes nothing about its content today.
- **Attribution is the id.** The coordinate inside the category is RFC 0015 §2's fetchable
  identity — a finding always names the component that asserted it, in the field consumers
  already section by. `rule-name` is the plugin's own sub-identifier (lower-kebab, same
  charset as core categories).
- **Host-enforced.** The sink (§4) prepends the prefix from the plugin's own registered
  descriptor id; a plugin cannot emit a bare category, an off-namespace category, or another
  plugin's namespace, by construction rather than by review. (Reserved `kndo:` built-ins get
  `plugin:kndo:<name>/<rule>` — built-ins asserting convention rules are held to the same
  channel, not smuggled into the core guarantee.)

`group` for plugin findings is a new additive group, `convention` — output-schema §6 already
obligates consumers to render unknown groups rather than drop them, so this degrades
correctly on old consumers.

### 2.2 The severity channel: advisory by default, gating by explicit opt-in

Plugin findings carry the same `severity` vocabulary (`error`/`warning`/`info`) as declared
by the rule — but **the exit-code gate ignores them by default**. `--fail-on warning` (the
pre-commit and CI shape, RFC 0006 §5) continues to mean "fail on core warnings"; a plugin
finding influences the exit code only when the user opts that rule (or plugin) into gating in
`kndo.toml`:

```toml
[plugins.gate]
"github.com/acme/kndo-deprecations" = "warning"   # whole plugin: gate at its declared severities
"github.com/acme/kndo-deprecations/v1-api" = "off" # per-rule override wins
```

The reasoning is the same as wasm-abi §9's threat model, extended one step: today a malicious
or sloppy component can only *suppress* findings; a finding-emitting component could *break
builds* — a much louder failure mode, and one that would make "install a plugin" a
riskier decision than this platform wants it to be. With the channel split, installing a
finding-emitting plugin is safe by default (its output is visible, sectioned, attributed —
and inert to exit codes), and making it enforceable is the same kind of deliberate,
per-coordinate trust decision as installing it was. Health (RFC 0005 §11) likewise excludes
plugin findings from the score — the score is a claim about the codebase under kndo's own
standards, not under any installed plugin's.

### 2.3 What a plugin finding must carry

The sink accepts only well-formed verdicts, validated host-side:

- **A resolvable target** — the same `PluginTarget` (path, optional symbol) vocabulary as
  every other sink, resolved against the graph; a finding on a nonexistent target is dropped
  silently (the uniform miss behavior, RFC 0003 §2). Locations in the output are therefore
  always real graph objects with real spans where the target has one.
- **`confidence`** — required, same three-level vocabulary (RFC 0005 §1). The docs hold
  plugin authors to the same "silence over a guess" discipline the authoring guide already
  states; the field makes the claim inspectable even though the host can't verify it.
- **`message`** — the human sentence; and the **rule name** (§2.1), which doubles as the
  SARIF `rule.id` (namespaced, so third-party rules never collide with core ones in a SARIF
  store — output-schema §7's mapping needs no change beyond the longer id).
- **Finding id** — the standard recipe (output-schema §5) over the namespaced category, so
  baseline entries, suppressions, and agent fix-assertions work unchanged.

`kndo:allow` inline suppressions and the baseline already key on category + location, so both
mechanisms work for plugin findings with zero new machinery — a team can baseline a newly
installed plugin's existing findings exactly like a newly enabled core analysis.

### 2.4 Determinism and the cache

Rule evaluation is a pure function of the graph and the plugin's content-channel reads —
both already folded into the graph cache key (RFC 0016 §6). Findings are computed *after*
assembly (they are output, not graph state), so the incremental patch is untouched: no new
strip-and-rerun obligation, no snapshot format change. R1 (RFC 0017 §2) applies as-is: rules
read adapter-derived data plus their own inputs, never other plugins' contributions, so a
finding set is identical whatever else is installed. The per-plugin audit record (RFC 0017
§7) gains a `findings` count alongside roots/edges/annotations.

## 3. What this deliberately does not allow

- **No severity `error` in the gate without opt-in** — and even opted-in, a plugin rule
  cannot exceed the severity it declared (a config can lower, never raise).
- **No core-category emission** (§2.1's host-enforced prefix) and **no mutation of core
  findings**: a plugin cannot edit, suppress, or reclassify a core finding — suppression
  stays a *user* action (inline comments, config, baseline). The existing mutation hooks
  already let a plugin prevent core findings honestly, by changing what the graph says.
- **No cross-plugin visibility** (R1) and **no plugin-to-plugin finding composition** —
  "meta-rules over other plugins' findings" would reintroduce composition-order dependence.
- **No auto-fixes.** A finding may *describe* a fix in prose; machine-applicable edits are a
  separate capability with a separate trust model, out of scope.

## 4. Mechanism sketch (informative until this RFC is accepted)

- Native: a fourth read-only hook, `contribute_findings(&GraphView, &ContentView, &mut
  FindingSink)`, default empty; `FindingSink::add(rule, target, severity, confidence,
  message)`. Not a graph-mutation hook: `mutates_graph` stays false for a findings-only
  plugin, preserving its snapshot fast path.
- WASM: one additive export on the `kndo:plugin` world (same evolution shape as every RFC
  0017 §5 addition — old components simply lack it; the host probes and skips), fuel-bounded
  per call like every other hook.
- Descriptor: rules are declared up front — `rules: list<rule-descriptor>` (name, one-line
  description, declared severity) — so `kndo doctor` and `kndo plugin verify` can show what
  a component *may* assert before it ever runs, and the gate config can be validated against
  declared rule names.
- Output: findings render in their own section (RFC 0009's human renderer sections by group —
  `convention` sorts after core groups), carry `delta` in diff modes like any finding, and
  flow to SARIF/agent formats unchanged.

## 5. Open questions (why this stays a draft)

1. **Config surface.** Is `[plugins.gate]` the right shape, or does gating belong in the
   existing `[[rule]]` per-path override system (RFC 0006 §7) so path-scoped opt-in works
   from day one?
2. **Rule versioning.** When a plugin upgrade renames or retires a rule, what happens to
   baseline entries keyed on the old namespaced category? (Probably: they age out as stale,
   the same as a deleted core finding — needs confirming against RFC 0006 §6's baseline
   semantics.)
3. **Budget.** One fuel budget for `contribute_findings`, or a per-rule sub-budget so one
   pathological rule can't starve its siblings within the same component?
4. **Noise ceiling.** Whether the host should cap findings-per-rule-per-run (with a
   truncation diagnostic) so a buggy rule cannot flood the report — leaning yes, mirroring
   the content channel's budget-plus-diagnostic shape.

## 6. Acceptance bar

This RFC graduates from Draft when (a) a real third-party-shaped rule — written as a fixture
plugin, not hypothesized — exercises the full path (declare → emit → render → baseline →
suppress → gate opt-in) and (b) the zero-FP rescoping lands in output-schema/RFC 0005 in the
same change, so the guarantee's wording and the mechanism ship atomically. Until then, the
namespace `plugin:` and the group `convention` are **reserved** — nothing else may claim
either.
