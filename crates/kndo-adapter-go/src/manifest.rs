//! What one `go.mod` teaches the engine: the module is ONE unit — Go compiles a
//! module's packages together, and there is no per-target manifest section to
//! split it — declared as a library, because a module path is what any other
//! module imports. The same line becomes an entry-less package: a Go module maps
//! import prefixes to directories, so no file a bare import resolves to exists.
//! Entries need no manifest here: `package main` + `func main` and `_test.go`
//! are extraction's to see, and a module has no single file the build enters.

use crate::modfile;
use kndo_contract::adapter::{
    DependencyDeclaration, DependencyScope, PackageEntry, ResolveContext, SourceFile,
};
use kndo_contract::manifest::{ManifestSink, Publication, Unit, UnitDep, UnitKind};
use kndo_contract::vocab::ProjectPath;
use smol_str::SmolStr;

pub fn structure(manifest: &SourceFile<'_>, cx: &ResolveContext<'_>, out: &mut ManifestSink) {
    let Ok(text) = std::str::from_utf8(manifest.content) else {
        return;
    };
    if manifest.path.as_str().ends_with("go.work") {
        return workspace(manifest, text, cx, out);
    }
    let mut depends_on: Vec<SmolStr> = Vec::new();
    for declaration in dependencies_of(text) {
        depends_on.push(declaration.name.clone());
        out.dependency(declaration);
    }
    depends_on.sort();
    depends_on.dedup();
    // A `tool` line names a package whose module the requirements already
    // carry: `go get -tool` writes both, and the tool is used with no import.
    for tool in modfile::of(text, "tool").filter_map(|d| d.token(0).map(str::to_string)) {
        out.mention(tool);
    }
    let Some(module) = module_path(text) else {
        return;
    };
    out.package(PackageEntry {
        name: SmolStr::new(&module),
        entry: None,
        dir: SmolStr::new(kndo_toolkit::parent_dir(manifest.path.as_str())),
        aliases: Vec::new(),
        subpaths: Vec::new(),
    });
    out.unit(Unit {
        name: SmolStr::new(&module),
        kind: UnitKind::Library,
        // Empty: the engine reads the manifest's own directory, which is every
        // package of the module and nothing above it.
        roots: Vec::new(),
        excludes: Vec::new(),
        entries: Vec::new(),
        // Go compiles a module as one thing: every requirement is a plain
        // dependency, and there is no second unit to befriend.
        depends_on: depends_on.into_iter().map(UnitDep::on).collect(),
        // Go has no `publish = false`: a module path resolvable by the proxy is
        // importable by anyone who spells it, and nothing in go.mod says
        // otherwise. What they spell is a NAME — the module path plus the
        // package — so every exported identifier of every file is on the
        // surface, and no entry file gates it.
        publication: Publication::ByName,
        // Every package of the module is imported by the module path plus its
        // directory — the `module` line is the prefix all of them hang under.
        namespace_root: Some(SmolStr::new(&module)),
    });
}

/// The `module` directive's path.
fn module_path(text: &str) -> Option<String> {
    let path = modfile::of(text, "module").next()?.token(0)?.to_string();
    (!path.is_empty()).then_some(path)
}

/// `go.work`: a WORKSPACE, which is not a unit of its own. Its `use` lines name
/// the directories the go tool builds together, and in workspace mode every
/// module in that set resolves the others' packages with no `require` line
/// anywhere — which is exactly what made a workspace import read as
/// `undeclared`.
///
/// So the file says two things and declares nothing. Each used directory's
/// `go.mod` is a MEMBER of this manifest, which is how a requirement naming a
/// sibling resolves to that sibling's unit rather than to a module of the same
/// name outside; and each used module's PATH is mentioned here, read from the
/// `go.mod` the directory holds, because the project supplying a name itself is
/// what `undeclared` must not accuse.
fn workspace(
    manifest: &SourceFile<'_>,
    text: &str,
    cx: &ResolveContext<'_>,
    out: &mut ManifestSink,
) {
    let dir = kndo_toolkit::parent_dir(manifest.path.as_str());
    for used in modfile::of(text, "use").filter_map(|d| d.token(0).map(str::to_string)) {
        let Some(joined) = kndo_toolkit::join_relative(dir, &used) else {
            continue;
        };
        let member = ProjectPath::new(if joined.is_empty() {
            "go.mod".to_string()
        } else {
            format!("{joined}/go.mod")
        });
        // The member's own module line, read through the engine rather than
        // guessed from the directory: a module path and its directory agree
        // only by convention, and this reader states no conventions. A `use`
        // naming a directory with no `go.mod` under it names nothing, and
        // saying so is more honest than aggregating an absence.
        let Some(member_text) = cx
            .manifest(&member)
            .and_then(|b| std::str::from_utf8(b).ok())
        else {
            continue;
        };
        out.member(member.clone());
        if let Some(path) = module_path(member_text) {
            out.mention(path);
        }
    }
}

/// The module paths this `go.mod` requires. A `// indirect` requirement is
/// still in the build (activation's `ManifestDependency` rules see it) but
/// states no usage claim: it declares `Transitive`. Direct requirements carry
/// no scope — go.mod has no sections, which the adapter declares as
/// `DependencyScoping::Unscoped`.
fn dependencies_of(text: &str) -> Vec<DependencyDeclaration> {
    modfile::of(text, "require")
        .filter_map(|d| {
            Some(DependencyDeclaration {
                name: SmolStr::new(d.token(0)?),
                scope: d.indirect().then_some(DependencyScope::Transitive),
                version_req: None,
            })
        })
        .collect()
}
