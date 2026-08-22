# `kndo:express` — Express conventions plugin

**Status:** Normative for the built-in `kndo:express` plugin (RFC 0015 §6 phase 4) ·
**Implements:** `contribute_roots` + `annotate_symbols` (RFC 0003 §2) · **Crate:**
`crates/kndo-plugin-express` · **Convention set versioned against:** Express 4/5 +
express-generator layout

## 1. An honest scope statement

Express is imperative, not convention-driven: routes are registered by calling
`app.get(...)` in ordinary code that plain import analysis already follows. So this
plugin's surface is deliberately small — it covers the one place the framework's
*ecosystem* conventions make real code invisible to import analysis: **the entry file is
launched by a script, not imported**.

`npm start` in an express-generator project runs `node ./bin/www`; hand-rolled projects run
`node server.js` or `node app.js` directly. Either way, no in-repo file imports the entry
module, so to reachability it is an orphan and everything only it reaches rolls up as
unused.

Two structural facts bound what the plugin can do about it:

- **`bin/www` is extension-less**, so no adapter claims it — it isn't a graph node, and a
  root targeting it resolves to nothing (dropped silently). Its `require('../app')` edge
  doesn't exist in the graph either. The fix that *is* expressible: root the conventional
  claimed entry files it (or `node` directly) launches.
- **`views/**` templates** (`.pug`, `.ejs`, `.hbs`) are unclaimed for the same reason, and
  the `res.render('index')` string → template edge would need file content, which
  `GraphView` doesn't expose. Templates stay out of scope — they're invisible to the graph
  altogether, so they produce no findings to suppress in the first place.

## 2. Activation

```text
activation:   [ManifestDependency("express")]
dependencies: []
```

Same single-rule reasoning as `kndo:nextjs` §2: every project running Express declares it
(it's a runtime dependency, never an implicit peer), the manifest scan is gitignore-aware
and monorepo-wide, and wrapper frameworks reach this plugin through RFC 0015 §3
`dependencies` implication — this is the plugin RFC 0015 §1's company-framework chain
terminates at.

## 3. What gets rooted

App roots are derived exactly as in nextjs.md §3, minus the `next.config.*` anchor: any
directory directly containing a `package.json`. For each app root `R`, the **conventional
entry candidates** are:

```text
R/app.<ext>   R/server.<ext>   R/src/app.<ext>   R/src/server.<ext>
```

for each extension the JS/TS adapter claims. Every candidate that exists as a claimed
`js-ts` file with `Production` role gets:

- **File root**: `RootKind::Production` at `Probable` — a convention, not a certainty: a
  file named `app.ts` in an Express-using project is *probably* its entry, unlike a file
  under `pages/` which is *definitionally* routed. `Probable` still suppresses `unused`
  findings at every tier (RFC 0005 §1 rule 4) while keeping the evidence honestly labeled.
- **Exported top-level symbols** at `Probable`, each also marked **externally consumed**:
  the generator layout's `app.js` exports the app object solely for the unclaimed `bin/www`
  to require — an external consumer the graph cannot see. Member symbols (`member_of` set)
  are never touched.

`index.<ext>` is deliberately **not** a candidate: root-level `index` files are the
package-main convention of the whole JS ecosystem, not an Express signal — rooting them
would blanket-exempt library surfaces in every monorepo that happens to use Express
somewhere. Projects with genuinely custom entry names fall back to plain reachability —
degrade toward silence, matching the zero-false-positive standard's direction (the entry
file may be *reported* unused only when nothing roots or imports it; a project that hits
this can root it via `bin/`-script conventions landing in a future revision, or restructure
to a conventional name).

## 4. What this plugin does *not* do

- **No route edges** (`app.use('/users', usersRouter)`): the router module is imported by
  ordinary code — the language adapter already sees it. String-path → handler edges add
  nothing to liveness.
- **No `contribute_edges`, no `classify_file`** — nothing to correct.
- **No `package.json` `"main"`/`"scripts"` parsing**: `GraphView` exposes no file content.
  Deriving the true entry from `"scripts": {"start": ...}` is the *right* long-term fix and
  would replace §3's name heuristic; it needs a host-mediated content channel (RFC 0003
  §2's `requested_file_access` exists for `ingest_coverage` but is not plumbed to the graph
  hooks). Tracked as the known gap of this spec, not silently ignored.

## 5. Verification shape

Pure classifiers (entry-candidate matching per app root) unit-tested in the crate;
end-to-end baseline-then-plugin fixture through `kndo::open` (authoring.md §8): an
`app.js` that only wires routers, imported by nothing, is `unused` without the plugin
(no `express` in the manifest) and silent with it.
