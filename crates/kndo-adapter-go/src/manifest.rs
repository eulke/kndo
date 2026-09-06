//! What one `go.mod` teaches the engine: the module is ONE unit — Go compiles a
//! module's packages together, and there is no per-target manifest section to
//! split it — declared as a library, because a module path is what any other
//! module imports. The same line becomes an entry-less package: a Go module maps
//! import prefixes to directories, so no file a bare import resolves to exists.
//! Entries need no manifest here: `package main` + `func main` and `_test.go`
//! are extraction's to see, and a module has no single file the build enters.

use kndo_contract::adapter::{
    DependencyDeclaration, DependencyScope, PackageEntry, ResolveContext, SourceFile,
};
use kndo_contract::manifest::{ManifestSink, Publication, Unit, UnitKind};
use smol_str::SmolStr;

pub fn structure(manifest: &SourceFile<'_>, _cx: &ResolveContext<'_>, out: &mut ManifestSink) {
    let Ok(text) = std::str::from_utf8(manifest.content) else {
        return;
    };
    let mut depends_on: Vec<SmolStr> = Vec::new();
    for declaration in dependencies_of(text) {
        depends_on.push(declaration.name.clone());
        out.dependency(declaration);
    }
    depends_on.sort();
    depends_on.dedup();
    // A `tool` line names a package whose module the requirements already
    // carry: `go get -tool` writes both, and the tool is used with no import.
    for tool in directive_values(text, "tool") {
        out.mention(tool);
    }
    let Some(module) = module_path(text) else {
        return;
    };
    out.package(PackageEntry {
        name: SmolStr::new(&module),
        entry: None,
        dir: SmolStr::new(kndo_toolkit::parent_dir(manifest.path.as_str())),
    });
    out.unit(Unit {
        name: SmolStr::new(&module),
        kind: UnitKind::Library,
        // Empty: the engine reads the manifest's own directory, which is every
        // package of the module and nothing above it.
        roots: Vec::new(),
        excludes: Vec::new(),
        entries: Vec::new(),
        depends_on,
        friend_of: Vec::new(),
        // Go has no `publish = false`: a module path resolvable by the proxy is
        // importable by anyone who spells it, and nothing in go.mod says
        // otherwise.
        publication: Publication::Unstated,
    });
}

/// The `module` directive's path. Two spellings are legal — `module PATH` and a
/// parenthesised block — and a `//` comment may follow the path on either.
fn module_path(text: &str) -> Option<String> {
    let path = directive_values(text, "module").next()?;
    (!path.is_empty()).then_some(path)
}

/// Every value a directive declares, with the line it was read from: go.mod
/// gives every directive two spellings — `NAME value` and a parenthesised
/// block — so one reader answers `module`, `tool` and `require` alike, and the
/// line comes along for the requirements, whose `// indirect` is a comment
/// that means something.
fn directive_lines<'a>(
    text: &'a str,
    directive: &'a str,
) -> impl Iterator<Item = (String, &'a str)> + 'a {
    let mut in_block = false;
    text.lines().filter_map(move |line| {
        let body = strip_comment(line.trim());
        if in_block {
            if body.starts_with(')') {
                in_block = false;
                return None;
            }
            return first_word(body).map(|v| (v, line));
        }
        let rest = body.strip_prefix(directive)?;
        if !rest.starts_with([' ', '\t', '(']) {
            return None;
        }
        let rest = rest.trim_start();
        if let Some(rest) = rest.strip_prefix('(') {
            in_block = true;
            // `require (` — the opening line normally carries nothing else.
            return first_word(rest).map(|v| (v, line));
        }
        first_word(rest).map(|v| (v, line))
    })
}

fn directive_values<'a>(text: &'a str, directive: &'a str) -> impl Iterator<Item = String> + 'a {
    directive_lines(text, directive).map(|(value, _)| value)
}

fn first_word(line: &str) -> Option<String> {
    let word = line.split_whitespace().next()?;
    let word = word.trim_matches('"');
    (!word.is_empty()).then(|| word.to_string())
}

/// Everything after a `//` is a comment in go.mod, wherever it sits.
fn strip_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => line[..i].trim_end(),
        None => line,
    }
}

/// The module paths this `go.mod` requires. A `// indirect` requirement is
/// still in the build (activation's `ManifestDependency` rules see it) but
/// states no usage claim: it declares `Transitive`. Direct requirements carry
/// no scope — go.mod has no sections, which the adapter declares as
/// `DependencyScoping::Unscoped`.
fn dependencies_of(text: &str) -> Vec<DependencyDeclaration> {
    directive_lines(text, "require")
        .map(|(module, line)| DependencyDeclaration {
            name: SmolStr::new(module),
            scope: line
                .contains("// indirect")
                .then_some(DependencyScope::Transitive),
            version_req: None,
        })
        .collect()
}
