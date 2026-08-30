//! Roots and packages from `package.json`: the entry fields (`main`, `module`,
//! `browser`, `bin`, every string leaf under `exports` and under `imports` — the
//! `#alias` map — plus source files named in `scripts`), resolved dir-relative
//! through the same candidate machinery imports use. Everything that fails —
//! unparseable JSON, an entry naming a file that is not in the project (a built
//! `dist/`) — degrades to absence: a root that anchors nothing accuses nothing.

use crate::resolve::{parent_dir, resolve_in_dir};
use kndo_contract::adapter::{PackageEntry, ProjectRoot, ResolveContext, SourceFile};
use kndo_contract::evidence::RootKind;
use kndo_contract::vocab::{Confidence, ProjectPath};
use smol_str::SmolStr;
use std::collections::BTreeSet;

pub fn roots(manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<ProjectRoot> {
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(manifest.content) else {
        return Vec::new();
    };
    let dir = parent_dir(manifest.path);

    let mut anchored: BTreeSet<ProjectPath> = BTreeSet::new();
    for entry in &entry_fields(&json) {
        match entry.split_once('*') {
            // A wildcard entry (`"./types/*"`) declares every file it expands to.
            Some((before, after)) => {
                let stem = before.trim_start_matches("./");
                let prefix = if dir.is_empty() {
                    stem.to_string()
                } else {
                    format!("{dir}/{stem}")
                };
                // Exports wildcards conventionally substitute the extension too
                // (`./types/*` matches `types/foo.d.ts`), so a non-matching suffix
                // still anchors when the wildcard covered it.
                anchored.extend(
                    cx.files_with_prefix(&prefix)
                        .filter(|p| {
                            p.as_str().ends_with(after)
                                || SOURCE_EXTS.iter().any(|e| p.as_str().ends_with(e))
                                || p.as_str().ends_with(".d.ts")
                        })
                        .cloned(),
                );
            }
            None => {
                if let Some(path) = resolve_in_dir(dir, entry, cx) {
                    // A JS entry's `.d.ts` companion is published beside it.
                    for ext in [".js", ".mjs", ".cjs"] {
                        if let Some(stem) = path.as_str().strip_suffix(ext) {
                            let dts = ProjectPath::new(format!("{stem}.d.ts"));
                            if cx.contains(&dts) {
                                anchored.insert(dts);
                            }
                        }
                    }
                    anchored.insert(path);
                }
            }
        }
    }
    let mut out: Vec<ProjectRoot> = anchored
        .into_iter()
        .map(|file| ProjectRoot {
            file,
            kind: RootKind::Production,
            confidence: Confidence::Certain,
        })
        .collect();

    // Source files named in npm scripts are run by their tool, not imported.
    let mut script_files: BTreeSet<ProjectPath> = BTreeSet::new();
    if let Some(serde_json::Value::Object(scripts)) = json.get("scripts") {
        for value in scripts.values() {
            let Some(command) = value.as_str() else {
                continue;
            };
            for token in command.split(|c: char| c.is_whitespace() || c == ';' || c == '&') {
                let token = token.trim_matches(|c| c == '"' || c == '\'');
                if SOURCE_EXTS.iter().any(|e| token.ends_with(e))
                    && let Some(path) = resolve_in_dir(dir, token, cx)
                {
                    script_files.insert(path);
                }
            }
        }
    }
    let production: BTreeSet<&ProjectPath> = out.iter().map(|r| &r.file).collect();
    let script_roots: Vec<ProjectRoot> = script_files
        .iter()
        .filter(|f| !production.contains(f))
        .map(|file| ProjectRoot {
            file: file.clone(),
            kind: RootKind::Tooling,
            confidence: Confidence::Probable,
        })
        .collect();
    out.extend(script_roots);
    out
}

const SOURCE_EXTS: [&str; 6] = [".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"];

/// Every entry-declaring string in the manifest: `main`/`module`/`browser`, `bin`
/// values, and the string leaves of `exports` and `imports` (the internal `#alias`
/// map — its targets are entries of this package all the same).
fn entry_fields(json: &serde_json::Value) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    for key in ["main", "module", "browser"] {
        if let Some(s) = json.get(key).and_then(|v| v.as_str()) {
            entries.push(s.to_string());
        }
    }
    match json.get("bin") {
        Some(serde_json::Value::String(s)) => entries.push(s.clone()),
        Some(serde_json::Value::Object(m)) => {
            entries.extend(m.values().filter_map(|v| v.as_str().map(str::to_string)));
        }
        _ => {}
    }
    for key in ["exports", "imports"] {
        if let Some(v) = json.get(key) {
            export_leaves(v, &mut entries);
        }
    }
    entries
}

/// The package this manifest declares, when it has a name and an entry that
/// resolves — what links a workspace-internal bare import to its source.
pub fn packages(manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<PackageEntry> {
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(manifest.content) else {
        return Vec::new();
    };
    let Some(name) = json.get("name").and_then(|v| v.as_str()) else {
        return Vec::new();
    };
    let dir = parent_dir(manifest.path);
    let entry = entry_fields(&json)
        .iter()
        .find_map(|e| resolve_in_dir(dir, e, cx));
    match entry {
        Some(entry) => vec![PackageEntry {
            name: SmolStr::new(name),
            entry,
            dir: SmolStr::new(dir),
        }],
        None => Vec::new(),
    }
}

/// Every string leaf of the `exports` value — plain, per-subpath, or per-condition
/// nesting alike. Non-path leaves simply fail to resolve later.
fn export_leaves(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Object(m) => {
            for v in m.values() {
                export_leaves(v, out);
            }
        }
        serde_json::Value::Array(a) => {
            for v in a {
                export_leaves(v, out);
            }
        }
        _ => {}
    }
}
