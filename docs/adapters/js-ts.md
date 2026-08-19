# Adapter Spec — JavaScript / TypeScript

**Status:** Draft · **Implements:** `LanguageAdapter` (contracts §2) · **Milestone:** M1
**Grammar:** tree-sitter-typescript (TS + TSX variants)

The first adapter, and deliberately the hardest: JS/TS has two module systems, three ways to
export the same thing, dynamic everything, and the largest AI-generated-code surface. If the
contracts survive this document unchanged, they are probably right.

## 1. Claiming & classification

| Claim | Files |
|-------|-------|
| Language `js-ts` | `.ts .tsx .js .jsx .mjs .cjs .mts .cts .d.ts` |
| Manifests | `package.json`, `pnpm-workspace.yaml` (topology only), lockfiles are **not** claimed |
| Role `test` | `*.test.*`, `*.spec.*`, `__tests__/**`, `__mocks__/**` |
| Role `tooling` | `*.config.{js,ts,mjs,cjs}` (webpack/vite/jest/eslint/…), `.storybook/**`, `scripts/**` when referenced from package.json `scripts` |
| Origin `generated` | first-comment markers (`@generated`, `eslint-disable` + `AUTO-GENERATED`, GraphQL/protobuf codegen banners), `*.d.ts` siblings of a same-name `.ts` emitted by `tsc` |
| Origin `vendored` | `vendor/**`, `third_party/**` |

`.d.ts` files contribute *declarations only* (no runtime edges); an ambient `declare global` or
`declare module "x"` block marks its symbols externally-consumed (they exist to be seen by the
outside).

## 2. Extraction

**Declarations** — functions, classes (+ methods, fields, getters/setters as members),
interfaces, type aliases, enums (+ members), top-level `const`/`let`/`var`, namespaces.
Anonymous default exports (`export default () => {}`) declare a synthetic symbol named
`default` (symbol path: `file#default`).

**Export surface** — `export` named/default, `export { a as b }`, re-exports
(`export * from`, `export { x } from`), CJS (`module.exports = …`, `exports.foo = …`).
`module.exports = { a, b }` with identifier shorthand exports those symbols `certain`;
computed/spread members demote the file's export surface to `probable`.

**References** — identifier uses with scope context, member accesses, `extends`/`implements`
(→ `RefKind::Extend/Implement`), type positions (→ `TypeUse`), **JSX element names** —
`<Button/>` is a `certain` reference to `Button`, which is what keeps React components alive
without any framework plugin (framework *roots* remain plugin territory, RFC 0003).

**Dynamic constructs → `DynamicUse`** (wildcard edges, RFC 0005 §1 expansion):

| Construct | Effect |
|-----------|--------|
| `import(expr)` / `require(expr)`, non-literal | wildcard, scope narrowed by any static string prefix (`./locales/${x}` → that directory) |
| `eval`, `new Function` | wildcard, file-wide |
| computed member access on module namespace (`ns[key]`) | wildcard over that namespace's exports |
| string-keyed registries (`obj["handler"]` patterns) | plain `possible` reference when the literal resolves; wildcard otherwise |

**Metrics** — cyclomatic complexity: +1 per `if / for / while / do / case / catch / && / || /
?? / ?: / optional-chain-call`. Fingerprints: normalized token stream per function body
(consistent `$n` renaming per RFC 0005 §6; template-literal text canonicalized, string/number
literals bucketed).

**Suppressions** — `// kndo:allow …`, `/* kndo:allow … */`, JSX `{/* kndo:allow … */}`.

## 3. Imports & resolution

Emitted import kinds and their confidence:

| Form | Edge | Confidence |
|------|------|-----------|
| ESM static (`import x from "spec"`) | file/dep | certain |
| `import type` / type-only re-export | file/dep, edges are `TypeUse` | certain; counts as usage for `@types/*`-mapped deps, and for the runtime dep only via its types |
| CJS `require("literal")` | file/dep | certain |
| `import("literal")` | file/dep | probable (bundler code-split semantics) |
| side-effect (`import "./polyfill"`) | file, `side_effect_only` | certain |
| `require.resolve`, `new URL("./x", import.meta.url)` | file | probable |

