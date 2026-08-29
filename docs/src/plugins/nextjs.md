# `kndo:nextjs` — Next.js conventions plugin

**Status:** Normative for the built-in `kndo:nextjs` plugin ·
**Implements:** `contribute_roots` + `annotate_symbols`, both content-channel-
aware · **Crate:** `crates/kndo-plugin-nextjs` · **Convention set versioned
against:** Next.js 13–15 (pages router + app router)

## 1. What Next.js breaks about import analysis

Next.js is a *file-system router*: the framework loads `pages/**` and `app/**` files by
path convention, and calls well-known exported symbols (`default`, `getServerSideProps`,
`generateMetadata`, …) by name. None of that appears as an import in the project's own
code, so to plain reachability every page is an orphan:

- **The file** — nothing imports `pages/index.tsx`; without a root it rolls up into an
  `unused` file finding.
- **Its symbols** — rooting the file is *not* enough. Reachability (`analysis/reachability.rs`)
  has a symbol→owning-file edge (module-load rule) but
  deliberately **no file→contained-symbols edge**: a reachable file does not make its
  declarations reachable. The page component and its data hooks would still be flagged as
  unused symbols. So the plugin must root the file *and* the framework-consumed exports.

Both contributions carry `Provenance::Plugin("kndo:nextjs")` and flow through the same
named-target resolution every adapter fact uses — an export the JS adapter didn't extract
resolves to nothing and is dropped silently, never invented.

## 2. Activation

```text
activation:   [ManifestDependency("next")]
dependencies: []
```

One rule, deliberately. Every real Next.js project declares `next` in some `package.json`
(it provides the `next` CLI the project runs), and `ManifestDependency` already scans every
manifest under the root through the gitignore-aware walker — monorepos included, `node_modules`
excluded. A `FileExists("**/next.config.*")` rule was considered and dropped:
`FileExists` is a raw filesystem glob, so a recursive pattern would walk `node_modules` on
every run of every project — real cost and a real false-activation hazard for a signal the
manifest rule already carries. Wrapper frameworks that hide the `next` dependency reach this
plugin through `dependencies` implication, not through a weaker activation rule added here:
when an active plugin lists `kndo:nextjs` among its own `dependencies`, this plugin activates
as part of that fixpoint even though its own `ManifestDependency("next")` rule never fired
directly.

The plugin declares `mutates_graph() = true` (it exists to contribute graph facts), which is
exactly why gating matters: on a project that doesn't match, it is never registered and never
costs the graph-snapshot/patch bypass.

## 3. App roots: anchoring conventions in a monorepo

`pages/` and `app/` are conventions *relative to a Next.js application root*, not to the
kndo project root. Matching any path segment named `pages` anywhere would swallow the very
common `src/components/pages/` layout — over-rooting real component code. Instead the plugin
derives **app roots** from `GraphView::files()` (which lists every discovered file, claimed
or not):

> An app root is any directory that directly contains a `package.json` or a `next.config.*`
> file. The set is computed once per run from file paths alone — no file content is read.
> (`pageExtensions` narrowing, covered further below, is a separate, later step over an
> already-derived root — it reads content, root derivation itself never does.)

For each app root `R` (where `R` may be the project root itself), the convention
directories are exactly:

| Tier | Directories |
|------|-------------|
| pages router | `R/pages/`, `R/src/pages/` |
| app router | `R/app/`, `R/src/app/` |
| support files | directly in `R/` or `R/src/` |

This is over-inclusive in one bounded way: a non-Next package inside a Next-using monorepo
that happens to have a top-level `pages/` directory is treated as if it were a Next app.
That errs toward keeping code alive (false-negative direction) and only within packages of
a project that genuinely depends on `next` — accepted under the zero-false-positive
standard: silence over a guess.

## 4. What gets rooted

Only files the JS/TS adapter claimed (`language == "js-ts"`) with **`Production` role**
participate. Test files under `pages/` (`pages/index.test.tsx`) are a routing mistake in a
real app, and rooting them as production would let production-reachability mask `test-only`
findings — the role machinery already handles them. Unclaimed files (`.mdx` pages, images)
resolve to nothing and are skipped by construction.

### 4.1 pages router — every claimed file under a pages directory

Next routes *every* eligible file under `pages/` (including `pages/api/**` and the
`_app`/`_document`/`_error` specials — same rule, no special-casing needed):

