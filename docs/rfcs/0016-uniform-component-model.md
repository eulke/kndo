# RFC 0016 — Uniform Component Model

**Status:** Accepted (design), phased (§8) · **Depends on:** RFC 0002 (adapter contract),
RFC 0003 (plugin system), RFC 0004 (cache), RFC 0015 (identity & installation), ADR 0003
(linking strategy), ADR 0006 (single binary, zero-config) · **Ships:** post-1.0, except the
freeze reservations in §8 phase 0, which must land in M6

## 1. The question this RFC answers

With RFC 0015 fully landed, the extension story has two visible seams:

1. **Capability gap.** The first real convention plugins (`kndo:nextjs`, `kndo:express`)
   documented concrete detections as out of scope *solely because plugins cannot read file
   content*: express's `package.json` `main`/`scripts` parsing (its spec names host-mediated
   content access as "the right long-term fix"), nextjs's `pageExtensions`, route-string →
   page edges, `res.render` → template edges. The capability exists in the descriptor
   (`requested_file_access`, RFC 0003 §2) but is plumbed for `ingest_coverage` only.
2. **Asymmetry gap.** Plugins now have coordinate identity, activation rules, dependencies, a
   global tier, and an installer. `LanguageAdapter`s have none of those — a stated gap in RFC
   0003 §4. An external adapter works only hand-dropped per-project; it cannot be installed,
   depended on, or gated.

Behind both sits a strategic question worth answering explicitly rather than by drift:
**should languages themselves become plugins, with kndo reduced to a shell?**

This RFC's answer: **converge the *contracts*, not the *linkage*.** Every extension —
language or ecosystem — becomes a *component*: one identity scheme, one activation model, one
dependency mechanism, one installer, one doctor surface. Whether a given component is
statically linked or loaded as WASM stays a distribution detail, invisible at every one of
those surfaces. The "kndo as a shell" build becomes a supported, CI-proven *configuration* —
not the shipped default.

## 2. Why the shell must not be the default

Each argument is an existing, load-bearing decision; this section only connects them:

- **No third linkage exists.** Rust has no stable native ABI (ADR 0003): "native, dynamically
  loaded" plugins are not on the table. The real choice per component is statically linked
  (full speed, monomorphized, in the default binary) or WASM (sandboxed, marshalled,
  fuel-metered). Adapters are the hot path — tree-sitter over every file, `resolve` per import
  — and the M4.5 performance budget was won with native adapters under rayon. Moving
  first-party languages to WASM would tax every user to benefit none.
- **Zero-config is the product** (ADR 0006). A shell that fetches languages on first run puts
  network inside pre-commit/CI/air-gapped runs — the exact download-on-demand model ADR 0006
  already rejected. A shell that pre-bundles everything is today's binary renamed.
- **Determinism is simplest when the binary is the hash.** "Same inputs ⇒ same findings" with
  a dynamically composed set is contingent on what's installed; `plugins.lock` + doctor keep
  it auditable, but the default path shouldn't need the audit.

