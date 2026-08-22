//! Pure path classification for Express's entry-file conventions (docs/plugins/express.md §3)
//! — string in, bool out, no graph types.

/// Directories that anchor the entry convention: any directory directly containing a
/// `package.json` (`""` is the project root). Same derivation as nextjs.md §3 minus the
/// `next.config.*` anchor — Express has no config-file convention to anchor on.
pub(crate) fn app_roots<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut roots: Vec<String> = paths
        .into_iter()
        .filter(|p| basename(p) == "package.json")
        .map(|p| dirname(p).to_string())
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

/// Whether `path` is a conventional entry candidate at some app root:
/// `R/app.<ext>`, `R/server.<ext>`, `R/src/app.<ext>`, `R/src/server.<ext>`.
///
/// `index.<ext>` is deliberately excluded (spec §3): root-level `index` files are the JS
/// ecosystem's package-main convention, not an Express signal — including them would
/// blanket-exempt library surfaces in every monorepo that uses Express somewhere.
pub(crate) fn is_entry(path: &str, app_roots: &[String]) -> bool {
    app_roots
        .iter()
        .filter_map(|root| relative_to(path, root))
        .any(entry_rel)
}

fn relative_to<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    if root.is_empty() {
        return Some(path);
    }
    path.strip_prefix(root)?.strip_prefix('/')
}

fn entry_rel(rel: &str) -> bool {
    let rel = rel.strip_prefix("src/").unwrap_or(rel);
    !rel.contains('/') && matches!(stem(rel), "app" | "server")
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn dirname(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

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
    fn entries_match_at_root_and_under_src() {
        let r = roots(&["package.json"]);
        assert!(is_entry("app.js", &r));
        assert!(is_entry("server.ts", &r));
        assert!(is_entry("src/app.js", &r));
        assert!(is_entry("src/server.mjs", &r));
    }

    #[test]
    fn entries_match_per_package_in_a_monorepo() {
        let r = roots(&["package.json", "services/api/package.json"]);
        assert!(is_entry("services/api/app.js", &r));
        assert!(is_entry("services/api/src/server.ts", &r));
        // No anchor above it → not an entry, whatever it's named.
        assert!(!is_entry("scripts/app.js", &r));
    }

    #[test]
    fn index_and_nested_files_are_not_entries() {
        let r = roots(&["package.json"]);
        assert!(!is_entry("index.js", &r));
        assert!(!is_entry("src/index.js", &r));
        assert!(!is_entry("lib/app.js", &r));
        assert!(!is_entry("src/routes/app.js", &r));
    }
}
