# RFC 0015 — Plugin Identity, Dependencies & Installation

**Status:** Accepted, phased (§6) · **Depends on:** RFC 0003 (plugin system, activation — §4),
ADR 0003 (WASM linking), RFC 0014 (distribution posture: git-first, no central infrastructure) ·
**Ships:** M6

## 1. The problem, from a real scenario

A company builds an internal framework that wraps Next.js, which wraps React, which the JS
ecosystem's own conventions already partially cover. The company writes a kndo plugin for its
framework in a private repo. Three things must work:

1. A project that only declares `@company/framework` in its manifest must get the *whole
   chain's* conventions — the framework plugin's, Next's, React's — without declaring any of
   them, because the project's author doesn't even know the chain (that's the wrapper's job).
2. The company plugin must be able to reference kndo's own built-in plugins and third-party
   plugins from other sources, unambiguously.
3. "Reference by name" must not be a land grab: with flat names, anyone can publish a `.wasm`
   whose descriptor says `id: "react"` — and then *which* react activates is undefined.

RFC 0003 §4's `activation` rules answer "does *this* plugin apply to *this* project?" but say
nothing about plugins composing. This RFC adds exactly that, and nothing else. The practical
author-facing companion (toolchain, project setup, testing shape, maintenance checklist) is
[docs/plugins/authoring.md](../plugins/authoring.md).

## 2. Identity: the coordinate IS the id (the Go-modules move)

A plugin's id is not a name to be looked up — it is the coordinate it can be fetched from:

- **External plugins**: a source coordinate, `github.com/<owner>/<repo>` (host part extensible
  later; GitHub is v1). Optionally version-qualified where a version is expressible
  (`@v1.2.0`). Because the identity is the location, there is nothing to squat, no registry
  authority to assign names, and no ambiguity: two "react conventions" plugins are
  `github.com/a/react-conventions` and `github.com/b/react-conventions` — different ids, both
  installable, no conflict.
- **Built-ins**: the reserved `kndo:` namespace — `kndo:coverage-lcov`, `kndo:nextjs`,
  `kndo:express`. The loader **rejects** any external component whose descriptor claims a
  `kndo:`-prefixed id: the namespace is not claimable, so referencing a built-in from any
  external plugin is always unambiguous.

**Identity binding**: whenever a component is fetched *by* a coordinate (§4), the descriptor it
reports must declare exactly that coordinate as its `id`, or it is rejected. Nothing can
impersonate an id it wasn't fetched from. (A hand-dropped `.wasm` in `.kndo/plugins/` skips
this check — its presence in the project is already the trust decision, same as today.)

Existing ids migrate: `coverage-lcov` → `kndo:coverage-lcov`. The demo/example plugins keep
plain ids (`hooks-demo`) — legal for hand-dropped files, but such an id can never be the target
of a dependency (§3), which is the point: depending on something requires it to be fetchable.

## 3. `dependencies`: one field, two coupled effects

```text
PluginDescriptor {
    id:           "github.com/company/framework-plugin",
    activation:   [ManifestDependency("@company/framework")],
    dependencies: ["github.com/company/other-framework-plugin", "kndo:nextjs"],
}
```

A dependency is *a plugin whose conventions are part of this plugin's own* — the wrapper
relationship. Declaring one has exactly two effects:

1. **Install-time closure** (§4): installing the plugin installs its dependencies,
   transitively. `kndo:*` entries resolve as no-ops (compiled in).
2. **Activation implication**: when a plugin is active, every dependency that is *present*
   (installed globally, dropped project-locally, or built-in) becomes active too — computed as
   a fixpoint over the present set, so chains compose to any depth:
   `company-framework → other-framework → kndo:express` all activate when the project matches
   only the company plugin's own rule. Cycles are harmless (set semantics — no ordering is
   implied, because plugins still never consume each other's output; execution order remains
   the existing sorted-by-id interim rule).

Deliberately **one field, not two** ("requires" vs "implies"): since plugins cannot read each
other's contributions — structurally, `GraphView` exposes only adapter-built facts and sinks go
to the core — the *only* coherent meaning of inter-plugin dependency is "co-activate and
co-install". Splitting it would invent a distinction with no behavioral difference to hang it
on. For the same reason there are **no version constraints between plugins**: there is no ABI
between them to be compatible about. Version selection exists only at install time (§4), per
coordinate, not per edge.

**A missing dependency is never a runtime error.** If `kndo:nextjs` names a built-in, it's
always present. If `github.com/x/y` isn't installed (hand-managed setups), the plugin still
runs; `kndo doctor` reports the exact missing coordinate and the install command that fixes it.
Degradation is legible — "those conventions aren't being analyzed" — never a crash, matching
every other absence in the product.

**Over-activation is accepted within a declared closure**: a frontend-only project using the
company framework will also activate the express-conventions dependency, which will find no
express-shaped symbols and contribute nothing. That is the author-curated cost of the wrapper
declaring its user-facing surface — bounded by the closure, unlike lockfile inference (§5),
which is unbounded.

## 4. `kndo plugin install` — registry semantics without a registry service

```text
kndo plugin install github.com/company/framework-plugin@v2
kndo plugin list
kndo plugin remove github.com/company/framework-plugin
```

Git-first, mirroring RFC 0014's release posture (the sibling Yunta project's pack design
reached the same conclusion independently: "git-first, lockfile always"):

1. Resolve the coordinate to a GitHub release of that repo (`@vX.Y.Z` names the tag; bare
   coordinate = latest release). The release must carry a `.wasm` asset and a checksum file —
   the same artifact convention RFC 0014 §3 uses for kndo itself.