What the vision *actually requires* is already true at the layering level: `kndo-core` knows
no language (RFC 0001's ignorance rule), and the `kndo` crate is pure composition over feature
gates. The gap is not architecture — it is that the two extension kinds have unequal contracts
and that the shell configuration is possible but unproven. §§3–7 close exactly that.

## 3. The component contract

A **component** is: a descriptor + one of the two capability traits.

```text
ComponentDescriptor (conceptual — realized as PluginDescriptor and AdapterDescriptor):
    id:                     coordinate identity (RFC 0015 §2: kndo:* reserved, or source coordinate)
    version:                semver string
    activation:             Vec<ActivationRule>   — gates the global tier (RFC 0003 §4)
    dependencies:           Vec<coordinate>       — co-install + co-activate fixpoint (RFC 0015 §3)
    requested_file_access:  Vec<glob>             — the §5 content channel's scope, and part of §6's cache key
```

`PluginDescriptor` already has all five fields. `AdapterDescriptor` gains `id`, `activation`,
and `dependencies` (§4); its existing extension claims remain its file-claiming mechanism,
untouched. The traits stay two: parsing/resolution/manifests belong to `LanguageAdapter`,
graph hooks to `Plugin` — merging them would either weaken the adapter contract into
uselessness or widen the plugin contract until it *is* the adapter contract renamed. "Adapters
describe what code is; plugins describe what an ecosystem means by it" survives this RFC
intact; what dissolves is every *operational* difference between the kinds.

Uniformity across linkage is the invariant to protect: a statically linked component and an
installed WASM component must be indistinguishable at the descriptor, activation, doctor,
dependency, and (where applicable) installer surfaces. First-party components are simply
components whose distribution happens to be "compiled into the default binary."

## 4. Adapters become installable components

The concrete closure of RFC 0003 §4's stated gap:

1. **Identity.** First-party adapters take reserved ids (`kndo:go`, `kndo:typescript`, …);
   external adapters use source coordinates, with §2's identity binding enforced at install
   exactly as for plugins. The loader's `kndo:` rejection extends to the adapter ABI.
2. **Activation.** `AdapterDescriptor.activation` with the same two rules. Semantics per tier
   mirror plugins: compiled-in and project-local adapters with empty rules stay always-on
   (their extension claims already scope their work — an adapter claiming `.go` files is
   dormant in a Go-less repo at near-zero cost); **globally installed** adapters require a
   matching rule to join composition, and empty means never self-activate — the same
   silence-over-guessing default.
3. **Installation.** `kndo plugin install` grows to accept adapter components. The installer
   (`kndo::plugin_install`) needs no structural change — fetch, checksum, identity binding,
   lockfile are kind-agnostic; the probe tries both loaders the way `kndo::open` already does
   for project-local files. The command name stays `kndo plugin` — one more reason "component"
   is the right internal word, but renaming the user-facing verb is churn without benefit.
4. **Claim conflicts get one deterministic rule.** Composition order for file claims:
   project-local externals > globally installed externals > compiled-in, ties broken by id.
   Today's implicit "externals first" behavior becomes written contract; doctor shows which
   adapter won a contested extension and why.

## 5. The content channel: `requested_file_access` for graph hooks

The highest-value extension, and the one both shipped plugin specs already point at.

- **Shape.** The four graph hooks gain access to a read-only `ContentView`: the set of project
  files matching the component's declared globs (evaluated by the same gitignore-aware walker
  as everything else — `node_modules` etc. excluded like everywhere), with lazily loaded
  bytes. No ambient filesystem, no paths outside the declared globs, no writes — the WASM
  bridge exposes it as one host import (`read-file(path) → option<bytes>`), host-enforced
  against the descriptor, so the sandbox story (ADR 0003) is unchanged. Native components get
  the same API; the tiers stay behaviorally identical.
- **Budgets.** Per-component caps on files opened and total bytes (numbers set at
  implementation time, from the 50k-fixture benchmark); exceeding them disables the component
  for the run with a diagnostic — the same posture as the WASM fuel budget. Declaring a glob
  is cheap; *reading* is what's metered.
- **What it unlocks, immediately:** express reading `package.json` `main`/`scripts` instead of
  guessing entry files by name; nextjs honoring `pageExtensions` and config; route-string →
  page edges from `<Link href>`; `res.render("name")` → `views/**` template edges (which also
  makes templates *claimed*, so unused-template detection becomes expressible); Spring-style
  annotation scans over XML/properties config.
- **The boundary that keeps this from eroding RFC 0002:** the channel is for files *outside
  the language graph* — configs, manifests, templates. A plugin parsing `.ts` sources to
  second-guess the TypeScript adapter is out of contract; the enforcement is review-level
  (spec-per-plugin, as `docs/plugins/*` already practices), not mechanical, and stated here so
  it can be pointed at.
- **Determinism note:** content-derived contributions are already correct under the
  `mutates_graph` bypass (RFC 0003 §5) — every run re-reads. §6 is what makes them *fast*.

## 6. Cache-key folding — the performance gate for a component-heavy world

Today any graph-mutating component forfeits both the snapshot cache and the incremental patch
(RFC 0003 §5, wasm-abi §5.4) — correct, and acceptable while such components are rare. In the
world §§4–5 create, matching projects would full-rebuild every run. Before the shell
configuration can be claimed as supported, this lands:

