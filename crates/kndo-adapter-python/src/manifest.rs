//! Python's manifests, read as TOML and INI rather than scanned by line: what
//! `pyproject.toml`, `setup.cfg` and `requirements*.txt` STATE about the
//! project's structure.
//!
//! Two facts decide everything downstream. The first is the SOURCE ROOT, and
//! Python has no single answer: PEP 621 names the distribution but says
//! nothing about where its code lives, so the build backend does — setuptools
//! through `package-dir` and `packages.find.where`, flit through
//! `[tool.flit.module]`, poetry through `packages = [{from = …}]`, hatch
//! through its wheel target. Each is read where it speaks, and where none does
//! the answer is setuptools' own default: `src/` when the tree has one, the
//! manifest's directory otherwise.
//!
//! The second is the NAME a dependency is declared under. A specifier is PEP
//! 508 (`flask-sqlalchemy[async]>=3.0; python_version>'3.9'`) and the name is
//! everything before the first extras, comparison, marker or url character —
//! graded in `tests/tooling.rs` against `packaging.requirements.Requirement`,
//! because a reading of a PEP is not the same as what the ecosystem's own
//! parser answers.

use kndo_contract::adapter::{
    DependencyDeclaration, DependencyScope, PackageEntry, ResolveContext,
};
use kndo_contract::manifest::{ManifestSink, Publication, Unit, UnitKind, UnitRoot};
use kndo_contract::vocab::ProjectPath;
use smol_str::SmolStr;
use toml::Value;

/// The one entry point: which manifest this is decides how it is read.
pub fn structure(
    path: &ProjectPath,
    content: &[u8],
    cx: &ResolveContext<'_>,
    out: &mut ManifestSink,
) {
    let Ok(text) = std::str::from_utf8(content) else {
        return;
    };
    let p = path.as_str();
    let file = p.rsplit('/').next().unwrap_or(p);
    let dir = p.rsplit_once('/').map_or("", |(d, _)| d);
    match file {
        "pyproject.toml" => pyproject(text, dir, cx, out),
        "setup.cfg" => setup_cfg(text, dir, cx, out),
        _ => {
            // `requirements.txt` is a list of what to install, never a
            // distribution: dependencies and nothing else.
            for spec in requirements_lines(text) {
                declare(out, spec, None);
            }
        }
    }
}

// ------------------------------------------------------------------ pyproject

fn pyproject(text: &str, dir: &str, cx: &ResolveContext<'_>, out: &mut ManifestSink) {
    let Ok(root) = text.parse::<Value>() else {
        // A manifest that does not parse states nothing; the run is not the
        // place to argue with it.
        return;
    };
    let project = root.get("project");
    let poetry = root.get("tool").and_then(|t| t.get("poetry"));
    let name = project
        .and_then(|p| p.get("name"))
        .or_else(|| poetry.and_then(|p| p.get("name")))
        .and_then(Value::as_str);

    dependencies(&root, out);

    let Some(name) = name else {
        // No distribution here — a `pyproject.toml` that only configures tools
        // (`[tool.ruff]`) declares no unit and no package.
        test_unit(&root, dir, out);
        return;
    };

    let (roots, namespace_root) = source_roots(&root, dir, name, cx);
    out.package(PackageEntry {
        name: SmolStr::new(name),
        entry: None,
        dir: SmolStr::new(dir),
        aliases: distribution_aliases(name),
        subpaths: Vec::new(),
    });
    out.unit(Unit {
        name: SmolStr::new(name),
        kind: UnitKind::Library,
        entries: entries(&root, &roots, namespace_root.as_ref(), cx),
        roots: roots.into_iter().map(UnitRoot::from).collect(),
        excludes: Vec::new(),
        depends_on: Vec::new(),
        namespace_root,
        // A `[project]` table is a distribution: it exists to be built and
        // uploaded. The one thing that says otherwise is the classifier the
        // index itself refuses, and saying so is not the same as leaving it
        // unsaid — which is what a manifest without a `[project]` table does.
        publication: match (project.is_some(), private(&root)) {
            (_, true) => Publication::Unpublished,
            (true, false) => Publication::Published,
            (false, false) => Publication::Unstated,
        },
    });
    test_unit(&root, dir, out);
}

