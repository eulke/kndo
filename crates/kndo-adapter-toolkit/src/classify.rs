//! Shared path classification — patterns are per-adapter *data*, the matcher is written once.
//!
//! Every adapter classifies files on the two orthogonal axes (role × origin, contracts §1).
//! The *patterns* differ per language (`*.spec.ts` vs `_test.go`); the *matching machinery*
//! and the **universal** conventions (`vendor/`, `third_party/` trees) must not be
//! re-implemented per adapter — that is how eight subtly-different classifiers happen.

use kndo_core::vocab::{FileClass, FileOrigin, FileRole};

/// Vendored-tree directory names every ecosystem shares. Applied by [`classify`]
/// unconditionally — adapters never restate these.
pub const UNIVERSAL_VENDORED_DIRS: &[&str] = &["vendor", "third_party"];

/// An adapter's classification conventions, as data.
#[derive(Debug, Default)]
pub struct PathPatterns {
    /// Substrings of the *file name* marking a test file (`.test.`, `.spec.`).
    pub test_name_markers: &'static [&'static str],
    /// Directory names anywhere in the path marking test files (`__tests__`, `__mocks__`).
    pub test_dirs: &'static [&'static str],
    /// Substrings of the *file name* marking tooling (`.config.`).
    pub tooling_name_markers: &'static [&'static str],
    /// Directory names anywhere in the path marking tooling (`.storybook`).
    pub tooling_dirs: &'static [&'static str],
}

fn in_dir(path: &str, dir: &str) -> bool {
    path.starts_with(&format!("{dir}/")) || path.contains(&format!("/{dir}/"))
}

/// Classifies a `/`-separated project-relative path. Precedence: test beats tooling (a
/// `*.spec.ts` under `.storybook/` is a test); origin is independent of role.
pub fn classify(path: &str, patterns: &PathPatterns) -> FileClass {
    let file_name = path.rsplit('/').next().unwrap_or(path);

    let origin = if UNIVERSAL_VENDORED_DIRS.iter().any(|d| in_dir(path, d)) {
        FileOrigin::Vendored
    } else {
        FileOrigin::Authored
    };

    let role = if patterns
        .test_name_markers
        .iter()
        .any(|m| file_name.contains(m))
        || patterns.test_dirs.iter().any(|d| in_dir(path, d))
    {
        FileRole::Test
    } else if patterns
        .tooling_name_markers
        .iter()
        .any(|m| file_name.contains(m))
        || patterns.tooling_dirs.iter().any(|d| in_dir(path, d))
    {
        FileRole::Tooling
    } else {
        FileRole::Production
    };

    FileClass { role, origin }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: PathPatterns = PathPatterns {
        test_name_markers: &[".test.", ".spec."],
        test_dirs: &["__tests__"],
        tooling_name_markers: &[".config."],
        tooling_dirs: &[".storybook"],
    };

    #[test]
    fn classifies_both_axes_orthogonally() {
        let c = classify("src/a.ts", &P);
        assert_eq!(
            (c.role, c.origin),
            (FileRole::Production, FileOrigin::Authored)
        );

        let c = classify("vendor/lib/x.test.js", &P);
        assert_eq!((c.role, c.origin), (FileRole::Test, FileOrigin::Vendored));
    }

    #[test]
    fn universal_vendored_dirs_apply_without_adapter_patterns() {
        let c = classify("third_party/x.ts", &PathPatterns::default());
        assert_eq!(c.origin, FileOrigin::Vendored);
    }

    #[test]
    fn test_beats_tooling() {
        let c = classify(".storybook/setup.spec.ts", &P);
        assert_eq!(c.role, FileRole::Test);
    }

    #[test]
    fn dir_markers_do_not_match_substrings_of_names() {
        // "myvendor/" is not "vendor/" — dir matching is segment-anchored.
        let c = classify("myvendor/a.ts", &PathPatterns::default());
        assert_eq!(c.origin, FileOrigin::Authored);
    }
}
