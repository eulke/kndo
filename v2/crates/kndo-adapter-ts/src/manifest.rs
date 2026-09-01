//! Roots and packages from `package.json`: the entry fields (`main`, `module`,
//! `browser`, `bin`, every string leaf under `exports` and under `imports` — the
//! `#alias` map — plus source files named in `scripts`), resolved dir-relative
//! through the same candidate machinery imports use. Everything that fails —
//! unparseable JSON, an entry naming a file that is not in the project (a built
//! `dist/`) — degrades to absence: a root that anchors nothing accuses nothing.

use crate::resolve::{parent_dir, resolve_in_dir};
use kndo_contract::adapter::{
    DependencyDeclaration, DependencyScope, PackageEntry, ProjectRoot, ResolveContext, SourceFile,
};
use kndo_contract::evidence::RootKind;
use kndo_contract::vocab::{Confidence, ProjectPath};
use smol_str::SmolStr;
use std::collections::BTreeSet;

pub fn roots(
    manifest: &SourceFile<'_>,
    cx: &ResolveContext<'_>,
    exts: &[String],
) -> Vec<ProjectRoot> {
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
                                || exts.iter().any(|e| p.as_str().ends_with(e.as_str()))
                        })
                        .cloned(),
                );
            }
            None => {
                if let Some(path) = resolve_entry(dir, entry, cx, exts) {
                    // A JS entry's type-declaration companion is published beside it.
                    for ext in [".js", ".mjs", ".cjs"] {
                        if let Some(stem) = path.as_str().strip_suffix(ext) {
                            let dts =
                                ProjectPath::new(format!("{stem}.{}", crate::TYPE_DECLARATION_EXT));
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
                // A path-shaped token (`node lib/main/build-site`) or a source
                // suffix; a bare word (`eslint src`) names no file to root.
                if (token.contains('/') || exts.iter().any(|e| token.ends_with(e.as_str())))
                    && let Some(path) = resolve_in_dir(dir, token, cx, exts)
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

/// An entry that names a BUILT file names its source when the built tree is
/// absent: the same path with its first segment under `src/`, through the same
/// candidate machinery (the compiled-extension swap included). Only when the
/// literal entry resolves nowhere — an existing `dist/` wins untouched, and a
/// mapping that lands on nothing stays absent.
fn resolve_entry(
    dir: &str,
    entry: &str,
    cx: &ResolveContext<'_>,
    exts: &[String],
) -> Option<ProjectPath> {
    if let Some(found) = resolve_in_dir(dir, entry, cx, exts) {
        return Some(found);
    }
    let trimmed = entry.trim_start_matches("./");
    let (_built, rest) = trimmed.split_once('/')?;
    resolve_in_dir(dir, &format!("src/{rest}"), cx, exts)
}

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
pub fn packages(
    manifest: &SourceFile<'_>,
    cx: &ResolveContext<'_>,
    exts: &[String],
) -> Vec<PackageEntry> {
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(manifest.content) else {
        return Vec::new();
    };
    let Some(name) = json.get("name").and_then(|v| v.as_str()) else {
        return Vec::new();
    };
    let dir = parent_dir(manifest.path);
    let entry = entry_fields(&json)
        .iter()
        .find_map(|e| resolve_entry(dir, e, cx, exts));
    match entry {
        Some(entry) => vec![PackageEntry {
            name: SmolStr::new(name),
            entry: Some(entry),
            dir: SmolStr::new(dir),
        }],
        None => Vec::new(),
    }
}

/// The dependency names this manifest declares, every section npm installs from —
/// activation evidence for plugin `ManifestDependency` rules, never resolution.
pub fn dependencies(manifest: &SourceFile<'_>) -> Vec<DependencyDeclaration> {
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(manifest.content) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (section, scope) in SECTIONS {
        if let Some(serde_json::Value::Object(map)) = json.get(section) {
            for (name, req) in map {
                out.push(DependencyDeclaration {
                    name: SmolStr::new(name),
                    scope: Some(scope),
                    version_req: comparable_req(req.as_str().unwrap_or_default()),
                });
            }
        }
    }
    out
}

/// The words this manifest spells outside its dependency sections and its
/// prose, sorted — see [`mentioned_words`].
pub fn mentions(manifest: &SourceFile<'_>) -> Vec<SmolStr> {
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(manifest.content) else {
        return Vec::new();
    };
    mentioned_words(&json)
        .into_iter()
        .map(SmolStr::new)
        .collect()
}

/// Every section npm installs from, with the scope it declares.
const SECTIONS: [(&str, DependencyScope); 4] = [
    ("dependencies", DependencyScope::Prod),
    ("devDependencies", DependencyScope::Dev),
    ("peerDependencies", DependencyScope::Peer),
    ("optionalDependencies", DependencyScope::Optional),
];

/// Fields that describe the package rather than use anything: a dependency
/// named in prose is not in use.
const PROSE: [&str; 3] = ["name", "description", "keywords"];

/// The words this manifest spells outside its dependency sections and its
/// prose, split on everything a package name cannot contain: `"lint": "eslint .
/// && tsc"` names `eslint` — through its binary, without any import — a
/// `browser` map names the package it aliases to, a tool config names its
/// plugins. A path into a package names the package too
/// (`vite/bin/vite.js`, `./node_modules/vite/bin/vite.js`), and so does a
/// versioned invocation (`npx marky-markdown@^9`).
fn mentioned_words(json: &serde_json::Value) -> BTreeSet<&str> {
    let mut words = BTreeSet::new();
    if let serde_json::Value::Object(fields) = json {
        for (key, value) in fields {
            let excluded =
                SECTIONS.iter().any(|(section, _)| section == key) || PROSE.contains(&key.as_str());
            if !excluded {
                words_of_value(value, &mut words);
            }
        }
    }
    words
}

fn words_of_value<'a>(value: &'a serde_json::Value, words: &mut BTreeSet<&'a str>) {
    match value {
        serde_json::Value::String(text) => words_of(text, words),
        serde_json::Value::Array(items) => items.iter().for_each(|v| words_of_value(v, words)),
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                words_of(key, words);
                words_of_value(value, words);
            }
        }
        _ => {}
    }
}

fn words_of<'a>(text: &'a str, words: &mut BTreeSet<&'a str>) {
    for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || "@/._-".contains(c))) {
        if word.is_empty() {
            continue;
        }
        words.insert(word);
        if let Some(package) = path_package(word) {
            words.insert(package);
        }
        if let Some(package) = versioned_package(word) {
            words.insert(package);
        }
    }
}

