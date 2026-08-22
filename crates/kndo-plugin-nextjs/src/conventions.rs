//! Pure path classification for Next.js's file-system conventions (docs/plugins/nextjs.md
//! §3/§4) — string in, tier out, no graph types, so every rule is unit-testable in isolation.

/// Which convention surface a file belongs to — each tier carries its own set of
/// framework-consumed export names (spec §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tier {
    /// Any file under a pages directory — being routed is definitional (spec §4.1).
    Pages,
    /// A reserved basename under an app directory (spec §4.2).
    AppSpecial,
    /// `middleware.*` directly at an app root (spec §4.3).
    Middleware,
    /// `instrumentation.*` directly at an app root (spec §4.3).
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
    /// name is arbitrary — spec §4.1's documented over-approximation).
    pub(crate) fn certain_exports(self) -> &'static [&'static str] {
        match self {
            Tier::Pages => PAGES_EXPORTS,
            Tier::AppSpecial => APP_EXPORTS,
            Tier::Middleware => MIDDLEWARE_EXPORTS,
            Tier::Instrumentation => INSTRUMENTATION_EXPORTS,
        }
    }
}

/// Directories that anchor Next's conventions (spec §3): any directory directly containing a
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
pub(crate) fn classify(path: &str, app_roots: &[String]) -> Option<Tier> {
    app_roots
        .iter()
        .filter_map(|root| relative_to(path, root))
        .find_map(classify_rel)
}

/// `path` relative to `root`, when `path` is inside it (`root == ""` is the project root).
fn relative_to<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    if root.is_empty() {
        return Some(path);
    }
    path.strip_prefix(root)?.strip_prefix('/')
}

fn classify_rel(rel: &str) -> Option<Tier> {
    // `src/` is an equivalent prefix for every tier (spec §3's table) — one strip covers all.
    let rel = rel.strip_prefix("src/").unwrap_or(rel);
    pages_tier(rel)
        .or_else(|| app_tier(rel))
        .or_else(|| support_tier(rel))
}

fn pages_tier(rel: &str) -> Option<Tier> {
    rel.strip_prefix("pages/")
        .filter(|rest| !rest.is_empty())
        .map(|_| Tier::Pages)
}

fn app_tier(rel: &str) -> Option<Tier> {
    rel.strip_prefix("app/")
        .filter(|rest| APP_SPECIAL_STEMS.contains(&stem(basename(rest))))
        .map(|_| Tier::AppSpecial)
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
        assert_eq!(classify("pages/index.tsx", &r), Some(Tier::Pages));
        assert_eq!(classify("src/pages/api/users.ts", &r), Some(Tier::Pages));
        assert_eq!(classify("apps/web/pages/about.tsx", &r), Some(Tier::Pages));
    }

    #[test]
    fn a_components_directory_named_pages_is_not_a_pages_root() {
        // The spec's motivating counter-example (§3): `src/components/pages/` is a component
        // category, not a router — it must never be swallowed by segment matching.
        let r = roots(&["package.json"]);
        assert_eq!(classify("src/components/pages/HomePage.tsx", &r), None);
    }

    #[test]
    fn app_router_only_matches_reserved_basenames() {
        let r = roots(&["package.json"]);
        assert_eq!(classify("app/page.tsx", &r), Some(Tier::AppSpecial));
        assert_eq!(
            classify("app/blog/[slug]/page.tsx", &r),
            Some(Tier::AppSpecial)
        );
        assert_eq!(
            classify("src/app/api/users/route.ts", &r),
            Some(Tier::AppSpecial)
        );
        assert_eq!(classify("app/sitemap.ts", &r), Some(Tier::AppSpecial));
        // Ordinary colocated modules under app/ must still earn reachability via imports.
        assert_eq!(classify("app/components/button.tsx", &r), None);
        assert_eq!(classify("app/lib/data.ts", &r), None);
    }

    #[test]
    fn support_files_only_match_directly_at_an_app_root() {
        let r = roots(&["package.json", "apps/web/package.json"]);
        assert_eq!(classify("middleware.ts", &r), Some(Tier::Middleware));
        assert_eq!(classify("src/middleware.ts", &r), Some(Tier::Middleware));
        assert_eq!(
            classify("apps/web/instrumentation.ts", &r),
            Some(Tier::Instrumentation)
        );
        assert_eq!(classify("lib/middleware.ts", &r), None);
        assert_eq!(classify("middleware/index.ts", &r), None);
    }

    #[test]
    fn nothing_classifies_without_an_enclosing_app_root() {
        // `unrelated/pages/**` has no package.json/next.config anchor above it.
        let r = roots(&["apps/web/package.json"]);
        assert_eq!(classify("unrelated/pages/index.tsx", &r), None);
    }

    #[test]
    fn next_config_itself_is_an_anchor_but_never_classified() {
        let r = roots(&["next.config.js"]);
        assert_eq!(r, vec![""]);
        assert_eq!(classify("next.config.js", &r), None);
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
}
