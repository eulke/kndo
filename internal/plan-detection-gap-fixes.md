# Plan: root-cause fixes for the correctable detection gaps

**Branch:** `claude/core-api-ergonomics-architecture-983pom` ·
**Status:** W1–W5 and W7 landed; **W6 is the only item left** ·
**Companion:** `internal/detection-gaps.md` (the catalogue this plan works through)

This file exists so the work survives a session boundary. It records what the plan is, what
each landed item actually changed and what it measured, what remains, and — most importantly —
the rules that govern how the remaining work must be done. Those rules were not obvious: three
of them were learned by getting them wrong first, and they are written down here so the next
person does not pay for that twice.

---

## 0. Rules that govern every item

These override any instinct to the contrary. The first four come from `CLAUDE.md` and RFC 0012;
the rest were established during this plan's execution.

1. **Fix, don't catalogue.** Anything found that needs correcting gets corrected. A contract
   signature change is not a reason to defer. The only bar is that the solution be root-cause,
   architecturally correct and ergonomic. `internal/detection-gaps.md` is for real limits we
   decide not to cross — never for debt nobody will look at again.

2. **Layering, in the exact words that settled it.** *Anything that resolves a generality of a
   language — a mechanism the language itself must offer — belongs to the **adapter**.
   Knowledge specific to one concrete **tool** belongs to that tool's **plugin**. And when the
   language has no way to know something, what the **core** offers is the DYNAMIC that lets the
   language supply it* — flexible enough that not every language must adopt it. The precedent
   to imitate is visibility: every language scopes differently, and the core offers
   `VisibilityScope` + the declared ladder so each adapter says its own truth.

   Two corollaries, both learned by violating them:
   - A grammar-specific scanner must never live in `kndo-adapter-toolkit`. That crate is
     language-agnostic **and** its audience is adapters.
   - There is no such thing as a "Rust conventions toolkit" plugin crate. Either it is N
     plugins, one per concrete tool, or the thing being shared is language knowledge and
     belongs to the adapter with a core-side dynamic.

3. **Ignorance rule.** The core never names a language. An `if language == "go"` in core
   reverts the PR. If a feature seems to need one, the real gap is missing vocabulary.

4. **Degrade toward keep-alive, never toward accusation** (RFC 0012 §2). A marking mechanism
   that over-matches keeps a symbol alive; that is the safe direction. One that under-matches
   accuses. Prefer the former every time, and say so in the code.

5. **Measure, don't assume — and instrument rather than reason.** This overturned a stated
   premise three separate times during this plan (Rust's unit key, Rust's `private` scope,
   confidence-as-reconstructed). Run both variants from ONE binary behind a temporary env
   switch so the comparison cannot drift. Diff findings by the multiset of
   `(category, message)` — **finding ids are not unique** (54 duplicates in serde).

6. **Every regression test is verified failing without its fix**, by disabling the fix and
   re-running. Do this with a *file copy*, never `git checkout` — that command silently ate
   this session's work twice.

7. **Non-negotiable gates**, checked by name in CI on every PR touching graph/cache/analysis:
   `patch_equivalence`, cache equivalence, `--threads 1` vs default determinism, and the 72
   adapter `expected.json` fixtures byte-identical. A fixture diff is either a bug in the
   change or a deliberate, documented contract change — never something to fix by regenerating.

8. **Contract-first.** A change to a contractual signature updates
   `internal/contracts/core-traits.md` (and the relevant RFC and `internal/adapters/*.md`) in
   the *same* commit. Bump `GRAPH_SCHEMA_VERSION` and the adapter's `facts_schema_version` on
   any shape or semantics change.

9. **One commit per item**, with its field measurement in the message.

---

## 1. Landed

### W1 — `version-skew` over coordinates with no comparable version (`5630670`)
The root cause was a **parsing** bug, not a comparison policy: `gradle_dependency_line` split
every coordinate on its last `:`, and a BOM-managed coordinate has two segments, so an
*artifact id* was read as a version. `version_req` became `Option`, and placeholders resolve
against the manifest's own property pool. petclinic 5→0, coroutines 4→0, Exposed 9→4 (the
remaining four are real).