/// The package a `name@version` word invokes: `marky-markdown@^9` → the name,
/// `@scope/name@1.2.3` → the scoped name; a word with no version yields nothing.
fn versioned_package(word: &str) -> Option<&str> {
    let body = word.strip_prefix('@').unwrap_or(word);
    let at = body.find('@')?;
    let end = word.len() - body.len() + at;
    (end > 0).then(|| &word[..end])
}

/// The package a path-shaped word reaches into: `vite/bin/vite.js` and
/// `./node_modules/vite/bin/vite.js` → `vite`, `@scope/name/cli` → `@scope/name`;
/// a bare name has no path, and a relative path names no package.
fn path_package(word: &str) -> Option<&str> {
    const INSTALL_DIR: &str = "node_modules/";
    let word = match word.rfind(INSTALL_DIR) {
        Some(at) => &word[at + INSTALL_DIR.len()..],
        None => word,
    };
    if word.starts_with('.') {
        return None;
    }
    let mut parts = word.splitn(3, '/');
    let first = parts.next()?;
    let second = parts.next()?;
    if first.starts_with('@') {
        parts.next()?;
        Some(&word[..first.len() + 1 + second.len()])
    } else {
        Some(first)
    }
}

/// A requirement worth comparing across manifests. Protocol and wildcard forms
/// (`workspace:*`, `file:…`, `link:…`, git/url refs, `*`) name a RESOLUTION
/// mechanism, not a version — encoding them as text would diverge from every
/// real requirement and draw false skew wherever manifests otherwise agree.
fn comparable_req(req: &str) -> Option<SmolStr> {
    let non_version = req.is_empty()
        || req == "*"
        || [
            "workspace:",
            "file:",
            "link:",
            "portal:",
            "npm:",
            "git",
            "http",
        ]
        .iter()
        .any(|p| req.starts_with(p));
    (!non_version).then(|| SmolStr::new(req))
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