2. Download; verify the checksum; verify identity binding (§2): descriptor id == coordinate.
3. Read `dependencies`; recurse. `kndo:*` → no-op. Already-installed coordinate at a
   compatible version → no-op.
4. Write the `.wasm` files into the existing global plugin directory (RFC 0003 §4 /
   wasm-abi.md §5.5) plus a lockfile beside them (`plugins.lock`: coordinate → version →
   sha256) making the installed set reproducible and auditable.

**Version conflicts, minimal v1 policy**: one installed copy per coordinate. If an install
would require two incompatible versions of the same coordinate (different explicit tags), the
install **fails with both requirers named** — kndo does not guess. No dependency solving: there
is no inter-plugin ABI that would justify it.

**Private repos work with zero extra machinery** — the fetch uses the user's existing git/
GitHub credentials, which is precisely the company-framework scenario. A central registry
would have required private hosting; coordinates make privacy the repo's own access control.

**Landed** (`kndo::plugin_install`, the `kndo` crate's `plugin-install` feature, on by
default). Implementation decisions worth pinning:

- Release shape enforced literally: exactly one `.wasm` asset (ambiguity is an error naming
  every candidate) plus `checksums.txt` in `sha256sum` line format. Assets download through
  the API asset URL with `Accept: application/octet-stream` — the one form that carries
  auth for private repos; credentials are `GITHUB_TOKEN`/`GH_TOKEN` from the environment.
- The whole transaction stages first and commits last: any checksum, identity, conflict, or
  fetch failure anywhere in the closure leaves the directory and lockfile untouched.
- `plugins.lock` maps coordinate → `{version, sha256, file}`; the on-disk name is the
  coordinate with `/` → `__` (`github.com__owner__repo.wasm`), reversible because `__` cannot
  appear in a GitHub owner/repo name. Files present but not in the lock are reported by
  `kndo plugin list` as hand-installed, never hidden and never touched by `remove`.
- The version-conflict rule has a cross-transaction twin: an explicit tag that disagrees with
  the locked version fails, naming the installed version and the requirer — `remove` first if
  the change is intended. Bare (untagged) requests are compatible with anything installed.
- A `kndo:*` dependency that this build does *not* compile in is a warning in the install
  report (and a doctor line thereafter), not an error — §3's never-fatal rule applied at
  install time too.
- Network and component-probing are injected edges (`ReleaseSource` + a probe fn), so every
  policy above is proven by unit tests without either, plus one integration test driving the
  real WASM probe: a genuine component with a plain id fetched by coordinate trips identity
  binding and installs nothing (`crates/kndo/tests/plugin_install_probe.rs`).

## 5. Rejected: lockfile-transitive activation

Considered and rejected as the mechanism for the wrapper case (matching `ManifestDependency`
against the project's lockfile closure instead of its declared dependencies):

- Public frameworks already don't need it: Next declares React as a **peer dependency**, so
  every real Next project declares `react` directly — direct matching fires today.
- A lockfile cannot distinguish "framework re-exports React to its users" from "some CLI tool
  uses React internally" — activating on transitive presence is guessing, and this project's
  standard is silence over a guess (RFC 0005 §13).
- It is unbounded (thousands of transitive packages), where a declared `dependencies` closure
  is bounded and author-curated.
- Per-ecosystem lockfile parsers (three formats in JS alone) are real permanent surface.

If a concrete case ever appears that `dependencies` cannot express, this gets rediscussed with
that case on the table — not before.

## 6. Phases

1. **`Plugin::mutates_graph()`** — landed with this RFC's first commit: prerequisite hygiene
   (a coverage-only plugin must not cost the cache; a built-in convention plugin must not cost
   every non-matching project a full rebuild — see RFC 0003 §5's updated note and the
   regression tests in `graph.rs`).
2. **Identity + `dependencies` + fixpoint activation**: descriptor field (native + WIT),
   `kndo:` namespace reservation enforced at load, activation fixpoint in the composition
   layer, `kndo doctor` showing dependency chains and missing coordinates. Semantics complete
   and fully testable without any network code.
3. **`kndo plugin install/list/remove`**: the fetch/verify/lockfile machinery of §4 —
   landed, see §4's implementation notes.
4. **First real built-ins**: `kndo:nextjs` (file-system routing roots, special exports — the
   flagship, spec: [docs/plugins/nextjs.md](../plugins/nextjs.md)) and `kndo:express`
   (script-launched entry files the import graph can't see — honest spec:
   [docs/plugins/express.md](../plugins/express.md); express is imperative, so its convention
   surface is real but modest, and `views/**` templates turned out to be *unclaimed* files —
   invisible to the graph, hence producing no findings to suppress — so they're documented out
   of scope rather than covered). Both gated by their own `activation` rules — a built-in
   convention plugin must never run (or cost cache bypass) on a project that doesn't match.

## 7. Explicitly out of scope

- A central registry service (hosting, accounts, moderation) — coordinates make it
  unnecessary; revisit only if discoverability demands a *directory* (which can be a static
  page, not a service).
- Version constraints or ordering constraints between plugins — no inter-plugin ABI exists to
  justify either. RFC 0003 §5's ordering-constraints gap stays open, unchanged by this RFC.
- Plugins consuming other plugins' contributions — still structurally impossible, still
  deliberate.
- Signing beyond checksums (sigstore-style provenance) — worth a look post-1.0; checksums +
  identity binding + the WASM sandbox are the v1 trust story.
