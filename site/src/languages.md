# Languages

Every language lands through the same adapter contract — the core never contains
`if language == X`. What each adapter understands:

| Language | Files | Manifests | Highlights |
|---|---|---|---|
| JavaScript / TypeScript | `.ts .tsx .js .jsx .mjs .cjs .mts .cts` (incl. `.d.ts`) | `package.json` | `exports`/`main` entry roots, workspaces, barrel re-export chains, `test`/`tests`/`__tests__` conventions, script-invoked CLI deps |
| Go | `.go` | `go.mod`, `go.work` | capitalization visibility, compiler-walled `internal/`, `_test.go` packages, `// indirect` excluded from declared deps |
| Rust | `.rs` | `Cargo.toml` | module tree from `lib.rs`/`main.rs`, `pub`/`pub(crate)` ladder, `#[cfg(test)]` regions, macro token-tree scanning, workspace topology |
| Java | `.java` | `pom.xml`, `build.gradle(.kts)` | `src/main/java` promotion, `@Override` + Serializable-hook dispatch roots, implicit-public interface members, `<init>` constructors |
| Kotlin | `.kt` | shared Gradle/Maven | `internal` ladder, primary/secondary constructors, `.kt` under `src/main/java` too |
| Swift | `.swift` | `Package.swift` | SPM targets (incl. custom `path:`), XCTest role, `open/public/internal/…` ladder, protocol-witness rooting, `init`/`deinit` |
| JSON | `.json` | — | references into/out of config files |
| CSS | `.css .scss` | — | selectors and custom properties as symbols; `unused` CSS variables fall out of the same reachability |

Mixed repos are the point: one graph, cross-language edges, one report.

Adapters are held to a shared **conformance harness** — fixture projects with expected-finding
JSON that every adapter must reproduce exactly — plus per-language visibility "ladders" so
`internal-only` speaks each language's own words (`pub(crate)`, `fileprivate`,
`package-private`) in its remediation text.
