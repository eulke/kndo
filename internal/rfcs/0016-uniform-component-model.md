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

`PluginDescriptor` already has all five fields. `AdapterDescriptor` already had `id`; it gains
`activation` and `dependencies` (§4 — landed; `version` was never added, §4's own note on why).
Its existing extension claims remain its file-claiming mechanism, untouched. The traits stay two: parsing/resolution/manifests belong to `LanguageAdapter`,
graph hooks to `Plugin` — merging them would either weaken the adapter contract into
uselessness or widen the plugin contract until it *is* the adapter contract renamed. "Adapters
describe what code is; plugins describe what an ecosystem means by it" survives this RFC
intact; what dissolves is every *operational* difference between the kinds.

Uniformity across linkage is the invariant to protect: a statically linked component and an
installed WASM component must be indistinguishable at the descriptor, activation, doctor,
dependency, and (where applicable) installer surfaces. First-party components are simply
components whose distribution happens to be "compiled into the default binary."

## 4. Adapters become installable components — Landed

The concrete closure of RFC 0003 §4's stated gap.

1. **Identity, landed narrower than first drafted.** External adapters use source coordinates
   (`github.com/<owner>/<repo>`), with §2's identity binding enforced at install exactly as for
   plugins (`kndo::plugin_install::wasm_probe` tries the plugin loader, then the adapter loader,
   and identity binding runs on whichever accepts the bytes). The loader's `kndo:` rejection
   extends to the adapter ABI (`WasmAdapter::load` now checks `is_reserved_id`, mirroring
   `WasmPlugin::load`). **First-party adapter ids stay as they are** (`"js-ts"`, `"go"`, …) — not
   renamed into the `kndo:` namespace. Nothing requires the rename for the protection to work:
   the reservation is "no *external* component may claim an id starting with `kndo:`,"
   independent of what compiled-in ids actually are, and compiled-in adapters never pass through
   the loader that rejection lives in. Renaming would only churn the graph cache key
   (`compute_graph_key` hashes adapter ids) for zero behavioral gain — deferred indefinitely, not
   just to this phase.
2. **Activation.** `AdapterDescriptor.activation`, wired end to end: a new `activation-rule`
   variant plus `activation`/`dependencies` fields on `adapter.wit`'s `adapter-descriptor`
   (duplicated from `plugin.wit`'s own type, same cross-package-independence reasoning that
   module already documents), read by the host bridge (`host.rs`'s `native_descriptor`).
   `dependencies` rode the wire unevaluated at this phase's landing — no adapter had needed
   cross-adapter implication yet, and wiring a fixpoint nothing exercised would have been
   exactly the speculative machinery this project's standing rules reject. (RFC 0017 §6 later
   evaluated it, under the platform criterion, with the plugin tier's own fixpoint made
   kind-neutral.) Semantics mirror plugins exactly: compiled-in and project-local
   adapters are unconditional regardless of `activation` (their file-extension claims already
   scope the cost — an adapter claiming `.go` files is dormant in a Go-less repo); a
   **globally installed** adapter requires a matching rule to join composition, empty rules
   never self-activate globally (`kndo::compose_adapters`, reusing the plugin tier's
   `activation::activates`/`global_plugin_dir` machinery in `crates/kndo/src/lib.rs`).
3. **Installation.** `kndo plugin install` accepts adapter components with no structural
   change to `kndo::plugin_install` — fetch, checksum, identity binding, and the lockfile were
   already kind-agnostic (`ProbedDescriptor` never carried anything plugin-specific); only the
   probe closure grew a fallback try. The command name stays `kndo plugin`.
   `AdapterDescriptor` gains no `version` field — the release *tag* already is the version
   (`plugins.lock` records it directly; `ProbedDescriptor.version` turns out to be unused by
   the install pipeline for either kind, discovered auditing this, not by design) and nothing
   inside the descriptor needs to restate it.
4. **Claim conflicts get one deterministic rule — and the *actual* prior behavior was the
   opposite of this RFC's first draft.** Composition order for file claims is now project-local
   externals > globally installed externals > compiled-in, ties within a tier broken by id
   (`kndo::compose_adapters`). Auditing the code before writing this down found the pre-existing
   order was **compiled-in first, externals appended after** — meaning a project-local adapter
   could never win a contested extension against a built-in one, silently. That's corrected
   here, not merely documented: presence in `.kndo/plugins/` is deliberate, strong opt-in signal
   (RFC 0003 §3's own framing), and a user who drops a custom adapter there almost certainly
   means to override, not to be silently shadowed. `kndo doctor`/`kndo::adapter_resolution`
   show the composed order so which adapter would win a contested extension is inspectable, not
   just implied by list position.

## 5. The content channel: `requested_file_access` for graph hooks — Landed

The highest-value extension, and the one both shipped plugin specs already pointed at.
Implemented in full (`kndo_core::plugin::ContentView`, `crates/kndo-plugin-api`'s `read-file`
host import — wasm-abi.md §5.1/§5.3/§8); two amendments from this section's original design,
made at implementation time and recorded here rather than left as silent drift:

- **Three hooks, not four.** `contribute_roots`/`contribute_edges`/`annotate_symbols` gain
  `content: &ContentView<'_>`; `classify_file` does not. It runs once per *file* across every
  registered component (`graph.rs`'s phase-2 loop, `O(files × components)`), where the other
  three run once per *component* per round — giving it the same channel would mean
  instantiating a WASM guest's content snapshot on that hot path for a use case nothing has
  asked for. Revisit if a real `classify_file` consumer needs it.
- **The boundary is narrower than "outside the graph" alone suggests.** The channel is for
  files the language graph doesn't itself claim and parse — configs, manifests, templates.
  What it does *not* cover, even though nothing stops a component from trying: reading a
  claimed source file to extract a fact the adapter itself owns. Concretely, this ruled out
  two items this section originally listed as unlocked — route-string → page edges from
  `<Link href="...">` and `res.render("name")` → template edges both require parsing `.tsx`/
  `.jsx` *source*, which the JS/TS adapter already claims; second-guessing it through the
  content-channel side door is exactly RFC 0002's boundary this RFC promised not to erode
  (docs/plugins/nextjs.md §5 records the final call). What *did* land as real consumers:
  `kndo:express` reading `package.json` `main`/`scripts` instead of guessing entry files by
  name (docs/plugins/express.md §3/§4), and `kndo:nextjs` statically reading a literal
  `pageExtensions: [...]` array out of `next.config.*` to narrow which extensions count as
  routed (docs/plugins/nextjs.md §5) — both bounded, both degrade to the pre-channel behavior
  on anything they can't statically read.
- **Shape, as built.** Glob matching runs in memory against paths this run already discovered
  (no second disk walk; source-blind — identical behavior for a directory or an in-memory git
  tree, RFC 0004 §6) rather than against the real filesystem the way `ActivationRule::
  FileExists` does, so a recursive glob like `**/package.json` never touches `node_modules`
  regardless of gitignore state. The WASM bridge prefetches every glob-matched path into an
  owned snapshot before instantiating each round's guest (`HostViewData`, mirroring how it
  already snapshots `list-files`/`symbols-in`); `read-file` on the guest side is a lookup into
  that snapshot, never a live call.
- **Budgets.** Per-component caps on distinct paths read and total bytes (`CONTENT_MAX_FILES`/
  `CONTENT_MAX_BYTES` in `kndo_core::plugin`) — conservative constants for the channel's
  stated scope (configs/manifests/templates, never source), not derived from a benchmark
  sweep; exceeding them cuts the component off from further reads for the rest of the run,
  with one diagnostic recording why — the same posture as the WASM fuel budget. Accounting is
  keyed by *path*, not by call: a component's read scope shouldn't depend on how many hooks
  look at the same file. (When this landed, the keying also compensated the WASM bridge's
  then-current instance-per-hook triple-fetch; RFC 0017 §4's one-instance-per-round lifecycle
  removed that motivation, and the keying stays on its own merits.)
- **Determinism note:** content-derived contributions are already correct under the
  `mutates_graph` bypass (RFC 0003 §5) — every run re-reads. §6 is what makes them *fast*.

## 6. Cache-key folding — the performance gate for a component-heavy world — Landed

Before this landed, any graph-mutating component forfeited both the snapshot cache and the
incremental patch (RFC 0003 §5, wasm-abi §5.4) — correct, and acceptable while such components
were rare. In the world §§4–5 create, matching projects would full-rebuild every run. Landed:

1. **Component identity folds into the graph cache key** (RFC 0004 §3's original design, now
   in `compute_graph_key`): every registered *graph-mutating* plugin contributes its id and
   declared version; WASM plugins additionally contribute the component's own content hash
   (`blake3` over the `.wasm` bytes, computed once at load and exposed via a new
   `Plugin::content_hash()` default-`None` method — compiled-in plugins need no content term
   because the binary's own version already subsumes their code). Enable/upgrade/remove of a
   plugin now invalidates exactly the snapshots it could have touched, sorted by id so
   composition order never perturbs the key.
2. **No separate content-channel read-set tracking was needed** — the RFC's original draft (2)
   assumed one would be, but implementation-time analysis found the existing key already closes
   the gap: `compute_graph_key` folds in `discovered_files`, the full unfiltered discovery
   output, and `ContentView::read` (plugin.rs) can only ever return a path already inside that
   same discovered set — it never reads outside the project tree it was handed. So every file a
   content-channel read could observe already has its content hash in the key via the discovery
   term; a change to that file was already a cache miss before this phase, with zero added
   machinery. Building dedicated per-component read-set bookkeeping would have duplicated
   information the key already carries. Glob *result* changes (a new file matching a declared
   glob) are likewise already caught, for the same reason.
3. **The blanket `mutates_graph` bypass narrows, but only on the snapshot path.** A
   graph-mutating plugin's presence no longer forces `graph_key`'s cache lookup/write to be
   skipped — (1)+(2) make the key itself sufficient to detect any input change, so
   snapshot reuse (`cache.get_graph`/`graph_writer`) is now unconditional. At this phase's
   landing, the incremental *patch* path (`try_patch`) stayed bypassed whenever any
   graph-mutating plugin was registered: a patch mutates an existing graph in place from a
   source-file diff alone, and proving a plugin's hook output composes correctly with a
   partial re-derivation was a materially harder claim than "the whole snapshot is either
   valid or it's rebuilt" — not attempted here. *(Subsequently closed: RFC 0017 §3 removed
   that bypass structurally — the patch strips provenance-tagged plugin contributions and
   re-runs the round, guarded by a snapshot-stored plugin-set digest — so the composition
   proof this paragraph declined to attempt was never needed.)* The posture is the same as
   phase 0–2: earn scope incrementally, keep the bypass as the correctness backstop wherever
   the narrower claim isn't proven, never remove it wholesale.

Performance consequence: a plugin-bearing project's *first* run after a plugin changes still
full-rebuilds (no different from before), but every unchanged repeat run now takes the snapshot
path instead of forced-bypass — the same cost as a plugin-free project's warm run. The existing
50k-fixture baseline (`internal/perf-baseline.json`) already measures that path: `50k/cold-full` is
7333.1ms, `50k/warm-noop` is 600.8ms. Those numbers weren't re-measured with a plugin attached
because they don't need to be — the warm-run code path a plugin-bearing project now takes on a
no-op re-run is the identical `cache.get_graph` hit already covered by `50k/warm-noop`, not a
new one; the mechanism, not the fixture composition, is what determines the cost.

## 7. Smaller alignments — decided

- **`suppress` is cut, not wired.** Declared since RFC 0003 §2, never called. This phase's
  review found no shipped component — `kndo:nextjs`, `kndo:express`, or the reference examples —
  needs domain-specific suppression: every exemption those two plugins' own detections require
  is already reachable through `classify_file`/`contribute_roots` narrowing what gets analyzed
  in the first place, not through suppressing a finding after the fact. Building a reporting-side
  hook against a use case that doesn't exist yet is exactly the speculative surface this RFC's
  own freeze discipline (§8 phase 0) argues against adding. The decision is closed, not merely
  deferred: `suppress` is not part of either WIT package (wasm-abi §0/§5.2 already said so) and
  stays undeclared on the native `Plugin` trait too. A real use case reopens this — nothing about
  the freeze forecloses adding it later as a new, additive hook — but none exists today.
- **`GraphView` does not widen.** Both candidates named in the original draft were evaluated
  against an actual consumer, not a hypothetical one, and neither clears the bar:
  - *Package/unit topology* — motivated by `kndo:nextjs`'s `conventions::app_roots`, which scans
    every discovered path for a `package.json`/`next.config.*` anchor and takes its directory.
    `FileNode` already carries `package: PackageId` and `unit: Option<SmolStr>` (RFC 0011 §3,
    RFC 0012 §6), so exposing the package table through `GraphView` would let a plugin ask "which
    package owns this file" — but only the `package.json` half of `app_roots`' two anchors maps
    onto core's manifest-derived package boundary; `next.config.*` is a Next.js-specific
    convention core has no reason to treat as a package boundary. Widening `GraphView` here would
    add ABI surface without letting `app_roots` actually delete its own scan — a partial,
    speculative win, not an earned one.
  - *Import-edge queries* ("which files reference X") — motivated by route-string → handler and
    template → class edges. §5's own landed scope explicitly keeps both out of the content
    channel's contract (a graph-claimed source file read through the content side door, not a
    file outside the graph) — the motivating consumer was scoped out in phase 1, before this
    phase started. There is no current caller left to widen `GraphView` for.
  Both stay open for a future RFC with a real, landed consumer on the table — not ruled out,
  just not built speculatively now.
- **The shell build is CI-proven.** `.github/workflows/ci.yml` gained a standalone `shell-build`
  job: `cargo build -p kndo-cli --no-default-features --features external-adapters,plugin-install`
  (plus its own `cargo clippy -D warnings` pass), then a smoke check — the reference external
  adapter (`examples/kndo-plugin-demo`, M5's own exit-bar fixture) built to `wasm32-unknown-unknown`
  and componentized via a new `cargo xtask componentize` step (the same
  `wit_component::ComponentEncoder` call `crates/kndo/tests/external_adapter.rs` already makes
  in-process, exposed as its own dev-time command so CI needs no extra tool install), dropped
  into a fixture project's `.kndo/plugins/` with zero first-party adapters compiled in, and
  `kndo doctor`/`kndo check` asserted to auto-discover and run it. This surfaced one real,
  previously-latent bug: `kndo::default_adapters`'s `let mut adapters` was unconditionally
  mutable across every feature combination, which is only true when at least one language
  feature is on — with all eight off (the shell configuration exactly), the `mut` is unused and
  `rustc` warns. Fixed with an `#[allow(unused_mut)]` alongside the existing
  `#[allow(clippy::vec_init_then_push)]`, not by adding a language back. `crates/kndo-cli`
  gained its own `[features]` table (`default-features = false` on its `kndo` dependency, one
  pass-through feature per `kndo` feature) — without it, `--no-default-features` on `kndo-cli`
  had nothing of its own to disable and `kndo`'s defaults would activate regardless of what
  `kndo-cli`'s own flags said. That artifact *is* the "kndo as cascarón" configuration — real,
  tested on every push, and one flag away for embedders — while the default binary keeps ADR
  0006's promise unchanged.

## 8. Phases

0. **Freeze reservations (M6, before ABI/schema freeze):** none of §§4–6 needs to ship at
   1.0, but the freeze must not wall it off. Concretely: wasm-abi §8's versioning note gains
   the planned additive evolutions (adapter-world descriptor export; plugin-world `read-file`
   host import) so they're declared forward-compatible extensions, not breaking changes;
   `AdapterDescriptor` gains the dormant `id`/`activation`/`dependencies` fields native-side
   (cheap, invisible to behavior) so first-party adapters can populate them without a contract
   break later. **Landed**: wasm-abi §8 carries the two declared extensions;
   `AdapterDescriptor.activation`/`.dependencies` exist (empty everywhere, unread) — `id`
   needed no new field, only the §4 migration note on the existing one, deferred to phase 2
   because renaming ids churns cache keys.
1. **Content channel (§5) — Landed.** Highest value per unit of new surface; upgrades
   `kndo:express` and `kndo:nextjs` from their documented approximations, which also made it
   the phase with built-in dogfood — both plugins' own baseline-then-plugin fixture suites
   (`crates/kndo/tests/builtin_convention_plugins.rs`) grew a scenario apiece proving the
   content-derived rescue actually fires, plus native (`kndo-core`) and WASM
   (`kndo-plugin-api`'s compliance suite, `examples/kndo-plugin-hooks-demo`) round-trip tests
   for the channel mechanism itself.
2. **Adapter componentization (§4) — Landed.** Identity (loader rejection of the reserved
   namespace), activation (wired end to end including the WIT wire format), installer
   acceptance, the corrected claim-order contract, and doctor parity
   (`kndo::adapter_resolution`/`global_adapter_candidates`, `kndo doctor`'s new sections).
   Proven at every layer: `kndo-core` compiles under both the full and `--no-default-features
   --features js` builds unchanged; `crates/kndo/tests/global_adapter_activation.rs` proves the
   global-tier gate and the claim-priority ordering against a real WASM component; `crates/
   kndo/tests/plugin_install_probe.rs` proves the installer's dual probe against a real adapter
   component. Closing this phase also surfaced and fixed a real, if narrow, pre-existing
   concurrency bug in `kndo::plugin_install::wasm_probe`'s temp-file naming (PID-only, so two
   concurrent calls in one process could race on the same path) — found because this phase's
   own test suite was the first caller to exercise `wasm_probe` from two `#[test]`s in the same
   binary.
3. **Cache-key folding (§6) — Landed.** `compute_graph_key` folds in every graph-mutating
   plugin's id, declared version, and (WASM only) component content hash, sorted by id;
   snapshot reuse (`cache.get_graph`/`graph_writer`) is unconditional now — a narrower,
   honestly-scoped claim, not the full read-set-tracking design originally drafted in §6(2),
   which implementation-time analysis showed was already subsumed by the existing
   `discovered_files` term. The half this phase left bypassed — the incremental patch — was
   subsequently closed by RFC 0017 §3 (strip & re-run, with the plugin-set digest guard);
   `crates/kndo-core/src/graph.rs`'s
   `the_patch_re_derives_plugin_contributions_instead_of_bypassing` proves the combined
   result, and `compute_graph_key_distinguishes_wasm_plugin_content_from_its_own_id_and_version`
   the key term.
4. **`suppress` decision + GraphView additions + shell CI job (§7) — Landed.** `suppress`
   decided cut (no shipped consumer); both `GraphView`-widening candidates evaluated and left
   unbuilt (neither had a real, current, landed consumer — see §7 for each); the shell build is
   now a standalone, CI-proven `shell-build` job (`.github/workflows/ci.yml`) with its own
   smoke check, plus the `crates/kndo-cli` feature-passthrough and `cargo xtask componentize`
   plumbing that job needed to exist at all.

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
