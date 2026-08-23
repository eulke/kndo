//! Pure path classification for Next.js's file-system conventions —
//! string in, tier out, no graph types, so every rule is unit-testable in isolation.

/// Which convention surface a file belongs to — each tier carries its own set of
/// framework-consumed export names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tier {
    /// Any file under a pages directory — being routed is definitional.
    Pages,
    /// A reserved basename under an app directory.
    AppSpecial,
    /// `middleware.*` directly at an app root.
    Middleware,
    /// `instrumentation.*` directly at an app root.
    Instrumentation,
}

/// App-router reserved basenames (minus extension) — Next.js 13–15's full set, including the
/// code-generating metadata files (`sitemap.ts` et al). Arbitrary other files under `app/`
/// are ordinary modules and must earn reachability through imports.
const APP_SPECIAL_STEMS: &[&str] = &[
    "page",
    "layout",
    "template",
    "loading",
    "error",
    "global-error",
    "not-found",
    "default",
    "route",
    "icon",
    "apple-icon",
    "opengraph-image",
    "twitter-image",
    "sitemap",
    "robots",
    "manifest",
];

const PAGES_EXPORTS: &[&str] = &[
    "getServerSideProps",
    "getStaticProps",
    "getStaticPaths",
    "config",
    "reportWebVitals",
];

const APP_EXPORTS: &[&str] = &[
    "generateMetadata",
    "generateStaticParams",
    "generateImageMetadata",
    "generateSitemaps",
    "generateViewport",
    "metadata",
    "viewport",
    "revalidate",
    "dynamic",
    "dynamicParams",
    "fetchCache",
    "runtime",
    "preferredRegion",
    "maxDuration",
    "GET",
    "POST",
    "PUT",
    "PATCH",
    "DELETE",
    "HEAD",
    "OPTIONS",
];

const MIDDLEWARE_EXPORTS: &[&str] = &["middleware", "config"];

const INSTRUMENTATION_EXPORTS: &[&str] = &["register", "onRequestError"];

impl Tier {
    /// Exports the framework calls by name for this tier — rooted at `Certain`; every other
    /// exported top-level symbol is rooted at `Probable` (the default-export component's local
    /// name is arbitrary — a deliberate over-approximation).
    pub(crate) fn certain_exports(self) -> &'static [&'static str] {
        match self {
            Tier::Pages => PAGES_EXPORTS,
            Tier::AppSpecial => APP_EXPORTS,
            Tier::Middleware => MIDDLEWARE_EXPORTS,
            Tier::Instrumentation => INSTRUMENTATION_EXPORTS,
        }
    }
}

/// Directories that anchor Next's conventions: any directory directly containing a
/// `package.json` or a `next.config.*`. `""` is the project root. Sorted + deduped so
/// classification order (and with it contribution order) is deterministic.
pub(crate) fn app_roots<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut roots: Vec<String> = paths
        .into_iter()
        .filter(|p| is_anchor(basename(p)))
        .map(|p| dirname(p).to_string())
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

fn is_anchor(base: &str) -> bool {
    base == "package.json"
        || base
            .strip_prefix("next.config.")
            .is_some_and(|ext| !ext.is_empty() && !ext.contains('.'))
}

/// Which convention tier `path` belongs to, if any, relative to the nearest matching app root.
/// `page_extensions` is a per-root override of which extensions count as
/// a page/app-router file (empty/missing entry for a root = the framework's own unfiltered
/// default). Only `pages`/`app` tiers are ever
/// extension-restricted — `middleware`/`instrumentation` aren't governed by `pageExtensions` in
/// real Next.js, so `support_tier` never consults it.
pub(crate) fn classify(
    path: &str,
    app_roots: &[String],
    page_extensions: &std::collections::BTreeMap<String, Vec<String>>,
) -> Option<Tier> {
    app_roots
        .iter()
        .filter_map(|root| relative_to(path, root).map(|rel| (root, rel)))
        .find_map(|(root, rel)| classify_rel(rel, page_extensions.get(root).map(Vec::as_slice)))
}

