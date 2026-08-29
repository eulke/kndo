# RFC 0011 — Workspaces & Monorepos

**Status:** Accepted · **Depends on:** RFC 0001, 0002, 0004, 0005 · **Changes:** graph vocabulary (contracts §1)

## 1. Problem

The repos where kndo matters most are not single-package projects: they are npm/pnpm workspaces,
Cargo workspaces, Go multi-module repos, Gradle multi-project builds — often several ecosystems
in one tree (JS frontend + Go backend). Until now the docs said "library mode per package"
without defining what a package *is*, who owns each file, or what happens at package boundaries.
This RFC defines that model.

## 2. Vocabulary: `Package` is the workspace unit; the external thing is a `Dependency`

The graph node previously named `Package` (a declared external dependency) is renamed
**`Dependency`** — aligning it with the `dependency` subject kind and the dependency verdicts
that already used that word. **`Package`** now means what developers mean by it: a
manifest-defined unit of the workspace (npm package, crate, Go module, Gradle subproject, SPM
target). Contracts §1 is updated accordingly (`DependencyId`, `ImportsDependency`,
`Resolution::Dependency`). Renaming now is free; living with "package sometimes means lodash"
forever is not.

## 3. The model

- **Project** — the analyzed root. Contains one or more Packages.
- **Package** — one manifest + the file tree it governs. Adapters already parse manifests
  (RFC 0002 §2.5); `ManifestFacts` now also carries *identity and topology*: package name,
  workspace membership declarations (`workspaces` globs, `[workspace] members`, `go.work` uses,
  `settings.gradle` includes), publish signals (`private: true`, `publishConfig`, registry
  metadata), and entry points.
- **Ownership** — every file belongs to exactly **one** Package: the nearest manifest ancestor
  (adapters may refine where a toolchain's own rule differs). A repo with no manifest at all is
  one implicit Package — the single-project case is the monorepo model with n = 1, not a
  separate mode.
- **Derived edges** — `Package depends-on Package` edges are derived from cross-package file
  edges and manifest declarations; they power package-level queries (`kndo uses pkg:ui`),
  package rollups (§6) and package-level cycles (`cyclic` subject `package`).

## 4. Resolution & boundary enforcement

An import specifier may now resolve to a third target: an **internal package** (`workspace:*`
deps, path dependencies, tsconfig path aliases into a sibling, Go replace directives, Cargo
path deps). Resolution yields the concrete internal file — real edges, full reachability across
packages — *and* kndo validates the boundary contract both ways:

| Manifest vs reality | Finding |
|--------------------|---------|
| internal dep declared, no import resolves into that package | `unused` (subject `dependency`) — same verdict, remediation says "remove the workspace dep" |
| import resolves into a sibling package not declared in the importer's manifest | `undeclared` (subject `dependency`) — phantom internal dependency; breaks publishability and build graphs |

This table — and `Resolution::WorkspaceMember`'s edge derivation generally (both `ImportsFile`
*and* `ImportsDependency`, contracts §2) — assumes the ecosystem has a per-sibling declaration
contract to validate in the first place (npm's `workspaces`/`dependencies` entries, Cargo's
`[dependencies]` path entries). Not every language does: a Go module's own subpackages need no
`require` entry (a module cannot require itself), so an adapter resolving its own module's
internal imports uses plain `Resolution::File` instead — full `ImportsFile` reachability, no
`ImportsDependency` edge, and correctly no `undeclared` finding for a contract that doesn't exist
(docs/adapters/go.md §3). `WorkspaceMember` stays reserved for resolutions where a real
declaration contract exists to validate — a `go.work` sibling *module* (once supported) would
still be `WorkspaceMember`, since that's a genuinely separate module Go's own tooling tracks by
name via `go.work`'s `use` directives, just not via `require`.

### The `deep-import` verdict (group `risk`, M3) — internal *and* external providers

An import that bypasses a provider package's declared entry points erodes a boundary someone
explicitly drew. **The provider does not have to be a workspace sibling** — the verdict covers
both cases with one definition, because the mechanism is identical:

- **Internal provider** (workspace sibling): `@org/app` importing `@org/ui/src/private/x`
  instead of `@org/ui` — the consumer now depends on the sibling's internal file layout, and
  the import breaks outright if the provider is ever published.
- **External provider** (a dependency): `import { helper } from "some-lib/dist/internal/utils"`
  — common in perfectly ordinary single-package apps. It often *works* only because a bundler
  is lax where Node's `exports` enforcement would refuse, and it breaks silently on the next
  library upgrade. The dependency's own manifest (already read for resolution) supplies the
  declared surface.

Three design rules keep the verdict signal, not noise:

1. **Contract-gated, so it is zero-config and self-opting.** The finding fires **only when the
   provider declares an explicit surface** (an `exports` map or the language's equivalent,
   reported by the adapter — from the sibling's manifest or the dependency's own). No declared
   surface = no declared boundary = no finding: monorepos where sibling deep imports are
   accepted practice never see noise, and `lodash/fp` is not a finding (lodash declares no
   `exports` map — its subpaths are deliberately open). Boundaries whose enforcement is
   *unconditional at build time* (Go `internal/`) are skipped outright; **partially** enforced
   boundaries (Node `exports`, which bundlers and legacy TS resolution routinely bypass) are
   exactly where the finding earns its keep — "works in webpack today, breaks in Node/jest
   tomorrow".
2. **One finding per (consumer package → provider) pair** — subject `package`, rollup spirit:
   "`@org/app` deep-imports `@org/ui` at 23 sites, touching 4 internal symbols", with sites and
   symbols in the evidence (capped, `elided` counted). That pair is the unit a migration is
   planned in; 23 line-level findings are not.