### W7a — §16 was stale, zero work
Java's `unit` is the declared package only, never derived from the directory; retrofit's three
multi-release variants all declare `package retrofit2;`, so they are same-unit twins, and the
twin mechanism landed after the audit. Fixtured as `multi-release-variants`.

### W2 — the submodule imported between braces (`cf6157d`, `0f5902a`)
`use crate::internals::{attr, check};` + `check::check(..)` bound nothing. Phase 3b split into
three passes — `resolve_imports` → `link_module_bindings` → `resolve_references` — with a single
hop: a binding of file *i* that resolves to file *F* gains F's own module bindings as
qualifiers. serde 363→348, alacritty `unused` 155→150 with six newly-reachable findings. The
second commit removed the `rsplit("::")` fallback; the two shapes that depended on it now
declare `local_alias` from the adapter.

### W3 — field recall on inferred-typed locals
`RawMemberType::owner` became `Option<SmolStr>`, where `None` means *free function* — "calling
this evaluates to T", the same statement about a value's type that `Some(owner)` makes about a
member, so the core walks it with the same chain machinery. Three of §3's four cases closed;
kndo on itself held at 37/96.6 with **three pragmas fewer**.

### W3-bis — the chain carries a type, not a name
`TypeExpr` replaced `yields` + `yields_params`, because flattening was the root cause and not a
representation detail. Substitution at the hop, `AdapterDescriptor::builtin_member_types` as a
second tier for language-provided types, and `.@element` as an ordinary declared member so the
core never interprets the chain. Closed §3's fourth case; the `gitutil.rs` pragma is gone.

> rkyv 0.8 cannot derive `Archive` for a recursive type. The contract stays a real tree and the
> flattening happens only at the archive boundary, in `rkyv_support.rs` — the module whose doc
> already states that job. Probe this kind of risk on day one, as the plan demanded.

### W4 — the missing visibility rung
Rust's `private` is **module-and-descendants**, not file — the docs said so while mapping it to
`File`. `VisibilityScope::Module`, `FileFacts::unit_parent`, `Declaration::visible_in_unit`, and
`region_covers` (region ⊆ region) replacing both the gate and a three-way ordering that was
quietly wrong. The `rung_surface_transitive` gate came out.

### W4-bis — a synthesized import cannot outrank what the file declares
Rust forbids a `use` from shadowing a local declaration (E0255), so only *reconstructed* imports
can collide — and they were taking a precedence the language never grants. My assumption that
confidence already encoded this was **false and verified false**: `emit_path`'s synthetic import
is `Confidence::Certain`. The fact got its own name, `RawImport::reconstructed`, and both
consumers read it. Name resolution went from four tiers to five.

### W5 — §1 and §2: dispatch through a trait the graph cannot see (`c6b1cbd`, and `1ed9315`)
**This is the item that taught rule 2**, in two wrong attempts before the right one.

The Rust adapter already knew which trait's `impl` declares each member — `handle_impl` reads it
to decide `implicitly_invoked` — and threw the name away. So `kndo:serde` re-parsed a grammar
its adapter had already parsed. Hoisting that scanner into `kndo-adapter-toolkit` was wrong
(agnostic crate, adapter audience); inventing `kndo-plugin-toolkit-rust` was wrong too.

The fix names the fact instead: **`Declaration::implements` / `SymbolNode::implements`** — the
trait/protocol whose implementation declares this member. A fact, never a verdict; `None`
wherever a language has no such grouping. The dynamic is
**`AnnotationSink::mark_machinery_impls(graph, drives)`**: the walk belongs to the core, the
curated table to the plugin. Each conventions plugin is now its table plus one call and requests
**no file access at all**. Third parties reach the same fact through `symbol-implements`, an
additive WIT import. Both `kndo:allow-file untested` pragmas (`plugin_host.rs`,
`rkyv_support.rs`) are deleted; without the plugins the same tree reports 23 `untested` findings
across those two files, so the absence is load-bearing.

