//! The first real language: TypeScript and JavaScript (`.ts`, `.tsx`, `.js`, `.jsx`,
//! `.mjs`, `.cjs`), through the tree-sitter-typescript grammars. The adapter reports
//! evidence only — declarations with reach and export aliases, ESM imports in every
//! shape, keep-alive-biased references, comment spans — and resolves relative
//! specifiers; judgment stays in the engine.
//!
//! The adapter id is `js-ts`, the same id v1 used for this territory, so oracle
//! comparisons line up file-for-file.

mod extract;
mod manifest;
mod resolve;

use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::{Attachment, EvidenceSink, RootKind};
use kndo_contract::plugin::{FileRole, Plugin, PluginSpec, PublishedSurface, Rung, Step};
use kndo_contract::vocab::{Confidence, ProjectPath};
use tree_sitter::Language;

/// The `.d.ts` fact, spelled once: the type-declaration companion extension that
/// resolution tries after the TS pair, wildcard exports anchor, and JS entries
/// publish beside themselves.
pub(crate) const TYPE_DECLARATION_EXT: &str = "d.ts";

/// Node's own modules (`module.builtinModules`) plus the specifier schemes a
/// runtime resolves itself: never a dependency to declare. Subpaths
/// (`fs/promises`, `assert/strict`) match through the package-name spelling.
const NODE_BUILTINS: &[&str] = &[
    "node:",
    "bun:",
    "deno:",
    "npm:",
    "jsr:",
    "data:",
    "http:",
    "https:",
    "virtual:",
    "assert",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
    "dns",
    "domain",
    "events",
    "fs",
    "http",
    "http2",
    "https",
    "inspector",
    "module",
    "net",
    "os",
    "path",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "repl",
    "stream",
    "string_decoder",
    "sys",
    "timers",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
];

pub struct TypeScriptAdapter {
    spec: PluginSpec,
    /// Dotted resolution candidates in TS priority order, derived once from the
    /// spec's declared extensions (with `.d.ts` after the TS pair) — resolution and
    /// manifest logic read this, never a second extension list.
    resolution_exts: Vec<String>,
}

/// What JAVASCRIPT ITSELF dispatches on. A `#!` line is the one fact here no
/// path convention can state: the file says it is run, whatever it is called
/// and wherever it sits. Framework dispatch — Storybook's `*.stories.*`,
/// Vitest's config patterns — is its pack's (M8.e).
fn dispatch_rules() -> Vec<kndo_contract::plugin::DispatchRule> {
    use kndo_contract::plugin::{DispatchRule, Effect, Trigger};
    vec![DispatchRule {
        when: Trigger::Marker {
            path: SHEBANG.into(),
            arg: None,
            target: None,
        },
        then: Effect::Root(RootKind::Production),
        confidence: Confidence::Certain,
    }]
}

impl TypeScriptAdapter {
    pub fn new() -> Self {
        let spec = kndo_toolkit::source_adapter_builder(
            "kndo:js-ts",
            // 15: `package.json`'s `imports` and `exports` maps are alias
            // tables with captures and conditions, so a `#` import and a
            // package subpath resolve to the file the manifest names.
            15,
            &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"],
            // `tsconfig.json` states the names that are not packages — its
            // `paths` aliases; every other spelling of the file is a variant
            // of the same config (`tsconfig.app.json`, `tsconfig.base.json`).
            &["**/package.json", "**/tsconfig.json", "**/tsconfig.*.json"],
            // ESM/CJS initialization order makes cycles bite: TDZ errors and
            // partially-initialized modules at run time.
            kndo_contract::plugin::CycleTolerance::Hazard,
        )
        // npm's installed dependencies are never the project's own source,
        // committed or not.
        .ignores(&["**/node_modules/**"])
        // Two dot-named directories the ecosystem puts real source in: a
        // Storybook config with its stories, a VitePress site with its theme.
        // Hidden by convention, never by intent, and no manifest glob names
        // them, so discovery is told out loud.
        .hidden_opt_in(&[".storybook", ".vitepress"])
        // GitHub Actions steps hand files to the same runtimes npm scripts do.
        .launchers(&[
            "**/.github/workflows/*.yml",
            "**/.github/workflows/*.yaml",
            "**/action.yml",
            "**/action.yaml",
        ])
        // Two rungs: a top-level declaration is either exported or its
        // module's own — dropping `export` is the narrowing tsc then checks,
        // turning any missed external use into a compile error. Members have
        // no step here: `unexported` is a keyword only a top-level takes.
        .ladder(&[
            Step::for_free(Rung::File, "unexported"),
            Step::new(Rung::Exported, "export"),
        ])
        // The ecosystem's habits, where no manifest said what a file is: the
        // runners collect `*.test.*`/`*.spec.*` and everything under a test
        // directory, and each tool reads its own `*.config.*` or rc-dotfile.
        // Habit, never rule — nothing in npm enforces either name — so both
        // are `Probable`, and they overlap freely: a `vitest.config.ts` under
        // `test/` is a tool's file AND a test's, and the engine takes both.
        .file_roles(&[
            FileRole::probable("__tests__/**", RootKind::Test),
            FileRole::probable("**/__tests__/**", RootKind::Test),
            FileRole::probable("test/**", RootKind::Test),
            FileRole::probable("**/test/**", RootKind::Test),
            FileRole::probable("tests/**", RootKind::Test),
            FileRole::probable("**/tests/**", RootKind::Test),
            FileRole::probable("**/*.test.*", RootKind::Test),
            FileRole::probable("**/*.spec.*", RootKind::Test),
            FileRole::probable("**/*.config.*", RootKind::Tooling),
            FileRole::probable("**/.*rc.*", RootKind::Tooling),
        ])
        // An npm package resolves through `main`/`exports`: what an entry
        // exports is published, and an export no entry reaches is internal
        // however it is spelled.
        .published_surface(PublishedSurface::Entries)
        // `lodash/fp` names `lodash`; a scoped name carries its own slash.
        .dependency_identity(kndo_contract::plugin::DependencyIdentity::PackageName)
        .dependency_builtins(kndo_contract::plugin::DependencyBuiltins::Named(
            NODE_BUILTINS
                .iter()
                .map(|s| smol_str::SmolStr::new_static(s))
                .collect(),
        ))
        // Files that carry ESM imports without being JS/TS: single-file
        // components, pages with module scripts, stylesheets with `@import`,
        // markdown-with-modules, server templates that embed JS.
        .dependency_importers(&[
            "vue", "svelte", "astro", "marko", "html", "htm", "css", "scss", "sass", "less",
            "styl", "pcss", "mdx", "coffee", "ejs", "pug",
        ])
        .dispatch(dispatch_rules())
        .build();
        let mut resolution_exts = Vec::new();
        for ext in spec.suffixes() {
            resolution_exts.push(format!(".{ext}"));
            if ext == "tsx" {
                resolution_exts.push(format!(".{TYPE_DECLARATION_EXT}"));
            }
        }
        TypeScriptAdapter {
            spec,
            resolution_exts,
        }
    }
}

