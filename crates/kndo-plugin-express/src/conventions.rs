//! Pure path classification for Express's entry-file conventions —
//! string in, bool out, no graph types.

/// Directories that anchor the entry convention: any directory directly containing a
/// `package.json` (`""` is the project root). Same derivation as the nextjs plugin's minus
/// the `next.config.*` anchor — Express has no config-file convention to anchor on.
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
/// `index.<ext>` is deliberately excluded: root-level `index` files are the JS
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

/// Candidate entry paths derived from one app root's `package.json` content — the `"main"`
/// field and any `node`/`nodemon` invocation inside `"scripts"` values: deriving the true
/// entry from `scripts.start` instead of guessing by name alone. Pure parsing over
/// already-fetched bytes; the caller resolves
/// each candidate against the real claimed file set — this function only proposes, the same
/// "propose, host validates" split every plugin-contributed target has
/// (`PluginTarget` resolution). Malformed JSON yields no candidates, not an error
/// — a plugin degrading to its name heuristic on a manifest it can't parse is the same
/// silence-over-guessing posture as every other miss in this product.
pub(crate) fn manifest_entry_candidates(root: &str, package_json: &[u8]) -> Vec<String> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(package_json) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    if let Some(main) = value.get("main").and_then(|v| v.as_str()) {
        candidates.push(join_root(root, main));
    }
    if let Some(scripts) = value.get("scripts").and_then(|v| v.as_object()) {
        for script in scripts.values().filter_map(|v| v.as_str()) {
            if let Some(path) = node_invocation_target(script) {
                candidates.push(join_root(root, path));
            }
        }
    }
    candidates
}

/// `"node ./bin/www"` / `"nodemon src/server.js"` / `"node --inspect server.js"` → the path
/// argument, skipping leading flags. `None` for anything that isn't a direct node/nodemon
/// invocation (`"next dev"`, `"jest"`, …) — those aren't Express's own launch convention, and
/// guessing at other runners' argument conventions is exactly the over-reach this stays
/// conservative to avoid.
fn node_invocation_target(script: &str) -> Option<&str> {
    let mut parts = script.split_whitespace();
    let cmd = parts.next()?;
    if cmd != "node" && cmd != "nodemon" {
        return None;
    }
    parts.find(|p| !p.starts_with('-'))
}

fn join_root(root: &str, rel: &str) -> String {
    let rel = rel.strip_prefix("./").unwrap_or(rel);
    if root.is_empty() {
        rel.to_string()
    } else {
        format!("{root}/{rel}")
    }
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

    #[test]
    fn manifest_candidates_read_main_and_a_node_start_script() {
        let json = br#"{"main":"index.js","scripts":{"start":"node ./bin/www","test":"jest"}}"#;
        let candidates = manifest_entry_candidates("", json);
        assert_eq!(candidates, vec!["index.js", "bin/www"]);
    }

    #[test]
    fn manifest_candidates_resolve_against_a_nested_app_root() {
        let json = br#"{"scripts":{"start":"nodemon src/server.js"}}"#;
        let candidates = manifest_entry_candidates("services/api", json);
        assert_eq!(candidates, vec!["services/api/src/server.js"]);
    }

    #[test]
    fn manifest_candidates_ignore_non_node_scripts_and_flags() {
        let json = br#"{"scripts":{"dev":"next dev","start":"node --inspect server.js"}}"#;
        let candidates = manifest_entry_candidates("", json);
        assert_eq!(candidates, vec!["server.js"]);
    }

    #[test]
    fn manifest_candidates_degrade_to_empty_on_malformed_json() {
        assert!(manifest_entry_candidates("", b"not json").is_empty());
    }
}
