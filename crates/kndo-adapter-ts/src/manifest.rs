//! What a JavaScript project's manifests STATE, through the one door.
//!
//! `package.json` states a unit — the npm package, entered through its entry
//! fields (`main`, `module`, `browser`, `bin`, every string leaf under
//! `exports` and under `imports`, the `#alias` map included), published unless
//! it says `"private": true`, compiled against the dependencies it declares.
//! Source files named in `scripts` are run by a tool rather than entered by
//! the package, so they are the manifest's own roots at the tooling colour.
//!
//! `tsconfig.json` states the names that are not packages: a
//! `compilerOptions.paths` alias is a NAME resolving to a file, which is what
//! a package entry is, so an alias whose target exists in the project is
//! emitted as one. Nothing else in a tsconfig is read — `references`,
//! `include`/`exclude` and `extends` are measured in `DECISIONS.md` and cost
//! nothing on the corpus.
//!
//! The launchers GitHub Actions runs state roots and nothing else: a
//! workflow's or composite action's `run:` steps hand files to runtimes
//! exactly as npm scripts do, and a JavaScript action's `main`/`pre`/`post`
//! are its entries.
//!
//! Everything that fails — unparseable JSON, an entry naming a file that is
//! not in the project (a built `dist/`) — degrades to absence: structure
//! nobody stated is structure the engine does not assume.

use crate::resolve::resolve_in_dir;
use kndo_contract::adapter::{
    DependencyDeclaration, DependencyScope, PackageEntry, ProjectRoot, ResolveContext, SourceFile,
};
use kndo_contract::evidence::RootKind;
use kndo_contract::manifest::{ManifestSink, Publication, Unit, UnitKind};
use kndo_contract::vocab::{Confidence, ProjectPath};
use kndo_toolkit::github_actions::{self, Launcher};
use smol_str::SmolStr;
use std::collections::BTreeSet;

/// Everything one manifest states, written to the sink that collects it. The
/// dispatch is by filename because that is what the spec's globs matched.
pub fn structure(
    manifest: &SourceFile<'_>,
    cx: &ResolveContext<'_>,
    exts: &[String],
    out: &mut ManifestSink,
) {
    let path = manifest.path.as_str();
    if github_actions::launcher(path).is_some() {
        for root in roots(manifest, cx, exts) {
            out.root(root);
        }
        return;
    }
    let name = path.rsplit_once('/').map_or(path, |(_, f)| f);
    if name.starts_with("tsconfig") {
        tsconfig(manifest, cx, exts, out);
        return;
    }
    package(manifest, cx, exts, out);
}

/// What `package.json` states: the unit npm compiles, the package name a bare
/// specifier reaches it by, the dependencies it declares, the words it spells
/// elsewhere, and the files its scripts run.
fn package(
    manifest: &SourceFile<'_>,
    cx: &ResolveContext<'_>,
    exts: &[String],
    out: &mut ManifestSink,
) {
    let Some(json) = package_json(manifest) else {
        return;
    };
    let dir = kndo_toolkit::parent_dir(manifest.path.as_str());
    let entries: Vec<ProjectPath> = entry_roots(&json, dir, cx, exts).into_iter().collect();
    let declared = json.get("name").and_then(|v| v.as_str());
    let named = declared
        .or_else(|| dir.rsplit('/').next().filter(|s| !s.is_empty()))
        .unwrap_or("root");
    out.unit(Unit {
        name: SmolStr::new(named),
        // A package with an entry field is imported; one without is run. Both
        // enter at production colour — the difference is whether the outside
        // world consumes its API, which `Publication` then narrows.
        kind: if entries.is_empty() {
            UnitKind::Executable
        } else {
            UnitKind::Library
        },
        // The manifest's own directory, minus nothing: what npm packs is a
        // publish filter, never a compile one, and a nested workspace member
        // takes its own files by the longest-prefix rule.
        roots: Vec::new(),
        excludes: Vec::new(),
        entries,
        depends_on: dependency_declarations(&json)
            .iter()
            .map(|d| d.name.clone())
            .collect(),
        friend_of: Vec::new(),
        // `"private": true` is npm's own word for "no consumer outside".
        publication: match json.get("private").and_then(serde_json::Value::as_bool) {
            Some(true) => Publication::Unpublished,
            _ => Publication::Unstated,
        },
    });
    for package in packages(manifest, cx, exts) {
        out.package(package);
    }
    for declaration in dependency_declarations(&json) {
        out.dependency(declaration);
    }
    for word in mentions(manifest) {
        out.mention(word);
    }
    for root in script_roots(&json, dir, cx, exts) {
        out.root(root);
    }
}