/// `Private :: Do Not Upload` — the trove classifier PyPI rejects an upload
/// for. PEP 621 has no other word for it, so this is the whole of the
/// exception.
fn private(root: &Value) -> bool {
    root.get("project")
        .and_then(|p| p.get("classifiers"))
        .and_then(Value::as_array)
        .is_some_and(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .any(|c| c.trim().eq_ignore_ascii_case("Private :: Do Not Upload"))
        })
}

/// Where this distribution's code lives, asked of each backend in turn and
/// answered by the first that speaks. Every root is relative to the manifest's
/// own directory, which is the only thing a backend's paths are ever relative
/// to.
fn source_roots(
    root: &Value,
    dir: &str,
    name: &str,
    cx: &ResolveContext<'_>,
) -> (Vec<SmolStr>, Option<SmolStr>) {
    let join = |rel: &str| -> SmolStr {
        let rel = rel.trim_matches('/');
        match (dir.is_empty(), rel.is_empty()) {
            (true, true) => SmolStr::new(""),
            (true, false) => SmolStr::new(rel),
            (false, true) => SmolStr::new(dir),
            (false, false) => SmolStr::new(format!("{dir}/{rel}")),
        }
    };
    let tool = |names: &[&str]| -> Option<&Value> {
        let mut cursor = root.get("tool")?;
        for n in names {
            cursor = cursor.get(n)?;
        }
        Some(cursor)
    };

    // setuptools: `package-dir` maps the ROOT package (`""`) to a directory,
    // and `packages.find.where` lists directories to search. Both are read
    // from the file — `read_configuration` does not run discovery, so there is
    // no richer answer to defer to (see `tests/captured/tooling.json`).
    if let Some((package, directory)) = tool(&["setuptools", "package-dir"])
        .and_then(Value::as_table)
        .and_then(|t| match t.get("") {
            Some(v) => Some(("", v)),
            None => t.iter().next().map(|(k, v)| (k.as_str(), v)),
        })
        .and_then(|(k, v)| Some((k, v.as_str()?)))
    {
        // A NAMED key maps one package to a directory that does not contain
        // it: `{"mypkg": "lib"}` makes `lib/mod.py` the module `mypkg.mod`,
        // and the name is nowhere in the path.
        let namespace = (!package.is_empty()).then(|| SmolStr::new(package));
        return (vec![join(directory)], namespace);
    }
    if let Some(list) = tool(&["setuptools", "packages", "find", "where"]).and_then(Value::as_array)
    {
        let roots: Vec<SmolStr> = list.iter().filter_map(Value::as_str).map(join).collect();
        if !roots.is_empty() {
            return (roots, None);
        }
    }
    // poetry: each entry names a package and optionally the directory holding
    // it; the ROOT is that directory, never the package inside it.
    if let Some(list) = tool(&["poetry", "packages"]).and_then(Value::as_array) {
        let mut roots: Vec<SmolStr> = list
            .iter()
            .map(|e| {
                e.get("from")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            })
            .map(|f| join(&f))
            .collect();
        roots.sort_unstable();
        roots.dedup();
        if !roots.is_empty() {
            return (roots, None);
        }
    }
    // hatch: `packages = ["src/demo"]` names the package WITH its directory,
    // so the root is the parent.
    if let Some(list) =
        tool(&["hatch", "build", "targets", "wheel", "packages"]).and_then(Value::as_array)
    {
        let mut roots: Vec<SmolStr> = list
            .iter()
            .filter_map(Value::as_str)
            .map(|p| join(p.rsplit_once('/').map_or("", |(parent, _)| parent)))
            .collect();
        roots.sort_unstable();
        roots.dedup();
        if !roots.is_empty() {
            return (roots, None);
        }
    }

    // Nothing said it, so the layout does, and setuptools' auto-discovery
    // states the rule plainly: a `src` directory beside the manifest MEANS a
    // src-layout — its mere existence, not a name that matches the
    // distribution's (`type-checking-cycle` ships `pkg`). flit asks the same
    // question of the same tree. Which makes this a QUESTION ABOUT THE TREE,
    // and why it reads `cx` instead of guessing.
    let _ = name;
    let src = join("src");
    let prefix = format!("{src}/");
    if cx
        .known_files()
        .any(|p| p.as_str().starts_with(&prefix) && p.as_str().ends_with(".py"))
    {
        return (vec![src], None);
    }
    (vec![join("")], None)
}

