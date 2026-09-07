//! Resolution by the compiler-checked convention: `com.foo.Bar` lives at
//! `<source root>/com/foo/Bar.java`, so an import resolves by PATH SUFFIX
//! against the discovered file set — no classpath, no unit index, javac's rules
//! mirrored from the one fact javac itself enforces. Third-party packages match
//! no suffix and stay `Unresolved`, deliberately: no reliable
//! package→Maven/Gradle-coordinate mapping exists without resolving the
//! classpath, and guessing would flood false positives.
//!
//! Multi-module repositories can hold the same package in sibling modules
//! (retrofit's shape: two source trees both declaring `package retrofit2;`).
//! Among equal suffix matches the importer's NEAREST target wins — longest
//! shared path prefix — so an intra-module import lands in its own module and
//! never invents a cross-module edge; a package that genuinely lives only in a
//! sibling still resolves, because nearest falls back rather than refusing.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;
use kndo_toolkit as tk;

pub fn resolve(from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
    // `com.foo.Bar` → a single type's file; `com.foo` (a Glob import's package)
    // → the package directory's files. Which one the specifier is cannot be
    // read off its spelling — `com.foo.Bar.Baz` is a NESTED type whose file is
    // `com/foo/Bar.java` — so: try the file, then the directory, then peel the
    // last segment (nested types, at most one level deep in practice) and try
    // the file again.
    let path = specifier.replace('.', "/");

    if let Some(file) = tk::nearest_suffix_match(&format!("{path}.java"), from, cx) {
        return Resolution::File(file);
    }
    let dir_members = package_dir_files(&path, cx);
    if !dir_members.is_empty() {
        return Resolution::Files(dir_members);
    }
    if let Some((parent, _)) = path.rsplit_once('/')
        && let Some(file) = tk::nearest_suffix_match(&format!("{parent}.java"), from, cx)
    {
        return Resolution::File(file);
    }
    Resolution::Unresolved
}

/// Every direct `.java` child of any directory whose path ends in the package's
/// segments — ALL modules' matches, deliberately: a wildcard import is opaque
/// use over the package, and the keep-alive direction includes every candidate.
fn package_dir_files(dir_suffix: &str, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
    let want = format!("/{dir_suffix}/");
    let mut out: Vec<ProjectPath> = cx
        .known_files()
        .filter(|p| {
            let s = p.as_str();
            if !s.ends_with(".java") {
                return false;
            }
            let Some(dir_end) = s.rfind('/') else {
                return false;
            };
            let dir = &s[..dir_end + 1];
            dir.ends_with(&want) || dir == &want[1..]
        })
        .cloned()
        .collect();
    out.sort();
    out
}


