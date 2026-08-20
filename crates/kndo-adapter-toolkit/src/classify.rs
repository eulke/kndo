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

/// One line rule for content-derived generated-origin detection (RFC 0012 §7).
#[derive(Debug)]
pub enum LineMarker {
    /// The line (trailing whitespace ignored) starts with `prefix` *and* ends with `suffix` —
    /// the shape of Go's authoritative convention (`^// Code generated .* DO NOT EDIT\.$`,
    /// `go help generate`) without pulling in a regex engine. Anchoring at column 0 is
    /// deliberate: a codegen *tool's own source* usually mentions the marker inside a string
    /// literal (`fmt.Fprintf(w, "// Code generated…")`), where the line starts with code, not
    /// the comment — anchored matching doesn't misclassify the generator as its own output.
    PrefixSuffix {
        prefix: &'static str,
        suffix: &'static str,
    },
    /// The line contains the substring anywhere (`@generated` and banner-style markers, which
    /// real emitters wrap in every comment shape imaginable).
    Contains(&'static str),
}

/// An adapter's content-derived generated-origin conventions, as data (RFC 0012 §7) — the
/// content-side mirror of [`PathPatterns`]. The *field* the result feeds
/// (`FileFacts::detected_origin`) is the contract; this scanner is a convenience for
/// comment-marker languages — an adapter with a structured signal (Java's `@Generated`
/// annotation) sets the field from the AST it already has instead.
#[derive(Debug)]
pub struct ContentMarkers {
    pub generated_markers: &'static [LineMarker],
    /// Only the first N lines are scanned: every real-world convention puts the marker in the
    /// file header (Go's must precede the first non-comment text), and bounding the scan keeps
    /// extraction cost flat on huge generated bundles — exactly the files most likely to
    /// carry the marker.
    pub scan_window_lines: usize,
}

impl ContentMarkers {
    /// True when any of the first `scan_window_lines` lines matches any marker. Non-UTF-8
    /// lines never match (a binary blob is not a comment banner).
    pub fn detect_generated(&self, content: &[u8]) -> bool {
        content
            .split(|&b| b == b'\n')
            .take(self.scan_window_lines)
            .filter_map(|line| std::str::from_utf8(line).ok())
            .map(|line| line.trim_end())
            .any(|line| {
                self.generated_markers.iter().any(|m| match m {
                    LineMarker::PrefixSuffix { prefix, suffix } => {
                        line.starts_with(prefix) && line.ends_with(suffix)
                    }
                    LineMarker::Contains(needle) => line.contains(needle),
                })
            })
    }
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

    // ------------------------------------------------- ContentMarkers (RFC 0012 §7)

    const GO_STYLE: ContentMarkers = ContentMarkers {
        generated_markers: &[LineMarker::PrefixSuffix {
            prefix: "// Code generated",
            suffix: "DO NOT EDIT.",
        }],
        scan_window_lines: 64,
    };

    #[test]
    fn prefix_suffix_matches_the_go_convention_line() {
        assert!(GO_STYLE
            .detect_generated(b"// Code generated by protoc-gen-go. DO NOT EDIT.\n\npackage pb\n"));
        assert!(!GO_STYLE.detect_generated(b"package pb\n\nfunc F() {}\n"));
    }

    #[test]
    fn prefix_suffix_is_column_anchored_so_the_generator_itself_does_not_match() {
        // The tool that EMITS the marker mentions it in a string literal — indented, mid-line.
        let src = b"package gen\n\nfunc emit() {\n\tfmt.Fprintln(w, \"// Code generated by x. DO NOT EDIT.\")\n}\n";
        assert!(!GO_STYLE.detect_generated(src));
    }

    #[test]
    fn trailing_whitespace_and_crlf_do_not_defeat_the_suffix() {
        assert!(GO_STYLE.detect_generated(b"// Code generated by x. DO NOT EDIT.\r\npackage pb\n"));
        assert!(GO_STYLE.detect_generated(b"// Code generated by x. DO NOT EDIT.   \npackage pb\n"));
    }

    #[test]
    fn scan_window_bounds_the_search() {
        let mut src = String::new();
        for _ in 0..80 {
            src.push_str("// filler\n");
        }
        src.push_str("// Code generated by x. DO NOT EDIT.\n");
        assert!(
            !GO_STYLE.detect_generated(src.as_bytes()),
            "marker beyond the window must not match"
        );
    }

    #[test]
    fn contains_matches_anywhere_in_a_header_line() {
        let js = ContentMarkers {
            generated_markers: &[LineMarker::Contains("@generated")],
            scan_window_lines: 64,
        };
        assert!(js.detect_generated(b"/**\n * @generated by codegen\n */\nexport const x = 1;\n"));
        assert!(!js.detect_generated(b"export const x = 1;\n"));
    }

    #[test]
    fn binary_content_never_matches() {
        assert!(!GO_STYLE.detect_generated(&[0xff, 0xfe, 0x00, 0x01]));
    }
}