1. **Component identity folds into the graph cache key** (RFC 0004 §3's original design):
   compiled-in components contribute id+version (the binary's own version subsumes their
   content); WASM components contribute id+version+content hash. Enable/upgrade/remove
   invalidates exactly what changed.
2. **The content channel records its read set**: (path, content hash) per component, stored
   with the snapshot. A snapshot or patch is servable only if every recorded read is
   unchanged — the same file-hash discipline the patch already applies to source files,
   extended to channel reads. Glob *result* changes (a new file matching a declared glob)
   are caught because discovery output is already part of the key.
3. **Only then** does the blanket `mutates_graph` bypass narrow: components whose inputs are
   fully captured by (1)+(2) allow snapshot/patch reuse; anything else keeps the bypass. The
   bypass remains the correctness backstop, never removed — only earned past.

## 7. Smaller alignments

- **`suppress` gets wired or cut.** Declared since RFC 0003 §2, still uncalled. This RFC's
  position: wire it in the reporting phase for components, with suppressions attributed and
  counted (a suppressed finding is reported as suppressed-by under `--verbose`, RFC 0005 §12's
  transparency posture) — or, if no shipped component needs it by the time phase 3 closes,
  delete the hook before it fossilizes as dead ABI surface. Decide with a real use case on the
  table, not by default.
- **`GraphView` widens only against demand**: package/unit topology (which unit is this file
  in — nextjs's app-root logic re-derives this today) and import-edge queries ("which files
  reference X" — route/template edge validation wants it) are the two candidates with a known
  consumer. Each addition is judged against the ABI freeze individually; nothing widens
  speculatively.
- **The shell build becomes CI-proven.** A workspace job builds
  `--no-default-features --features external-adapters,plugin-install` and runs a smoke check
  (open a fixture project with adapters loaded purely from a plugin dir). That artifact *is*
  the "kndo as cascarón" configuration — real, tested, and one flag away for embedders — while
  the default binary keeps ADR 0006's promise.

## 8. Phases

0. **Freeze reservations (M6, before ABI/schema freeze):** none of §§4–6 needs to ship at
   1.0, but the freeze must not wall it off. Concretely: wasm-abi §8's versioning note gains
   the planned additive evolutions (adapter-world descriptor export; plugin-world `read-file`
   host import) so they're declared forward-compatible extensions, not breaking changes;
   `AdapterDescriptor` gains the dormant `id`/`activation`/`dependencies` fields native-side
   (cheap, invisible to behavior) so first-party adapters can populate them without a contract
   break later.
1. **Content channel (§5)** — highest value per unit of new surface; upgrades `kndo:express`
   and `kndo:nextjs` from their documented approximations, which also makes it the phase with
   built-in dogfood.
2. **Adapter componentization (§4)** — identity, activation, installer acceptance, claim-order
   contract, doctor parity.
3. **Cache-key folding (§6)** — measured on the 50k fixture; closes with the shell CI job
   (§7) turning on.
4. **`suppress` decision + GraphView additions (§7)** — demand-gated, possibly empty.

Order matters: 1 before 2 because installable external adapters are more attractive once the
plugin side demonstrates the full component surface; 3 before the shell is advertised because
a shell that full-rebuilds every run would demo badly and deserve it.

## 9. Explicitly out of scope

- **Analyses as components.** The zero-false-positive bar is enforceable because the core owns
  analysis semantics end to end (RFC 0003 §6's deliberate post-1.0 deferral of custom
  analyses; unchanged here). "Shell" never means the analyses move out.
- **Output formats as components** — unchanged from RFC 0003 §6, same schema-stability reason.
- **`dlopen`/cdylib native loading** — re-rejected for the reasons in ADR 0003; nothing in
  this RFC creates new pressure for it.
- **Making WASM the default for first-party anything** — §2 is the argument; revisit only if
  the WASM tier's measured overhead becomes negligible on the 50k fixture.
- **Splitting first-party components out of the workspace/repo.** Separate distribution ≠
  separate development: conformance fixtures, cross-adapter tests, and atomic contract changes
  (the standing rule: contract and doc change in the same PR) all depend on co-location.