/// `path` relative to `root`, when `path` is inside it (`root == ""` is the project root).
fn relative_to<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    if root.is_empty() {
        return Some(path);
    }
    path.strip_prefix(root)?.strip_prefix('/')
}

fn classify_rel(rel: &str, extensions: Option<&[String]>) -> Option<Tier> {
    // `src/` is an equivalent prefix for every tier — one strip covers all.
    let rel = rel.strip_prefix("src/").unwrap_or(rel);
    pages_tier(rel, extensions)
        .or_else(|| app_tier(rel, extensions))
        .or_else(|| support_tier(rel))
}

fn pages_tier(rel: &str, extensions: Option<&[String]>) -> Option<Tier> {
    let rest = rel.strip_prefix("pages/").filter(|rest| !rest.is_empty())?;
    extension_allowed(basename(rest), extensions).then_some(Tier::Pages)
}

fn app_tier(rel: &str, extensions: Option<&[String]>) -> Option<Tier> {
    let rest = rel.strip_prefix("app/")?;
    let base = basename(rest);
    (APP_SPECIAL_STEMS.contains(&stem(base)) && extension_allowed(base, extensions))
        .then_some(Tier::AppSpecial)
}

/// `None` (no override for this root) always allows — the framework's own default extension
/// set, which is exactly what every file this classifier ever sees already satisfies (only
/// `js-ts`-claimed files reach here — lib.rs's `is_production_js` gate). A `Some` override
/// requires the basename to end with `.<ext>` for at least one declared extension — `pages/
/// foo.tsx` under `pageExtensions: ["page.tsx"]` is real Next.js behavior: *not* a page.
fn extension_allowed(basename: &str, extensions: Option<&[String]>) -> bool {
    match extensions {
        None => true,
        Some(exts) => exts
            .iter()
            .any(|ext| basename.ends_with(&format!(".{ext}"))),
    }
}

/// Statically-extractable `pageExtensions` array from a `next.config.*` file's raw source — no
/// JS evaluation, ever: only statically readable
/// values count. Finds a `pageExtensions` key followed only by whitespace/`:`/`=`
/// and then a `[...]` literal of plain quoted strings; anything else — a variable, a spread, a
/// function call, the key missing entirely — yields `None`, and the caller keeps this root's
/// classification unfiltered rather than guessing at a dynamic value.
pub(crate) fn static_page_extensions(config_source: &str) -> Option<Vec<String>> {
    let array_body = page_extensions_array_body(config_source)?;
    let extensions: Option<Vec<String>> = array_body
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| strip_quotes(t).map(str::to_string))
        .collect();
    extensions.filter(|exts| !exts.is_empty())
}

/// The raw text between `[` and `]` of a `pageExtensions: [...]`/`pageExtensions = [...]`
/// assignment, or `None` if the key is missing or isn't immediately followed by an array
/// literal (a variable, a spread, a function call — anything this can't safely read).
fn page_extensions_array_body(config_source: &str) -> Option<&str> {
    let key_at = config_source.find("pageExtensions")?;
    let after_key = &config_source[key_at + "pageExtensions".len()..];
    let bracket_at = plain_assignment_bracket(after_key)?;
    let close_at = after_key[bracket_at..].find(']')? + bracket_at;
    Some(&after_key[bracket_at + 1..close_at])
}

/// The index of the `[` that opens this key's value, only when nothing but whitespace/`:`/`=`
/// separates the key from it — anything else (a type annotation, a comment, unrelated code
/// before an unrelated later `[`) means this isn't really `pageExtensions`'s own assignment.
fn plain_assignment_bracket(after_key: &str) -> Option<usize> {
    let bracket_at = after_key.find('[')?;
    let is_plain_assignment = after_key[..bracket_at]
        .chars()
        .all(|c| c.is_whitespace() || c == ':' || c == '=');
    is_plain_assignment.then_some(bracket_at)
}

fn strip_quotes(token: &str) -> Option<&str> {
    let bytes = token.as_bytes();
    let quoted = bytes.len() >= 2
        && (token.starts_with('"') || token.starts_with('\''))
        && bytes[0] == bytes[bytes.len() - 1];
    quoted.then(|| &token[1..token.len() - 1])
}