**A separate, larger bug surfaced while measuring it** and landed as its own commit
(`1ed9315`): `last_type_identifier` kept the LAST `type_identifier` in an impl header's subtree.
For `impl Index<usize> for Table` that is `usize`; for
`impl<E> Deserializer<'de> for StringDeserializer<E>` it is `E`. Every generic type's members
were filed under a **phantom owner**, and every generic machinery trait silently lost its marks.

    serde      345 -> 222   tokio  3379 -> 3045
    ripgrep    244 -> 157   alacritty 999 -> 981
    axios / Exposed / vapor / retrofit / petclinic: byte-identical
    serde findings naming a single-letter owner: 33 -> 0

### W7b — a finding may not say more than its evidence (`d951128`)
§5: duplicate-group labels printed the same `path#Owner.name` twice. The `related` entries had
carried distinct spans all along — the *data* was never ambiguous, only the rendering. The label
now names the declaring trait and falls back to the start line. The distinguisher deliberately
stays **out of the selector**, which is the finding's identity: a line number in an id churns the
baseline whenever anything above the clone moves. serde 1→0, tokio 1→0.

§5-bis: `internal-only` demoted its confidence under weak evidence (right) while printing the
certain tier's words — "only used within its own file" about a symbol whose graph holds a
cross-file match. Two evidence states now have two sentences.

### W7c — §15's mechanic verified, instance re-measured (`5b9b347`)
`publish = false` is Cargo's explicit opt-out and its absence means publishable; `manifest.rs`
reads exactly that. Re-measured: `Bytes`/`Lossy` still exist in ripgrep's
`crates/searcher/src/sink.rs`, `grep-searcher` still declares no `publish` key, and kndo now
reports **zero** `unused`/`internal-only` findings in that crate. What remains is the policy
statement, which was always the point.