/// Every `pkg.mod:func` this manifest registers with the installer. Three
/// tables spell one thing: `[project.scripts]` and `[project.gui-scripts]` are
/// the console entry points, and `[project.entry-points.<group>]` is every
/// other group — `pytest11`, `console_scripts`, `flask.commands`. What
/// registers a callable is what CALLS it: the group decides who does the
/// calling, never whether anyone does.
fn callables(root: &Value) -> Vec<&str> {
    let project = root.get("project");
    let mut out: Vec<&str> = Vec::new();
    for name in ["scripts", "gui-scripts"] {
        if let Some(t) = project.and_then(|p| p.get(name)).and_then(Value::as_table) {
            out.extend(t.values().filter_map(Value::as_str));
        }
    }
    if let Some(groups) = project
        .and_then(|p| p.get("entry-points"))
        .and_then(Value::as_table)
    {
        for group in groups.values().filter_map(Value::as_table) {
            out.extend(group.values().filter_map(Value::as_str));
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// How this distribution is entered, which for Python is two things.
///
/// `[project.scripts]` and `[project.gui-scripts]` name a callable as
/// `pkg.mod:func`, and the module half is a file — a console script is an
/// entry point in the literal sense. The other is the PACKAGE DOOR: `import
/// pkg` executes `pkg/__init__.py` whatever that file declares, so a
/// re-export-only `__init__.py` under a source root is how the outside enters
/// this distribution and cannot be dead while the distribution ships. A plain
/// module is NOT this — `import dark` reaches `dark.py`'s own exports, which
/// the published surface already answers, and calling it an entry would tell
/// `untested` it is wiring. Which top-level names exist is a question about
/// the tree, which is why both halves read `cx`.
fn entries(
    root: &Value,
    roots: &[SmolStr],
    namespace_root: Option<&SmolStr>,
    cx: &ResolveContext<'_>,
) -> Vec<ProjectPath> {
    let mut out: Vec<ProjectPath> = Vec::new();
    for base in roots {
        let prefix = if base.is_empty() {
            String::new()
        } else {
            format!("{base}/")
        };
        for path in cx.known_files() {
            let Some(rest) = path.as_str().strip_prefix(prefix.as_str()) else {
                continue;
            };
            // `<root>/pkg/__init__.py` — the door the name `pkg` opens. Where
            // the manifest MAPPED a package onto the root itself, the root's
            // own initializer is that door: `package-dir = {"pkg" = "lib"}`
            // makes `lib/__init__.py` what `import pkg` executes.
            let door = match namespace_root {
                Some(_) => rest == "__init__.py",
                None => rest
                    .split_once('/')
                    .is_some_and(|(_, tail)| tail == "__init__.py"),
            };
            if door {
                out.push(path.clone());
            }
        }
    }
    for target in callables(root) {
        let module = target.split(':').next().unwrap_or(target).replace('.', "/");
        for base in roots {
            for shape in [format!("{module}.py"), format!("{module}/__init__.py")] {
                let path = if base.is_empty() {
                    shape
                } else {
                    format!("{base}/{shape}")
                };
                let candidate = ProjectPath::new(path);
                if cx.contains(&candidate) {
                    out.push(candidate);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `[tool.pytest.ini_options] testpaths` — pytest's own statement about which
/// directories hold its tests, which is a unit of kind Test and not a
/// convention about a name.
fn test_unit(root: &Value, dir: &str, out: &mut ManifestSink) {
    let Some(paths) = root
        .get("tool")
        .and_then(|t| t.get("pytest"))
        .and_then(|p| p.get("ini_options"))
        .and_then(|i| i.get("testpaths"))
    else {
        return;
    };
    let listed: Vec<&str> = match paths {
        Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
        Value::String(s) => s.split_whitespace().collect(),
        _ => Vec::new(),
    };
    let roots: Vec<SmolStr> = listed
        .iter()
        .map(|p| {
            if dir.is_empty() {
                SmolStr::new(p.trim_matches('/'))
            } else {
                SmolStr::new(format!("{dir}/{}", p.trim_matches('/')))
            }
        })
        .collect();
    if roots.is_empty() {
        return;
    }
    out.unit(Unit {
        name: SmolStr::new("pytest"),
        kind: UnitKind::Test,
        roots: roots.into_iter().map(UnitRoot::from).collect(),
        excludes: Vec::new(),
        entries: Vec::new(),
        depends_on: Vec::new(),
        publication: Publication::Unpublished,
        // A test root is a directory pytest walks, not a package: the modules
        // under it carry their own dotted paths and hang under nothing.
        namespace_root: None,
    });
}

/// Every table that declares a requirement, each under the scope its table
/// means: `[project] dependencies` is what an install of this distribution
/// pulls in, and an extra, a PEP 735 group or a poetry group is not.
fn dependencies(root: &Value, out: &mut ManifestSink) {
    let mut runtime: Vec<&str> = Vec::new();
    let mut dev: Vec<&str> = Vec::new();
    fn strings(v: Option<&Value>) -> Vec<&str> {
        v.and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    }
    if let Some(project) = root.get("project") {
        runtime.extend(strings(project.get("dependencies")));
        if let Some(extras) = project
            .get("optional-dependencies")
            .and_then(Value::as_table)
        {
            for list in extras.values() {
                dev.extend(strings(Some(list)));
            }
        }
    }
    // PEP 735: a dependency group is for developing this project, never for
    // installing it — the distinction `[project.optional-dependencies]` blurs
    // and this table was added to make.
    if let Some(groups) = root.get("dependency-groups").and_then(Value::as_table) {
        for list in groups.values() {
            dev.extend(strings(Some(list)));
        }
    }
    // poetry states its requirements as a TABLE keyed by name, and `python` is
    // the interpreter rather than a package.
    if let Some(poetry) = root.get("tool").and_then(|t| t.get("poetry")) {
        if let Some(t) = poetry.get("dependencies").and_then(Value::as_table) {
            runtime.extend(t.keys().map(String::as_str).filter(|k| *k != "python"));
        }
        if let Some(groups) = poetry.get("group").and_then(Value::as_table) {
            for group in groups.values() {
                if let Some(t) = group.get("dependencies").and_then(Value::as_table) {
                    dev.extend(t.keys().map(String::as_str).filter(|k| *k != "python"));
                }
            }
        }
    }
    for spec in runtime {
        declare(out, spec, None);
    }
    for spec in dev {
        declare(out, spec, Some(DependencyScope::Dev));
    }
}

// ------------------------------------------------------------------ setup.cfg

/// setuptools' INI manifest: `[metadata] name`, `[options] package_dir` and
/// `install_requires`, `[options.extras_require]`. Values may be inline or
/// indented continuations, which is the whole of the INI shape that matters
/// here.
fn setup_cfg(text: &str, dir: &str, cx: &ResolveContext<'_>, out: &mut ManifestSink) {
    let cfg = ini(text);
    let get = |section: &str, key: &str| -> Option<String> {
        cfg.iter()
            .find(|(s, k, _)| s == section && k == key)
            .map(|(_, _, v)| v.clone())
    };
    for (section, _, value) in &cfg {
        let scope = match section.as_str() {
            "options" => None,
            s if s.starts_with("options.extras_require") => Some(DependencyScope::Dev),
            _ => continue,
        };
        let is_requires = cfg.iter().any(|(s, k, v)| {
            s == section && v == value && (k == "install_requires" || scope.is_some())
        });
        if !is_requires {
            continue;
        }
        for spec in value.lines().map(str::trim).filter(|l| !l.is_empty()) {
            declare(out, spec, scope);
        }
    }
    let Some(name) = get("metadata", "name") else {
        return;
    };
    // `package_dir` is `= src` (the root package) or `pkg = dir`; either way
    // the ROOT is the directory on the right of the first entry.
    let root = get("options", "package_dir")
        .and_then(|v| {
            v.lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .and_then(|l| l.split_once('='))
                .map(|(_, d)| d.trim().to_string())
        })
        .or_else(|| get("options.packages.find", "where").map(|w| w.trim().to_string()));
    let base = match (dir.is_empty(), root.as_deref().unwrap_or("")) {
        (true, "") => SmolStr::new(""),
        (true, r) => SmolStr::new(r),
        (false, "") => SmolStr::new(dir),
        (false, r) => SmolStr::new(format!("{dir}/{r}")),
    };
    let _ = cx;
    out.package(PackageEntry {
        name: SmolStr::new(&name),
        entry: None,
        dir: SmolStr::new(dir),
        aliases: distribution_aliases(&name),
        subpaths: Vec::new(),
    });
    out.unit(Unit {
        name: SmolStr::new(&name),
        kind: UnitKind::Library,
        roots: vec![UnitRoot::from(base)],
        excludes: Vec::new(),
        entries: Vec::new(),
        depends_on: Vec::new(),
        publication: Publication::Published,
        // Every layout `setup.cfg` states puts the package UNDER the root it
        // names, so the dotted path already carries the package's own name.
        namespace_root: None,
    });
}

/// `(section, key, value)` in file order, continuation lines folded into the
/// value they belong to.
fn ini(text: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    let mut section = String::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim_start().starts_with('#') || trimmed.trim_start().starts_with(';') {
            continue;
        }
        let bare = trimmed.trim();
        if bare.starts_with('[') && bare.ends_with(']') {
            section = bare[1..bare.len() - 1].to_string();
            continue;
        }
        let indented = trimmed.starts_with(' ') || trimmed.starts_with('\t');
        if indented && !bare.is_empty() {
            if let Some(last) = out.last_mut() {
                last.2.push('\n');
                last.2.push_str(bare);
            }
            continue;
        }
        if let Some((k, v)) = bare.split_once('=') {
            out.push((section.clone(), k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

// ------------------------------------------------------------- requirements

/// Requirement lines: comments, blanks and options (`-r other.txt`,
/// `--index-url`) are not requirements.
fn requirements_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('-'))
        .collect()
}

// ---------------------------------------------------------------- PEP 508/503

/// The distribution name a PEP 508 specifier declares: everything before the
/// first extras bracket, comparison, marker or url. Graded against
/// `packaging.requirements.Requirement` in `tests/tooling.rs`.
pub fn dependency_name(spec: &str) -> Option<&str> {
    let spec = spec.trim();
    let end = spec
        .find(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_' || c == '.'))
        .unwrap_or(spec.len());
    let name = spec[..end].trim();
    (!name.is_empty()).then_some(name)
}

/// PEP 503: runs of `-`, `_` and `.` collapse to one `-`, lowercased. The
/// spelling under which two names are the SAME name — how an import's
/// top-level module is compared with a declared distribution.
pub fn canonicalize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut pending = false;
    for c in name.chars() {
        if c == '-' || c == '_' || c == '.' {
            pending = !out.is_empty();
        } else {
            if pending {
                out.push('-');
                pending = false;
            }
            out.extend(c.to_lowercase());
        }
    }
    out
}

fn declare(out: &mut ManifestSink, spec: &str, scope: Option<DependencyScope>) {
    if let Some(name) = dependency_name(spec) {
        out.dependency(DependencyDeclaration {
            name: SmolStr::new(name),
            scope,
            version_req: None,
        });
    }
}

/// The other spellings one distribution name answers to. PEP 503 normalizes a
/// name by lowercasing it and folding every run of `-`, `_` and `.` into one
/// `-`; PyPI matches on that, so a requirement spelled `Flask_SQLAlchemy` and a
/// distribution named `flask-sqlalchemy` are the same package. The normal form
/// plus the underscore spelling an import uses — never the name itself, which
/// the entry already carries.
fn distribution_aliases(name: &str) -> Vec<SmolStr> {
    let normalized: String = {
        let mut out = String::with_capacity(name.len());
        for c in name.chars() {
            match c {
                '-' | '_' | '.' => {
                    if !out.ends_with('-') {
                        out.push('-');
                    }
                }
                c => out.extend(c.to_lowercase()),
            }
        }
        out
    };
    let mut aliases = vec![
        SmolStr::new(&normalized),
        SmolStr::new(normalized.replace('-', "_")),
    ];
    aliases.retain(|a| a.as_str() != name);
    aliases.sort_unstable();
    aliases.dedup();
    aliases
}
