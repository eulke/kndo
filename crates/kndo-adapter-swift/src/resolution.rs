//! Import resolution (docs/adapters/swift.md §3): pure lookups against `ResolveCtx`'s known-
//! files/units index — same algorithm as Java/Kotlin's, since Swift's `unit` (an SPM target
//! name, `manifest.rs::unit_for_path`) plays exactly the role a declared package name does
//! there. The one real difference: a Swift `import` names a module directly (no static-import
//! sentinel shape, no wildcard form — Swift has neither).

use kndo_core::adapter::{ImportSpec, Resolution, ResolveCtx};

/// Ships with the Swift toolchain/Apple SDK, never a package dependency — inherently non-
/// exhaustive against the full SDK surface (docs/adapters/swift.md §7), widened only if real
/// dogfooding surfaces a false `undeclared`.
const STDLIB_MODULES: &[&str] = &[
    "Swift",
    "Foundation",
    "Combine",
    "Dispatch",
    "SwiftUI",
    "UIKit",
    "AppKit",
    "CoreData",
    "CoreGraphics",
    "os",
    "XCTest",
];

pub(crate) fn resolve(spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
    let module = spec.specifier.as_str();
    if STDLIB_MODULES.contains(&module) {
        return Resolution::Stdlib;
    }
    let Some(target) = ctx.unit_files(module).first() else {
        return Resolution::Unresolved;
    };
    Resolution::File(target.clone(), kndo_core::vocab::Confidence::Certain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_core::adapter::ProjectPath;
    use kndo_core::vocab::Confidence;
    use rustc_hash::FxHashSet;
    use smol_str::SmolStr;

    fn path(p: &str) -> ProjectPath {
        ProjectPath(SmolStr::new(p))
    }

    fn spec(specifier: &str) -> ImportSpec {
        ImportSpec {
            specifier: SmolStr::new(specifier),
            from: path("Sources/MyLib/C.swift"),
        }
    }

    #[test]
    fn platform_sdk_modules_are_stdlib() {
        let known: FxHashSet<ProjectPath> = FxHashSet::default();
        let ctx = ResolveCtx::new(&known);
        assert_eq!(resolve(&spec("Foundation"), &ctx), Resolution::Stdlib);
        assert_eq!(resolve(&spec("Swift"), &ctx), Resolution::Stdlib);
        assert_eq!(resolve(&spec("SwiftUI"), &ctx), Resolution::Stdlib);
    }

    #[test]
    fn unresolved_external_module_never_becomes_a_dependency_edge() {
        let known: FxHashSet<ProjectPath> = FxHashSet::default();
        let ctx = ResolveCtx::new(&known);
        assert_eq!(resolve(&spec("Alamofire"), &ctx), Resolution::Unresolved);
    }

    #[test]
    fn a_local_target_name_resolves_to_its_first_file() {
        let known: FxHashSet<ProjectPath> = [path("Sources/Core/A.swift")].into_iter().collect();
        let units: rustc_hash::FxHashMap<SmolStr, Vec<ProjectPath>> =
            [(SmolStr::new("Core"), vec![path("Sources/Core/A.swift")])]
                .into_iter()
                .collect();
        let ctx = ResolveCtx::new(&known).with_units(&units);
        assert_eq!(
            resolve(&spec("Core"), &ctx),
            Resolution::File(path("Sources/Core/A.swift"), Confidence::Certain)
        );
    }
}