### Plugin-descriptor ergonomics (`b102e50`)
kndo flagged its own repository: five built-in plugins with structurally identical
`descriptor()` bodies. It was right, and the duplication hid worse. `PluginDescriptor` carried
two fields the contract itself describes as one fact twice — `detection` prose restating
`activation` — and they had **already drifted**: every conventions plugin's prose named one
manifest kind while `ManifestDependency` matches any. `detection` is now only what nothing else
can say (an always-on ingester's report paths), and
`PluginDescriptor::on_manifest_dependency(id, dep)` is the whole shape. kndo on itself
51 → 49 findings, 96.7 → 96.8.

---

## 2. What remains — W6

**References that live outside the code.** The plugin content channel exists exactly for this:
read the non-source files (`Info.plist`, `templates/**`, `vite.config.*`) the language graph
never sees, and contribute the root or the edge. `requested_file_access` scopes them;
`RootSink`/`EdgeSink` express them (`ReferencesFile` is liveness-only by contract — a false edge
can only suppress findings, never create one).

**A plugin is for one concrete tool, never a category.** That is the rule `kndo:nextjs`,
`kndo:express` and `kndo:serde` already follow: activation binds to *that* dependency or *that*
file, and the knowledge inside is that tool's real schema. A generic "bundler plugin" would be a
mix of other people's conventions, impossible to activate precisely and impossible to maintain.

Each is independent and can land on its own. Suggested order is by whether a field case is
measurable from the corpus already cloned under `…/scratchpad/repos/`:

| Plugin / adapter | Reads | Contributes | Field case | Corpus? |
|---|---|---|---|---|
| `kndo-plugin-info-plist` | `Info.plist` | roots | `ExtensionDelegate` in Alamofire — `WKExtensionDelegateClassName`, `NSPrincipalClass`, `UISceneDelegateClassName` name classes by string; the runtime instantiates them | **yes** (Alamofire) |
| `kndo-plugin-thymeleaf` | `src/main/resources/templates/**` | edges | the two `unused` left in petclinic — `th:href`/`th:src`/`href`/`src` resolved against `static/**` | **yes** (spring-petclinic) |
| `kndo-adapter-markdown` | `*.md` | `Possible` references | the four documents left pointing at `docs/perf-baseline.json`. An **adapter**, not a plugin: "a path-shaped token is a reference" is Markdown-general, not tool knowledge | **yes** (this repo) |
| `kndo-plugin-vite` | `vite.config.*` | edges | `bin/vite.js` does `import('../dist/node/cli.js')`, which only maps back to `src/node/cli.ts` through `build.lib.entry` / `build.rollupOptions.input` | no |
| `kndo-plugin-rollup` | `rollup.config.*` | edges | `input` / `output.file` / `output.dir`. A different tool with a different schema, even though vite uses rollup underneath | no |
| `kndo-plugin-guava-testlib` | source | roots | `MapTestSuiteBuilder` discovers methods by the `test` prefix at runtime; a plugin sees the builder call and the naming rule together, the shape `kndo:express` already uses | no |

**Watch for the promotion trap.** Four of these resolve a path-ish string found in a non-source
file against the project. That looks like a shared helper — and it is exactly where rule 2 bites:
extension inference and directory-index conventions are *language* knowledge (JS resolves `./x`
to `x.ts` or `x/index.js`), so a generic "resolve a path string" helper would drag language
knowledge into the wrong layer. Write one plugin first, see what it actually needs, and only
promote when a second one demonstrably needs the same thing — to the layer that owns the
knowledge, not the nearest one.

---

## 3. Explicitly out of scope

- **§6** (the `kndo-core` module cycle) is not a gap: legal structure the tolerance policy
  decides not to report.
- **§10, §12, and the annotated halves of §11 and §13** are real limits already covered by
  `[[externally-invoked]]` in `kndo.toml`.
- **User-declared generics** (`struct Wrapper<T> { inner: T }`) would need
  `Declaration::type_params` to substitute `T` from the receiver. The W3-bis design is
  forward-compatible — substitution already exists, only the names are missing — but §3 does not
  ask for it and adding it now would be guessing.
- **`HashMap` iterating to a tuple**: an anonymous two-argument type with destructuring on the
  other side. It does not type, and that is correct — silence, not a wrong type.

---

## 4. Verification checklist for any remaining item

- `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  and **one** `cargo test --workspace --all-features` run (two in a single shell command has
  OOM-killed the harness).
- `git status --porcelain | grep expected` must be empty.
- Field verification, not just `cargo test`: re-run the binary against the clones in
  `…/scratchpad/repos/` (serde, alacritty, tokio, ripgrep, axios, Exposed, vapor, coroutines,
  retrofit, spring-petclinic, Alamofire, gin, cobra) and compare against the previous snapshot —
  the predicted false positives must disappear **and no new findings may appear**.
- Docs in the same commit, per rule 8.

---

## 5. Transport note (2026-08-26) — resolved

This session lost its git credentials mid-run (`could not read Username for
'https://github.com'`) for roughly forty-five minutes: about fifty-two push attempts, plus
re-attaching the repository with push access, all refused, while plain egress kept working
unauthenticated. It came back on its own and everything through `ef76d4d` is pushed normally,
so nothing here needs recovering.

Two leftovers from that window, neither load-bearing:

- `tmp/w5-w7-transfer` holds a `git format-patch` copy of the commits, now redundant. The git
  proxy refuses branch deletions (`send-pack: unexpected disconnect`), so it has to go by hand
  from the GitHub UI or a session whose transport allows deletes. `tmp/kondo-bundle-transfer`
  is the same kind of leftover from an earlier session.
- The rule this cost is already in §0: work that only exists in an ephemeral container is work
  at risk. Commit early, push often, and when the push is refused, say so rather than
  accumulating.
