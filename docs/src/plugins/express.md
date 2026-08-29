# `kndo:express` — Express conventions plugin

**Status:** Normative for the built-in `kndo:express` plugin ·
**Implements:** `contribute_roots` + `annotate_symbols`, both content-channel-aware ·
**Crate:** `crates/kndo-plugin-express` · **Convention set versioned
against:** Express 4/5 + express-generator layout

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
- **`views/**` templates** (`.pug`, `.ejs`, `.hbs`) are unclaimed for the same reason. The
  `res.render('index')` string → template edge would need parsing the *route source* (`app.js`
  et al.) for the call's string argument — those files are already claimed by the JS/TS
  adapter, so reading them through the content channel to extract a fact the
  adapter itself owns is out of contract — the same boundary that rules out the equivalent
  `<Link href>` edges in `kndo:nextjs`. Moot either way: templates stay unclaimed regardless,
  so they produce no findings to suppress in the first place — the edge would have nothing to
  connect *to*.

## 2. Activation

```text
activation:   [ManifestDependency("express")]
dependencies: []
```

Same single-rule reasoning as `kndo:nextjs`: every project running Express declares it
(it's a runtime dependency, never an implicit peer), the manifest scan is gitignore-aware
and monorepo-wide, and wrapper frameworks reach this plugin through dependency-chain
activation: when an active plugin lists `kndo:express` among its own `dependencies`, express
activates too even though the project's manifest never mentions it directly. That is the
only path there is for the wrapper case — a company framework that uses Express internally
never puts `express` in its own users' manifests, so this plugin's manifest-declared
activation rule could never fire in such a project on its own.

## 3. What gets rooted

App roots are derived exactly as for `kndo:nextjs`, minus the `next.config.*` anchor: any
directory directly containing a `package.json`. For each app root `R`, the **conventional
entry candidates** are:

```text
R/app.<ext>   R/server.<ext>   R/src/app.<ext>   R/src/server.<ext>
```

for each extension the JS/TS adapter claims — **plus** whatever `R/package.json`'s own
`"main"`/`"scripts"` resolve to when read through the content channel (covered in the next
section). Every candidate that exists as a claimed `js-ts` file with `Production` role gets:

- **File root**: `RootKind::Production` at `Probable` — a convention, not a certainty: a
  file named `app.ts` in an Express-using project is *probably* its entry, unlike a file
  under `pages/` which is *definitionally* routed. `Probable` still suppresses `unused`
  findings at every tier while keeping the evidence honestly labeled.
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
  nothing to liveness. (The *mechanism* for such edges now exists: the JS/TS adapter records
  every call whose argument is a string literal as a generic fact — `("app.use", "/users",
  span)`, `("app.get", "/users", span)`, and so on — which the graph persists and a plugin
  could turn into a file-target edge. The reason not to emit them here is unchanged: they
  would provably alter zero findings.)
- **No `contribute_edges`, no `classify_file`** — nothing to correct.
- **`package.json` `"main"`/`"scripts"` parsing — landed.** Every app root's own
  `package.json`, read through the host-mediated content channel (`requested_file_access:
  ["**/package.json"]`), is parsed for `"main"` and any `node`/`nodemon` invocation inside
  `"scripts"` values; each resolved path joins the conventional entry-candidate set above at
  the same `Probable` confidence. A manifest that fails to parse, or a root with none, simply
  falls back to the name heuristic alone — this was never a guess-or-nothing upgrade.

## 5. Verification shape

App roots are derived from the graph's own package topology (`GraphView::packages()`) —
every directory holding a `package.json` is a `PackageNode`, so the plugin no
longer path-scans the whole file list for manifest basenames.

Pure classifiers (entry-candidate matching per app root) unit-tested in the crate;
end-to-end baseline-then-plugin fixture through `kndo::open`: an
`app.js` that only wires routers, imported by nothing, is `unused` without the plugin
(no `express` in the manifest) and silent with it.