/// What `tsconfig.json` states that the engine can use: every
/// `compilerOptions.paths` alias whose target is a file of this project.
///
/// An alias is a NAME that resolves to a FILE, which is exactly a package
/// entry, so it travels as one — `~utils` is spelled and resolved like a
/// package with no `node_modules` behind it. A wildcard alias (`"@/*":
/// ["./src/*"]`) names a DIRECTORY instead, and rides the same entry with its
/// subpath resolved against that directory. Targets are dir-relative, or
/// `baseUrl`-relative where the config sets one; an alias whose target is not
/// in the project (`react` mapped into `node_modules/`) is absent, like every
/// other dangling entry here.
fn tsconfig(
    manifest: &SourceFile<'_>,
    cx: &ResolveContext<'_>,
    exts: &[String],
    out: &mut ManifestSink,
) {
    let Some(json) = package_json(manifest) else {
        return;
    };
    let dir = kndo_toolkit::parent_dir(manifest.path.as_str());
    let options = json.get("compilerOptions");
    let base = options
        .and_then(|o| o.get("baseUrl"))
        .and_then(|v| v.as_str())
        .and_then(|b| kndo_toolkit::join_relative(dir, b))
        .unwrap_or_else(|| dir.to_string());
    let Some(serde_json::Value::Object(paths)) = options.and_then(|o| o.get("paths")) else {
        return;
    };
    for (alias, targets) in paths {
        // The first target that lands is the alias: TypeScript tries them in
        // order and takes the first that exists.
        let Some(target) = targets
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_str())
            .next()
        else {
            continue;
        };
        match (alias.strip_suffix("/*"), target.strip_suffix("/*")) {
            (Some(name), Some(under)) => {
                let Some(under) = kndo_toolkit::join_relative(&base, under) else {
                    continue;
                };
                // A directory nothing sits under names nothing.
                if cx.files_with_prefix(&format!("{under}/")).next().is_none() {
                    continue;
                }
                out.package(PackageEntry {
                    name: SmolStr::new(name),
                    entry: None,
                    dir: SmolStr::new(under),
                });
            }
            (None, None) => {
                if let Some(entry) = resolve_in_dir(&base, target, cx, exts) {
                    out.package(PackageEntry {
                        name: SmolStr::new(alias),
                        entry: Some(entry),
                        dir: SmolStr::new(&base),
                    });
                }
            }
            // A wildcard on one side alone is not a mapping TypeScript accepts.
            _ => {}
        }
    }
}

/// Commands whose first non-flag argument is a source file they run.
const RUNTIMES: &[&str] = &["node", "tsx", "ts-node", "bun", "deno"];

/// The parsed package manifest; the launchers this adapter declares reach
/// `roots` only, by contract, and never arrive here.
fn package_json(manifest: &SourceFile<'_>) -> Option<serde_json::Value> {
    serde_json::from_slice(manifest.content).ok()
}

/// A launcher's roots: the only place a workflow or action reaches.
fn roots(manifest: &SourceFile<'_>, cx: &ResolveContext<'_>, exts: &[String]) -> Vec<ProjectRoot> {
    match github_actions::launcher(manifest.path.as_str()) {
        Some(launcher) => launcher_roots(manifest, launcher, cx, exts),
        None => Vec::new(),
    }
}

