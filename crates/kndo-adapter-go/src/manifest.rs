//! `go.mod` parsing → `ManifestFacts`. A small hand-written parser —
//! `go.mod`'s grammar (`module`/`go`/`require`/`replace`/`exclude`, single-line or parenthesized
//! block form) is simple enough that a dependency would add more than it saves, the same
//! "no more machinery than the format needs" stance `package.json`'s `serde_json` parse takes
//! for a format that *does* warrant a real parser.
//!
//! `go.mod`'s surface is genuinely much thinner than `package.json`'s:
//! no publish-privacy flag, no entry-point concept, no scripts, no explicit-surface
//! declaration — `roots`/`resolved_entries`/`declares_surface`/`script_invoked_names` all stay
//! empty/false/unset here. Go's roots (`func main`, `func init`, every non-internal exported
//! declaration) are source-file facts, extracted in `extraction.rs`, not manifest ones.

use kndo_core::adapter::{
    Diagnostic, DiagnosticLevel, ManifestDependency, ManifestFacts, ResolveCtx,
};
use kndo_core::vocab::DependencyScope;
use smol_str::SmolStr;

pub(crate) fn extract(path: &str, content: &[u8], _ctx: &ResolveCtx<'_>) -> ManifestFacts {
    let mut out = ManifestFacts::default();
    let Ok(text) = std::str::from_utf8(content) else {
        out.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warn,
            path: None,
            message: "go manifest is not valid UTF-8".to_string(),
            span: None,
        });
        return out;
    };

    if path.rsplit('/').next() == Some("go.work") {
        return extract_go_work(text, out);
    }

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
            // `// indirect` entries (checked pre-strip) are transitive requirements `go mod
            // tidy` maintains, not the author's declarations — nothing in the module imports
            // them BY DESIGN, so counting them as declared guarantees a false `unused`
            // dependency per entry.
            if !raw_line.contains("// indirect") {
                out.dependencies.extend(parse_require_entry(line));
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("module ") {
            out.package_name = Some(SmolStr::new(rest.trim()));
        } else if line == "require (" {
            in_require_block = true;
        } else if let Some(rest) = line.strip_prefix("require ") {
            if !raw_line.contains("// indirect") {
                out.dependencies.extend(parse_require_entry(rest.trim()));
            }
        }
        // `go 1.x`, `toolchain`, `replace`, `exclude` directives: not surfaced — none of them
        // are dependency declarations. A local `replace` whose target declares the same module
        // path already resolves through the workspace-member index; one that
        // *renames* a module path is the recorded divergence, not modeled.
    }

    // No publish-privacy flag exists in go.mod at all — this is a
    // statement of fact, not a default guess; Go's real per-package privacy signal is the
    // `internal/` path convention, handled entirely in extraction.rs.
    out.private = false;
    out
}

/// `go.work`: the workspace aggregator — `use` directives (single-line or
/// parenthesized block) become `workspace_members`. Everything else the format allows (`go`,
/// `toolchain`, `replace`) contributes nothing here: member modules already self-register in
/// the core's workspace index by their own declared module paths, so cross-module resolution
/// needs no data from this file — and a local `replace` whose target declares the same module
/// path resolves through that same index anyway. The one true gap, a `replace` that *renames*
/// a module path, is a recorded divergence, not modeled. No `module` directive
/// exists in the format, so `package_name` stays `None` — the go.work PackageNode is an
/// aggregator, never a same-directory shadow of a real module's `go.mod` (assembly's ownership
/// tie-break is discovery order, and `go.mod` sorts first).
fn extract_go_work(text: &str, mut out: ManifestFacts) -> ManifestFacts {
    let mut in_use_block = false;
    for raw_line in text.lines() {
        let line = strip_line_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if in_use_block {
            if line == ")" {
                in_use_block = false;
                continue;
            }
            push_use_entry(line, &mut out);
            continue;
        }
        if line == "use (" {
            in_use_block = true;
        } else if let Some(rest) = line.strip_prefix("use ") {
            push_use_entry(rest.trim(), &mut out);
        }
    }
    out.private = false; // same statement of fact as go.mod: the format has no publish flag
    out
}

fn push_use_entry(entry: &str, out: &mut ManifestFacts) {
    let cleaned = entry.trim_matches('"').trim_start_matches("./");
    if !cleaned.is_empty() {
        out.workspace_members.push(SmolStr::new(cleaned));
    }
}

fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// `github.com/pkg/errors v0.9.1` → one dependency. Only DIRECT requirements reach here —
/// the caller drops `// indirect` entries before comment-stripping: they are `go mod
/// tidy`'s transitive bookkeeping, not author declarations, and nothing in the module imports
/// them by design. Go has exactly one scope for what remains.
fn parse_require_entry(entry: &str) -> Option<ManifestDependency> {
    let mut parts = entry.split_whitespace();
    let name = parts.next()?;
    let version = parts.next().unwrap_or("");
    Some(ManifestDependency {
        name: SmolStr::new(name),
        version_req: SmolStr::new(version),
        scope: DependencyScope::Prod,
        inherited: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet as HashSet;

    fn ctx() -> ResolveCtx<'static> {
        static EMPTY: std::sync::OnceLock<HashSet<kndo_core::adapter::ProjectPath>> =
            std::sync::OnceLock::new();
        ResolveCtx::new(EMPTY.get_or_init(HashSet::default))
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
    fn indirect_require_entries_are_not_declared_dependencies() {
        // `// indirect` entries are `go mod tidy`'s bookkeeping of transitive requirements —
        // nothing in the module imports them by design, so declaring them would guarantee a
        // false `unused` dependency each. Single-line and block forms alike.
        let facts = extract(
            "go.mod",
            br#"module m

require gopkg.in/yaml.v3 v3.0.1 // indirect

require (
	github.com/pkg/errors v0.9.1
	golang.org/x/net v0.10.0 // indirect
)
"#,
            &ctx(),
        );
        let names: Vec<&str> = facts.dependencies.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["github.com/pkg/errors"]);
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

    // ------------------------------------------------- go.work

    #[test]
    fn go_work_use_directives_become_workspace_members() {
        let facts = extract(
            "go.work",
            b"go 1.22\n\nuse ./tools\n\nuse (\n\t./moda\n\t./modb // trailing comment\n)\n",
            &ctx(),
        );
        let members: Vec<&str> = facts.workspace_members.iter().map(|m| m.as_str()).collect();
        assert_eq!(members, vec!["tools", "moda", "modb"]);
        assert_eq!(facts.package_name, None, "go.work declares no module");
        assert!(facts.dependencies.is_empty());
        assert!(facts.roots.is_empty());
    }

    #[test]
    fn go_work_replace_and_go_directives_contribute_nothing() {
        let facts = extract(
            "go.work",
            b"go 1.22\n\nuse ./a\n\nreplace example.com/x => ../x\n",
            &ctx(),
        );
        assert_eq!(facts.workspace_members.len(), 1);
        assert!(facts.dependencies.is_empty());
    }

    #[test]
    fn nested_go_work_path_still_parses_as_go_work() {
        let facts = extract("sub/go.work", b"use ./m\n", &ctx());
        assert_eq!(facts.workspace_members.len(), 1);
    }
}
