//! `go.mod` parsing → `ManifestFacts` (docs/adapters/go.md §4). A small hand-written parser —
//! `go.mod`'s grammar (`module`/`go`/`require`/`replace`/`exclude`, single-line or parenthesized
//! block form) is simple enough that a dependency would add more than it saves, the same
//! "no more machinery than the format needs" stance `package.json`'s `serde_json` parse takes
//! for a format that *does* warrant a real parser.
//!
//! `go.mod`'s surface is genuinely much thinner than `package.json`'s (docs/adapters/go.md §4's
//! table): no publish-privacy flag, no entry-point concept, no scripts, no explicit-surface
//! declaration — `roots`/`resolved_entries`/`declares_surface`/`script_invoked_names` all stay
//! empty/false/unset here. Go's roots (`func main`, `func init`, every non-internal exported
//! declaration) are source-file facts, extracted in `extraction.rs`, not manifest ones.

use kndo_core::adapter::{
    Diagnostic, DiagnosticLevel, ManifestDependency, ManifestFacts, ResolveCtx,
};
use kndo_core::vocab::DependencyScope;
use smol_str::SmolStr;

pub fn extract(_path: &str, content: &[u8], _ctx: &ResolveCtx<'_>) -> ManifestFacts {
    let mut out = ManifestFacts::default();
    let Ok(text) = std::str::from_utf8(content) else {
        out.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warn,
            path: None,
            message: "go.mod is not valid UTF-8".to_string(),
            span: None,
        });
        return out;
    };

    let mut in_require_block = false;
    for raw_line in text.lines() {
        let line = strip_line_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }

        if in_require_block {
            if line == ")" {
                in_require_block = false;
                continue;
            }
            out.dependencies.extend(parse_require_entry(line));
            continue;
        }

        if let Some(rest) = line.strip_prefix("module ") {
            out.package_name = Some(SmolStr::new(rest.trim()));
        } else if line == "require (" {
            in_require_block = true;
        } else if let Some(rest) = line.strip_prefix("require ") {
            out.dependencies.extend(parse_require_entry(rest.trim()));
        }
        // `go 1.x`, `toolchain`, `replace`, `exclude` directives: not surfaced — none of them
        // are dependency declarations, and `replace` (a local-path or version override) has no
        // ManifestFacts field to land in today (closest to RFC 0011's `go.work` support, itself
        // deferred — docs/adapters/go.md §7).
    }

    // No publish-privacy flag exists in go.mod at all (docs/adapters/go.md §4) — this is a
    // statement of fact, not a default guess; Go's real per-package privacy signal is the
    // `internal/` path convention, handled entirely in extraction.rs.
    out.private = false;
    out
}

fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// `github.com/pkg/errors v0.9.1` (the `// indirect` marker, if any, is already stripped by the
/// caller) → one dependency. Go has exactly one scope (docs/adapters/go.md §4) — `// indirect`
/// (transitively pulled, not imported by this module directly) isn't surfaced as a different
/// one: kndo's scope taxonomy has no "transitive" concept, and treating it as anything but
/// `Prod` would misrepresent it as unused/optional when it's exactly as required as a direct
/// dependency, from `go build`'s point of view.
fn parse_require_entry(entry: &str) -> Option<ManifestDependency> {
    let mut parts = entry.split_whitespace();
    let name = parts.next()?;
    let version = parts.next().unwrap_or("");
    Some(ManifestDependency {
        name: SmolStr::new(name),
        version_req: SmolStr::new(version),
        scope: DependencyScope::Prod,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn ctx() -> ResolveCtx<'static> {
        static EMPTY: std::sync::OnceLock<HashSet<kndo_core::adapter::ProjectPath>> =
            std::sync::OnceLock::new();
        ResolveCtx::new(EMPTY.get_or_init(HashSet::new))
    }

    #[test]
    fn module_directive_becomes_package_name() {
        let facts = extract("go.mod", b"module github.com/foo/bar\n\ngo 1.22\n", &ctx());
        assert_eq!(facts.package_name.as_deref(), Some("github.com/foo/bar"));
        assert!(!facts.private);
    }

    #[test]
    fn single_line_require_is_parsed() {
        let facts = extract(
            "go.mod",
            b"module m\n\nrequire golang.org/x/net v0.10.0\n",
            &ctx(),
        );
        assert_eq!(facts.dependencies.len(), 1);
        assert_eq!(facts.dependencies[0].name.as_str(), "golang.org/x/net");
        assert_eq!(facts.dependencies[0].version_req.as_str(), "v0.10.0");
        assert_eq!(facts.dependencies[0].scope, DependencyScope::Prod);
    }

    #[test]
    fn require_block_with_indirect_comment_is_parsed() {
        let facts = extract(
            "go.mod",
            br#"module m

require (
	github.com/pkg/errors v0.9.1
	golang.org/x/net v0.10.0 // indirect
)
"#,
            &ctx(),
        );
        let names: Vec<&str> = facts.dependencies.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["github.com/pkg/errors", "golang.org/x/net"]);
    }

    #[test]
    fn no_roots_or_entries_or_surface_from_a_manifest_alone() {
        let facts = extract("go.mod", b"module m\n", &ctx());
        assert!(facts.roots.is_empty());
        assert!(facts.resolved_entries.is_empty());
        assert!(!facts.declares_surface);
        assert!(facts.script_invoked_names.is_empty());
    }

    #[test]
    fn invalid_utf8_degrades_to_a_diagnostic_not_a_panic() {
        let facts = extract("go.mod", &[0xff, 0xfe, 0x00], &ctx());
        assert!(!facts.diagnostics.is_empty());
        assert_eq!(facts.package_name, None);
    }
}