3. **Computed remediation, by case.** kndo has the graph, so the finding says which case each
   symbol is. Provider-internal *and* also reachable via the public surface → "switch the
   specifier to `@org/ui`" — trivially safe, the first candidate for `kndo clean` auto-fix
   post-1.0 (applies to external providers too when the symbol is re-exported publicly).
   Genuinely internal, internal provider → the exact subpath export to add (`"./testing"`), or
   extraction to a shared package. Genuinely internal, **external** provider → you don't own
   the surface: use the public equivalent, request the export upstream, or vendor the code —
   stated in that order.

Severity: warning (the gate means the provider explicitly declared the contract being
bypassed). Finding confidence = the underlying edge's confidence. The edges themselves are
recorded from M1 regardless (reachability must stay correct — deep-imported code *is* used);
the verdict lands in M3. The external-provider case is also listed with the dependency-hygiene
findings (RFC 0005 §5), since that is where a single-package app will meet it.

**Landed (M3), with one recorded boundary:** the internal-provider case is implemented
end-to-end — `ManifestFacts::{declares_surface, resolved_entries}` flow onto
`PackageNode::{declares_surface, surface}` at assembly, and `analysis/deep_import.rs`
implements all three rules (gate, pair rollup with capped site evidence, computed remediation
— the also-public-vs-genuinely-internal split computed per touched symbol via file-granular
surface-reachability, since a re-export chain from the entry is exactly an `ImportsFile`
path). The **external-provider case cannot fire yet**, by the gate's own logic rather than a
special case: evaluating it requires the provider's *own* manifest, which lives outside the
discovered tree (`node_modules/` is not walked — RFC 0008's discovery bounds), so
`declares_surface` is unknowable and the gate stays closed — silence, the safe direction.
Making it fire needs a provider-manifest peek at resolution time (its own
discovery/cache/purity design pass); tracked in ROADMAP as the remaining half of this
verdict, not silently absorbed.

## 5. Roots & library mode are per-package decisions

Each Package independently resolves its mode from manifest signals:

- **Published/library** (`private` absent, publish metadata, or a lib target): its public API
  is a production root — external consumers exist by definition. "Public API" is computed
  precisely (M6): manifest-declared entry files' surface-transitive exports, extended through
  whole-surface re-exports (`pub mod`/`export *` — assembly's library-surface fixpoint) and
  named re-exports (barrel indirection), and completed by the **surface-member closure**: a
  surface type's members whose rung is `surface_transitive` (RFC 0012 §6) are surface too — a
  `pub` method of a re-exported struct is consumer-callable API even with zero in-package
  references. Capped rungs (`pub(crate)`, Swift `internal`, Go `internal/` exports) never
  join the surface, keeping `unused`/`internal-only` at full precision for them.
- **Private/app** (`private: true`, bins, app targets): exports are *not* roots; an export is
  alive only if a real edge (same package or sibling) consumes it. Cross-package consumption
  keeps internal-package exports honest without root-inflation — this is where monorepo dead
  code hides, and it is exactly the `internal-only`/`unused` machinery already specified, now
  fed with correct roots.

**Not implemented.** A per-package config override — forcing a mode when manifest signals lie,
or skipping a category for one package — is conceivable but does not exist in any form today:
`config::LIVE_TABLES` carries no `package` table, `kndo init`'s template has no such section,
and there is no `PackageMode` type. Manifest signals are the only input; there is no override.

## 6. Verdicts at package granularity

The rollup ladder (RFC 0005 taxonomy rule 3) gains its natural top rung:
**symbol → file → directory → package.** A Package none of whose files are reachable rolls up
to one `unused` finding with subject `package` ("this whole workspace member is dead");
a Package consumed only by tests rolls up to `test-only:package`. `subject_kind` gains
`package`; findings gain an optional `package` field (owning package name) so CI and agents can
partition results without path arithmetic.

`kndo health` reports the global score plus a per-package breakdown (`--by-package`); weights
and formula are unchanged — the package axis is a *grouping* of the same penalties, not a new
metric.

## 7. Mixed ecosystems

Nothing special, by construction: Packages of different languages coexist as siblings; ownership
is per-manifest; dependency verdicts are per-manifest already; cross-language edges (RFC 0002
§4) compose with cross-package resolution unchanged. The Go backend importing nothing from the
JS frontend simply produces no edges between those packages.

## 8. Scoping runs & queries

- `kndo check [PATHS…]` already scopes by path; paths align with package boundaries naturally.
- Selectors: `pkg:<name>` addresses a Package; the external-dependency selector becomes
  `dep:<name>` (RFC 0007 updated). `describe pkg:@org/ui`, `used-by pkg:@org/ui`,
  `trace pkg:app pkg:legacy` work like any node.
- Diff modes need no changes: derived effects already cross package boundaries because edges do.

## 9. Cache & invalidation

Package topology is part of the graph key already (manifest content hashes, RFC 0004 §3) — 
adding/removing/renaming a workspace member invalidates precisely through its manifest hash.
Per-package facts need no partitioning: the facts cache is content-addressed and
package-agnostic.

## 10. Non-goals (1.0)

Task-runner integration (Nx/Turbo/Bazel graphs are *build* graphs; kndo derives its own from
code), affected-package CI splitting (consumers can compute it from `Package depends-on` edges
in the JSON), tsconfig project-references deep integration (parking lot), versioning/release
concerns (changesets et al. own that).
