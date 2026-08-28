# `kndo:libsass-maven-plugin`

**Status:** Shipped, built-in. Activation: any `**/*.scss` exists under the project root ·
**Implements:** `classify_file`, `contribute_edges` ·
**Crate:** `crates/kndo-plugin-libsass`

## The gap

A project can compile its stylesheets into its own source tree and commit the result.
spring-petclinic does: `com.gitlab.haynes:libsass-maven-plugin` reads `src/main/scss/` and
writes `src/main/resources/static/resources/css/petclinic.css`, and **both sides are in git**.

kndo sees two unrelated things, and is wrong about each:

- The CSS carries no `@generated` marker — libsass emits none — and sits under a perfectly
  ordinary path, so it reads as authored. The moment anything makes it reachable, every custom
  property the compiled Bootstrap bundle declares and never reads is reported.
- `src/main/scss/` is referenced by nothing at all, and reads as a dead directory.

The pom knows both facts. This plugin reads exactly two configuration values and nothing else.

## Why it gates on `*.scss` and not on the pom

A Maven **build** plugin lives in `<build><plugins>`, never in `<dependencies>`, so no
`ManifestDependency` rule can name it — and recording a build plugin as a dependency to make
one work would corrupt `undeclared` and `version-skew`, which are about what the project
*links against*. So it gates on a file, like `kndo:info-plist`, and pays the glob. `*.scss`
rather than `**/pom.xml` because it is the narrower of the two signals: with no Sass anywhere,
a Sass build plugin has nothing to say.

**Activation is therefore not the same question as contribution.** The plugin is active on any
project containing Sass; it contributes only where a pom declares the compilation. Its proof
asserts both halves separately for that reason.

## The rules

1. **Anything under a declared `outputPath` is `FileOrigin::Generated`.** Narrow on purpose:
   only the origin axis moves, and `role` is left exactly as the language decided.
2. **One edge per (output, input) pair, output → input.** `ReferencesFile` reads "if `from` is
   alive, that file is in use", so the direction says: *the stylesheet ships, therefore the
   Sass it was compiled from is in use.* The reverse would state the opposite of what is true
   — the `.scss` is the side nothing references.
3. **`${basedir}` is the only interpolation resolved**, against the pom's own directory, which
   is what Maven means by it. `${project.build.directory}`, a user property, or an absolute
   path all yield nothing rather than a guess: they depend on a build kndo never runs, or name
   something outside the project.
4. **Every `<plugin>` element in the document is walked**, not a fixed path. In
   spring-petclinic the declaration sits at `project > profiles > profile > build > plugins >
   plugin`, four levels below the `<build>` a fixed lookup would try, and a project may declare
   the same compilation in more than one profile.

An unparseable pom degrades to silence. Poms carrying a DOCTYPE are parsed with `allow_dtd`
(roxmltree resolves no external entities) — finding that out in the field rather than in a
fixture is a mistake this codebase already made once, with `kndo:info-plist`.

## The rollup hazard this plugin taught

`analysis/rollup.rs` collapses a uniformly dead directory into one finding. Reviving *some* of
its files turns 1 finding into N — which is what happens if the CSS is rescued and the Sass is
not. That is why the field-shaped proof pairs this plugin with `kndo:thymeleaf`: alone, this
one makes the output generated while the input stays dead, and the delta is a rollup breaking
rather than a gap closing. Any future plugin that revives part of a directory inherits the
same hazard.

## Proven by

`libsass_compilation_is_gated_by_the_pom_declaration` in
`crates/kndo/tests/builtin_plugin_proofs.rs`, over the shared reduced-petclinic fixture with
the thymeleaf starter declared in **both** runs — without it the CSS is dead, and a dead
output can keep nothing alive, so the link back to the Sass would be true and invisible. The
control is a `.scss` outside the declared `inputPath`: dead before and after, which is what
distinguishes this plugin from one that keeps all Sass alive.

## What it does not do

- **It parses no Sass.** `@import`/`@use` between `.scss` files are not followed; every file
  under `inputPath` is linked from every file under `outputPath`, which is keep-alive-only and
  cannot accuse.
- **It resolves no path it cannot point at a real file for** (rule 3).
- **Nothing for the Gradle, npm or dart-sass builds.** Each is a different tool with a
  different declaration, so each is a different plugin — the id is the tool's own name for
  exactly this reason.