**Resolution algorithm** (the adapter's `resolve`): Node's algorithm, both module systems —
relative/absolute paths with extension resolution order (explicit > `.ts .tsx .mts .cts` >
`.js .jsx .mjs .cjs` > `.d.ts` > directory `index.*`), self-referencing package `imports`
(`#internal/*`), package `exports` maps (conditions: `types`, `import`, `require`, `default` —
evaluated in that order), `main`/`module`/`types` fallbacks, `tsconfig` `baseUrl` + `paths`
(nearest `tsconfig.json` upward, `extends` chains followed), workspace packages (RFC 0011:
`workspace:*` and name matches against sibling manifests resolve to internal files through the
sibling's own `exports`), pnpm symlink layouts resolved to real paths before ownership checks.
Specifiers that resolve into `node_modules` yield `Dependency` targets via the subpath→package
mapping (`lodash/fp` → `lodash`, `@scope/pkg/sub` → `@scope/pkg`). **Builtins** → `Stdlib` via
the shared `kndo-stdlib v1` mechanism (RFC 0002 §6; toolkit owns format, loader, and the
four-step precedence): the `node:` prefix is the *structural* signal (covers every post-v18
builtin forever — Node's own policy makes new builtins prefix-only), and the frozen legacy
bare-name set is generated data (`scripts/gen-stdlib-js.mjs` from `module.builtinModules` —
regenerated, never hand-edited). A manifest-declared dependency shadowing a builtin name
(userland `punycode`) resolves as the dependency, not the builtin — precedence rule 2. Asset specifiers (`.css .svg .png .json …`) resolve as cross-language
file edges when the file exists (RFC 0002 §4) — the CSS/JSON adapters claim the targets.

## 4. Manifests & packages (RFC 0011)

From `package.json`: name, `private`, `workspaces` globs (+ `pnpm-workspace.yaml` packages),
dependency scopes mapped `dependencies→prod`, `devDependencies→dev`, `peerDependencies→peer`,
`optionalDependencies→optional`; entry points (`main`, `module`, `exports`, `bin`, `types`) both
as resolution inputs and as **roots**: `bin` targets and the export surface of non-`private`
packages are production roots (library mode); `scripts` file references become tooling roots.

**Visibility ladder** (for `internal-only`/`private-type-leak`, RFC 0005 §7):
module-local < exported < **package-surface** (reachable through the package's `exports` map).
An exported symbol not reachable through `exports` is *exported but package-internal* — the
ladder makes "exported yet not part of the public surface" expressible, which is exactly where
monorepo over-exposure hides.

## 5. Known hard cases & stances

| Case | Stance |
|------|--------|
| Barrel files (`index.ts` re-export fans) | resolved through, transparently; re-exported symbols alive only if some consumer imports them through *any* path |
| Declaration merging (`interface X` twice, namespace+function) | one logical symbol, multiple declaration spans |
| Decorators | reference edges to the decorator expression; `emitDecoratorMetadata` adds `TypeUse` edges on decorated signatures; DI semantics stay in plugins |
| `declare module`/ambient/global augmentation | symbols marked externally-consumed (never `unused`/`internal-only`) |
| Triple-slash `/// <reference path>` | file edge, certain |
| UMD wrappers | detected by shape, treated as generated-style opaque exports at `probable` |
| Re-export of a whole dep (`export * from "lib"`) | keeps the dep used; contributes a `probable` wildcard export surface |
| `tsconfig` project references | not followed in 1.0 (parking lot); path aliases within one graph are |

## 6. Conformance fixtures (shared harness, RFC 0002 §8)

Minimum corpus, each a mini-project with expected `FileFacts` + findings: ESM app with dead
symbol/file/dep · CJS interop pair · dual-mode package (`exports` conditions) · tsconfig-paths
monorepo with workspace dep + phantom internal import (`undeclared`) + `version-skew` · JSX
component tree (component kept alive only by JSX usage) · dynamic-import directory scan
(wildcard narrows, nothing false-positive) · test-only module + its tests (`test-only` +
deletion set) · barrel with one consumed and one orphaned re-export · `.d.ts` ambient globals ·
duplicate function pair surviving rename+reformat (Type-2) · CRAP fixture with lcov ·
suppression matrix incl. one stale pragma.

## 7. Open questions

1. `export =` (TS legacy CJS export) — support at `certain` or demote to `probable`?
2. Should `scripts` in package.json parse shell to find file refs (`node scripts/build.mjs`),
   or is "token that resolves to a claimed file" enough for 1.0? Current draft: the latter.
3. `import type` keeping a *runtime* dependency alive: current draft says types-only usage keeps
   `@types/*` alive but does **not** count as runtime usage of the implementation package —
   confirm against real-world `devDependencies` conventions during dogfooding.