impl Default for TypeScriptAdapter {
    fn default() -> Self {
        TypeScriptAdapter::new()
    }
}

/// `.ts` gets the TypeScript grammar (where `<T>` casts are legal); everything else —
/// `.tsx`, `.jsx`, and plain JS in all its extensions — gets TSX, whose JSX support
/// is a superset of what those files can contain.
fn language_for(path: &ProjectPath) -> Language {
    grammar(path.as_str().rsplit('.').next().unwrap_or(""))
}

/// TypeScript's own grammar for a `ts` suffix; the TSX grammar for every
/// other, which reads JavaScript, JSX and TSX alike.
fn grammar(suffix: &str) -> Language {
    if suffix == "ts" {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    } else {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    }
}

impl Plugin for TypeScriptAdapter {
    fn spec(&self) -> &PluginSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        // Convention roots come from the path and the first bytes, before any parse:
        // a test file that fails to parse must still be rooted, or the parse failure
        // would turn into an unreachable-file accusation. An embedded region has
        // none of its own: its host rooted the page, and the path is the host's.
        if file.region.is_none() {
            convention_roots(file, out);
        }
        let language = match file.region {
            Some(region) => grammar(region.language.as_str()),
            None => language_for(file.path),
        };
        if let Some(tree) = kndo_toolkit::parse_reporting(&language, file.content, out) {
            extract::extract(file.content, &tree, file.region.map(|r| r.mode), out);
        }
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        resolve::resolve(from, specifier, cx, &self.resolution_exts)
    }

    fn extract_manifest(
        &self,
        manifest: &SourceFile<'_>,
        cx: &ResolveContext<'_>,
        out: &mut kndo_contract::manifest::ManifestSink,
    ) {
        manifest::structure(manifest, cx, &self.resolution_exts, out);
    }
}

/// The marker a `#!` line is reported as. A name this crate owns, so the fact
/// and the rule that reads it cannot drift apart.
const SHEBANG: &str = "#!";

/// What the FILE ITSELF says about its role — the one fact here no path
/// convention can state: a `#!` line makes the file an executable entry
/// whatever it is called and wherever it sits. The path conventions
/// (`*.test.*`/`*.spec.*`/`__tests__/`, `*.config.*`, rc-dotfiles) are the
/// spec's `file_roles`; what stays is the membership those paths imply, which
/// is evidence: a spec file joins the project in a test run alone.
fn convention_roots(file: &SourceFile<'_>, out: &mut EvidenceSink) {
    if file.content.starts_with(b"#!") {
        out.marker(
            kndo_contract::evidence::MarkerTarget::File,
            SHEBANG,
            Vec::new(),
            kndo_contract::vocab::Span::new(0, 2),
        );
    }
    if kndo_toolkit::web_test_path(file.path.as_str()) {
        out.attachment(Attachment::TestOnly);
    }
}