- **File root**: `RootKind::Production` at `Certain` — being routed is definitional, not
  heuristic, once the directory is a pages dir.
- **Framework-consumed exports** at `Certain`: `getServerSideProps`, `getStaticProps`,
  `getStaticPaths`, `config`, `reportWebVitals`.
- **Every other exported top-level symbol** at `Probable`: the page component itself is a
  *default* export whose local name is arbitrary (`export default HomePage`), and
  `SymbolNode` carries no is-default-export flag — the only way to guarantee the component
  is covered is to root all exports. Documented over-approximation: a genuinely dead named
  export in a page file will not be reported (false-negative direction only).

### 4.2 app router — special basenames under an app directory

Only files whose basename-minus-extension is one of Next's reserved names (routing segments
themselves are directories, arbitrary files under `app/` are ordinary modules that must
still earn reachability through imports):

```text
page layout template loading error global-error not-found default route
icon apple-icon opengraph-image twitter-image sitemap robots manifest
```

- **File root**: `Production` at `Certain`.
- **Framework-consumed exports** at `Certain`: `generateMetadata`, `generateStaticParams`,
  `generateImageMetadata`, `generateSitemaps`, `generateViewport`, `metadata`, `viewport`,
  `revalidate`, `dynamic`, `dynamicParams`, `fetchCache`, `runtime`, `preferredRegion`,
  `maxDuration`, and the route-handler methods `GET`, `POST`, `PUT`, `PATCH`, `DELETE`,
  `HEAD`, `OPTIONS`.
- **Other exports** at `Probable` (same default-export reasoning as the pages-router case
  above).

### 4.3 Support files — directly at an app root

`middleware.*` and `instrumentation.*` directly in `R/` or `R/src/`: file root at
`Certain`; consumed exports at `Certain` — `middleware`, `config` for the former,
`register`, `onRequestError` for the latter; other exports at `Probable`.

`next.config.*` itself is an **anchor only**: its `.config.` name marker already classifies
it `Tooling`, and tooling files have their own role-derived treatment — the
plugin contributes nothing for it.

### 4.4 Annotations

Every export rooted above — pages router, app router, or support files — is also marked
**externally consumed**
(`annotate_symbols`) — the framework is the external consumer — feeding the
`internal-only`/`private-type-leak` exemption so a page's exported props type
is never told to narrow its visibility.

Member symbols (`member_of` set — class methods) are never rooted or annotated: Next's
conventions are module-level.

## 5. What this plugin does *not* do

- **No `contribute_edges`**: route-string → page edges (`<Link href="/about">`) would need
  reading `.tsx`/`.jsx` *source* — files the language graph already claims and parses. The
  content channel is deliberately scoped to files *outside* the graph (configs,
  manifests, templates); second-guessing the JS/TS adapter's own claimed files through the
  side door stays out of contract even now that content access exists. Dead-page detection
  therefore stays conservative: a page no `<Link>` points to is still rooted (it is
  externally routable by URL — that's not deadness).
- **No `classify_file`**: the JS adapter's role/origin classification is already right for
  Next projects.
- **`pageExtensions` awareness — landed, statically only.** Every app root's
  own `next.config.*` (`requested_file_access: ["**/next.config.*"]`) is scanned for a
  literal `pageExtensions: [...]` array of quoted strings; when found, it *narrows* which
  extensions qualify a file under `pages/`/`app/` as routed (real Next.js behavior: a plain
  `.tsx` under `pages/` stops being a page once `pageExtensions` is customized to e.g.
  `["page.tsx"]`). JS evaluation stays explicitly out of scope — a dynamic value
  (`pageExtensions: DEFAULT_EXTENSIONS`, a spread, anything not a literal array of strings)
  is left unrecognized, and that root keeps this plugin's original unfiltered behavior
  rather than guessing.
- **No React-generic conventions** (a future `kndo:react` concern, not folded in here).

## 6. Verification shape

Pure path classifiers (`app roots`, per-tier matching, special-name sets) are unit-tested
in the crate. End-to-end, the fixture pattern is baseline-then-plugin
(authoring.md, "the baseline-then-plugin standard") through `kndo::open`: a fixture Next project whose page
nothing imports yields `unused` findings for the page file/exports with the plugin
inactive (no `next` in the manifest) and none with it active — proving both the activation
gate and the contributed facts in one pair of runs.
