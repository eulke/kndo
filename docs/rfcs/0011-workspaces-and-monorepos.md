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

### The `deep-import` verdict (group `risk`, M3)

A cross-package import that bypasses the provider package's declared entry points
(`@org/app` importing `@org/ui/src/private/x` instead of `@org/ui`) erodes the boundary the
provider declared: the consumer now depends on the sibling's internal file layout, and the
import breaks outright if the provider is ever published (registries enforce `exports`).
Three design rules keep the verdict signal, not noise:

1. **Contract-gated, so it is zero-config and self-opting.** The finding fires **only when the
   provider package declares an explicit surface** (an `exports` map or the language's
   equivalent, reported by the adapter in `ManifestFacts`). No declared surface = no declared
   boundary = no finding — monorepos where deep imports are accepted practice never see noise,
   and a team opts in simply by making its package surface explicit. Boundaries the compiler
   already enforces (Go `internal/`) are skipped outright, like Go package cycles.
2. **One finding per (consumer package → provider package) pair** — subject `package`, rollup
   spirit: "`@org/app` deep-imports `@org/ui` at 23 sites, touching 4 internal symbols", with
   sites and symbols in the evidence (capped, `elided` counted). That pair is the unit a
   migration is planned in; 23 line-level findings are not.
3. **Computed remediation, two mechanical cases.** kndo already has the graph, so the finding
   says which case each symbol is: (a) *also reachable via the public surface* → "switch the
   specifier to `@org/ui`" — trivially safe, the first candidate for `kndo clean` auto-fix
   post-1.0; (b) *genuinely internal* → the exact subpath export to add (`"./testing"`), or
   extraction to a shared package. Consumers (humans and agents) receive executable
   instructions, not a lecture.

Severity: warning (the gate means the provider explicitly declared the contract being
bypassed). Finding confidence = the underlying edge's confidence. The edges themselves are
recorded from M1 regardless (reachability must stay correct — deep-imported code *is* used);
the verdict lands in M3 with the package-surface machinery.

## 5. Roots & library mode are per-package decisions

Each Package independently resolves its mode from manifest signals, overridable in config:

- **Published/library** (`private` absent, publish metadata, or a lib target): its public API
  is a production root — external consumers exist by definition.
- **Private/app** (`private: true`, bins, app targets): exports are *not* roots; an export is
  alive only if a real edge (same package or sibling) consumes it. Cross-package consumption
  keeps internal-package exports honest without root-inflation — this is where monorepo dead
  code hides, and it is exactly the `internal-only`/`unused` machinery already specified, now
  fed with correct roots.

```toml
[package."@org/legacy-ui"]        # per-package config override (kndo.toml)
mode = "library"                   # force, when manifest signals lie
skip = ["duplicate"]
```

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
