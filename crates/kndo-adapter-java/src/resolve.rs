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

/// The unit is the package: every `.java` sibling in the same directory shares
/// the declared package by the compiler-checked convention — plus the standard
/// layout's two mirrors:
///
/// - `src/test/java/<pkg>` sees `src/main/java/<pkg>` (a package-private test
///   exercises main classes from a parallel source root), one direction only —
///   production never sees test files, which are not on its classpath.
/// - Multi-release variants (`src/main/java9/<pkg>`, `java16`, …) and the base
///   `src/main/java/<pkg>` are ONE unit, symmetrically: the jar tool merges
///   them into the same package of the same artifact, so a reference to the
///   class keeps every release's variant of it.
pub fn sees(path: &ProjectPath, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
    let p = path.as_str();
    let dir = match p.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "",
    };
    let my_release_norm = normalize_release_dir(dir);
    let my_main_view = normalize_test_dir(&my_release_norm);
    let i_am_test = my_main_view != my_release_norm;

    let mut mates: Vec<ProjectPath> = cx
        .known_files()
        .filter(|f| {
            let s = f.as_str();
            if s == p || !s.ends_with(".java") {
                return false;
            }
            let fd = match s.rsplit_once('/') {
                Some((d, _)) => d,
                None => "",
            };
            if fd == dir {
                return true;
            }
            let fd_norm = normalize_release_dir(fd);
            fd_norm == my_release_norm || (i_am_test && fd_norm == my_main_view)
        })
        .cloned()
        .collect();
    mates.sort();
    mates.dedup();
    mates
}

/// `…/src/main/java<digits>/<pkg>` → `…/src/main/java/<pkg>`; anything else
/// unchanged.
fn normalize_release_dir(dir: &str) -> String {
    let marker = "src/main/java";
    let Some(ix) = dir.find(marker) else {
        return dir.to_string();
    };
    if ix != 0 && dir.as_bytes()[ix - 1] != b'/' {
        return dir.to_string();
    }
    let after = &dir[ix + marker.len()..];
    let digits = after.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || !matches!(after.as_bytes().get(digits), None | Some(b'/')) {
        return dir.to_string();
    }
    format!("{}{}{}", &dir[..ix], marker, &after[digits..])
}

/// `…/src/test/java/<pkg>` → `…/src/main/java/<pkg>`; anything else unchanged.
fn normalize_test_dir(dir: &str) -> String {
    let marker = "src/test/java";
    let Some(ix) = dir.find(marker) else {
        return dir.to_string();
    };
    if ix != 0 && dir.as_bytes()[ix - 1] != b'/' {
        return dir.to_string();
    }
    format!("{}src/main/java{}", &dir[..ix], &dir[ix + marker.len()..])
}