/// Support files live *directly* at the app root — any remaining `/` disqualifies.
fn support_tier(rel: &str) -> Option<Tier> {
    let tier = match stem(rel) {
        "middleware" => Tier::Middleware,
        "instrumentation" => Tier::Instrumentation,
        _ => return None,
    };
    (!rel.contains('/')).then_some(tier)
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn dirname(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Portion before the first `.` — `page.tsx` → `page`, and multi-part names like
/// `middleware.test.ts` collapse to `middleware` too (harmless: role gating in lib.rs already
/// excludes non-production files before classification).
fn stem(base: &str) -> &str {
    base.split('.').next().unwrap_or(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(paths: &[&str]) -> Vec<String> {
        app_roots(paths.iter().copied())
    }

    fn no_overrides() -> std::collections::BTreeMap<String, Vec<String>> {
        std::collections::BTreeMap::new()
    }

    #[test]
    fn app_roots_come_from_package_json_and_next_config() {
        let r = roots(&[
            "package.json",
            "apps/web/package.json",
            "apps/legacy/next.config.mjs",
            "apps/web/pages/index.tsx",
            "docs/next.config", // no extension — not an anchor
        ]);
        assert_eq!(r, vec!["", "apps/legacy", "apps/web"]);
    }

    #[test]
    fn pages_files_classify_at_any_matching_root() {
        let r = roots(&["package.json", "apps/web/package.json"]);
        assert_eq!(
            classify("pages/index.tsx", &r, &no_overrides()),
            Some(Tier::Pages)
        );
        assert_eq!(
            classify("src/pages/api/users.ts", &r, &no_overrides()),
            Some(Tier::Pages)
        );
        assert_eq!(
            classify("apps/web/pages/about.tsx", &r, &no_overrides()),
            Some(Tier::Pages)
        );
    }

    #[test]
    fn a_components_directory_named_pages_is_not_a_pages_root() {
        // The motivating counter-example: `src/components/pages/` is a component
        // category, not a router — it must never be swallowed by segment matching.
        let r = roots(&["package.json"]);
        assert_eq!(
            classify("src/components/pages/HomePage.tsx", &r, &no_overrides()),
            None
        );
    }

    #[test]
    fn app_router_only_matches_reserved_basenames() {
        let r = roots(&["package.json"]);
        assert_eq!(
            classify("app/page.tsx", &r, &no_overrides()),
            Some(Tier::AppSpecial)
        );
        assert_eq!(
            classify("app/blog/[slug]/page.tsx", &r, &no_overrides()),
            Some(Tier::AppSpecial)
        );
        assert_eq!(
            classify("src/app/api/users/route.ts", &r, &no_overrides()),
            Some(Tier::AppSpecial)
        );
        assert_eq!(
            classify("app/sitemap.ts", &r, &no_overrides()),
            Some(Tier::AppSpecial)
        );
        // Ordinary colocated modules under app/ must still earn reachability via imports.
        assert_eq!(
            classify("app/components/button.tsx", &r, &no_overrides()),
            None
        );
        assert_eq!(classify("app/lib/data.ts", &r, &no_overrides()), None);
    }

    #[test]
    fn support_files_only_match_directly_at_an_app_root() {
        let r = roots(&["package.json", "apps/web/package.json"]);
        assert_eq!(
            classify("middleware.ts", &r, &no_overrides()),
            Some(Tier::Middleware)
        );
        assert_eq!(
            classify("src/middleware.ts", &r, &no_overrides()),
            Some(Tier::Middleware)
        );
        assert_eq!(
            classify("apps/web/instrumentation.ts", &r, &no_overrides()),
            Some(Tier::Instrumentation)
        );
        assert_eq!(classify("lib/middleware.ts", &r, &no_overrides()), None);
        assert_eq!(classify("middleware/index.ts", &r, &no_overrides()), None);
    }

    #[test]
    fn nothing_classifies_without_an_enclosing_app_root() {
        // `unrelated/pages/**` has no package.json/next.config anchor above it.
        let r = roots(&["apps/web/package.json"]);
        assert_eq!(
            classify("unrelated/pages/index.tsx", &r, &no_overrides()),
            None
        );
    }

    #[test]
    fn next_config_itself_is_an_anchor_but_never_classified() {
        let r = roots(&["next.config.js"]);
        assert_eq!(r, vec![""]);
        assert_eq!(classify("next.config.js", &r, &no_overrides()), None);
    }

    #[test]
    fn each_tier_has_its_own_certain_exports() {
        assert!(Tier::Pages.certain_exports().contains(&"getStaticProps"));
        assert!(Tier::AppSpecial.certain_exports().contains(&"GET"));
        assert!(Tier::Middleware.certain_exports().contains(&"config"));
        assert!(Tier::Instrumentation
            .certain_exports()
            .contains(&"register"));
        assert!(!Tier::Pages.certain_exports().contains(&"GET"));
    }

    // ------------------------------------------------------ pageExtensions overrides

    #[test]
    fn static_page_extensions_reads_a_plain_array_literal() {
        let source = "module.exports = { pageExtensions: ['page.tsx', 'page.ts'] }";
        assert_eq!(
            static_page_extensions(source),
            Some(vec!["page.tsx".to_string(), "page.ts".to_string()])
        );
    }

    #[test]
    fn static_page_extensions_handles_double_quotes_and_whitespace() {
        let source =
            "const nextConfig = {\n  pageExtensions: [\n    \"mdx\",\n    \"tsx\",\n  ],\n};\n";
        assert_eq!(
            static_page_extensions(source),
            Some(vec!["mdx".to_string(), "tsx".to_string()])
        );
    }

    #[test]
    fn static_page_extensions_is_none_for_a_dynamic_value() {
        // Not a literal array — evaluating `DEFAULT_EXTENSIONS` would require running the JS,
        // which is explicitly out of scope.
        assert_eq!(
            static_page_extensions("pageExtensions: DEFAULT_EXTENSIONS"),
            None
        );
    }

    #[test]
    fn static_page_extensions_is_none_when_the_key_is_absent() {
        assert_eq!(static_page_extensions("module.exports = {}"), None);
    }

    #[test]
    fn custom_page_extensions_narrow_which_files_classify() {
        let r = roots(&["package.json"]);
        let mut overrides = no_overrides();
        overrides.insert(String::new(), vec!["page.tsx".to_string()]);
        // Matches the custom suffix — still a page.
        assert_eq!(
            classify("pages/index.page.tsx", &r, &overrides),
            Some(Tier::Pages)
        );
        // With pageExtensions customized, a plain .tsx under pages/ does not qualify —
        // real Next.js would not route this file either.
        assert_eq!(classify("pages/index.tsx", &r, &overrides), None);
    }

    #[test]
    fn custom_page_extensions_are_per_root_in_a_monorepo() {
        let r = roots(&["package.json", "apps/web/package.json"]);
        let mut overrides = no_overrides();
        overrides.insert("apps/web".to_string(), vec!["page.tsx".to_string()]);
        // The root app has no override — unfiltered.
        assert_eq!(
            classify("pages/index.tsx", &r, &overrides),
            Some(Tier::Pages)
        );
        // apps/web does — only the custom suffix qualifies there.
        assert_eq!(classify("apps/web/pages/index.tsx", &r, &overrides), None);
        assert_eq!(
            classify("apps/web/pages/index.page.tsx", &r, &overrides),
            Some(Tier::Pages)
        );
    }

    #[test]
    fn custom_page_extensions_never_restrict_support_files() {
        // middleware/instrumentation aren't governed by pageExtensions in real Next.js.
        let r = roots(&["package.json"]);
        let mut overrides = no_overrides();
        overrides.insert(String::new(), vec!["page.tsx".to_string()]);
        assert_eq!(
            classify("middleware.ts", &r, &overrides),
            Some(Tier::Middleware)
        );
    }
}