/// The files a package's entry fields name — its unit's entries, which the
/// engine anchors at the unit's colour.
fn entry_roots(
    json: &serde_json::Value,
    dir: &str,
    cx: &ResolveContext<'_>,
    exts: &[String],
) -> BTreeSet<ProjectPath> {
    let mut anchored: BTreeSet<ProjectPath> = BTreeSet::new();
    for entry in &entry_fields(json) {
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
    anchored
}

/// Source files named in npm scripts are run by their tool, not imported —
/// the manifest's own roots, at the tooling colour and a habit's confidence.
/// A file that is already an entry is the unit's, not a script's.
fn script_roots(
    json: &serde_json::Value,
    dir: &str,
    cx: &ResolveContext<'_>,
    exts: &[String],
) -> Vec<ProjectRoot> {
    let mut script_files: BTreeSet<ProjectPath> = BTreeSet::new();
    if let Some(serde_json::Value::Object(scripts)) = json.get("scripts") {
        for value in scripts.values() {
            if let Some(command) = value.as_str() {
                launched_files(command, dir, cx, exts, &mut script_files);
            }
        }
    }
    let entries = entry_roots(json, dir, cx, exts);
    script_files
        .into_iter()
        .filter(|f| !entries.contains(f))
        .map(|file| ProjectRoot {
            file,
            kind: RootKind::Tooling,
            confidence: Confidence::Probable,
        })
        .collect()
}

/// The project files a shell command runs, resolved from `dir`. The word a
/// runtime is handed is an entry however it is spelled (`node server`,
/// `node --inspect-brk server`); anywhere else only a path-shaped token or a
/// source suffix names a file — a bare word (`eslint src`) does not.
fn launched_files(
    command: &str,
    dir: &str,
    cx: &ResolveContext<'_>,
    exts: &[String],
    out: &mut BTreeSet<ProjectPath>,
) {
    let mut launched = false;
    for token in command.split(|c: char| c.is_whitespace() || c == ';' || c == '&') {
        let token = token.trim_matches(|c| c == '"' || c == '\'');
        if token.is_empty() {
            continue;
        }
        if RUNTIMES.contains(&token) {
            launched = true;
            continue;
        }
        if token.starts_with('-') {
            continue;
        }
        let file_shaped = token.contains('/') || exts.iter().any(|e| token.ends_with(e.as_str()));
        if (launched || file_shaped)
            && let Some(path) = resolve_in_dir(dir, token, cx, exts)
        {
            out.insert(path);
        }
        launched = false;
    }
}

/// The roots a workflow or action declares: a JavaScript action's entries are
/// what GitHub runs (`Production`, `Certain`, through the same built-to-source
/// mapping as a package entry); every file a `run:` step hands a runtime has
/// an npm script's standing (`Tooling`, `Probable`).
fn launcher_roots(
    manifest: &SourceFile<'_>,
    launcher: Launcher,
    cx: &ResolveContext<'_>,
    exts: &[String],
) -> Vec<ProjectRoot> {
    let path = manifest.path.as_str();
    let Ok(text) = std::str::from_utf8(manifest.content) else {
        return Vec::new();
    };
    let mut entries: BTreeSet<ProjectPath> = BTreeSet::new();
    if let Launcher::Action { dir } = &launcher {
        for entry in github_actions::action_entries(path, text) {
            if let Some(found) = resolve_entry(dir, &entry, cx, exts) {
                entries.insert(found);
            }
        }
    }
    let mut launched: BTreeSet<ProjectPath> = BTreeSet::new();
    for step in github_actions::run_steps(path, text) {
        launched_files(&step.command, &step.dir, cx, exts, &mut launched);
    }
    let mut out: Vec<ProjectRoot> = entries
        .iter()
        .map(|file| ProjectRoot {
            file: file.clone(),
            kind: RootKind::Production,
            confidence: Confidence::Certain,
        })
        .collect();
    out.extend(launched.difference(&entries).map(|file| ProjectRoot {
        file: file.clone(),
        kind: RootKind::Tooling,
        confidence: Confidence::Probable,
    }));
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
fn packages(
    manifest: &SourceFile<'_>,
    cx: &ResolveContext<'_>,
    exts: &[String],
) -> Vec<PackageEntry> {
    let Some(json) = package_json(manifest) else {
        return Vec::new();
    };
    let Some(name) = json.get("name").and_then(|v| v.as_str()) else {
        return Vec::new();
    };
    let dir = kndo_toolkit::parent_dir(manifest.path.as_str());
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
fn dependency_declarations(json: &serde_json::Value) -> Vec<DependencyDeclaration> {
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
fn mentions(manifest: &SourceFile<'_>) -> Vec<SmolStr> {
    let Some(json) = package_json(manifest) else {
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
