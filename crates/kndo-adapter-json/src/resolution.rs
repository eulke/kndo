//! Import resolution: structurally unreachable in normal operation.
//! `extraction::extract` never populates `FileFacts::imports` (JSON has no import syntax of its
//! own), and `kndo-core/src/graph/assemble.rs`'s `resolve_imports` only ever calls an adapter's
//! `resolve()`
//! once per entry in *that adapter's own claimed file's* `facts.imports` — never as a fan-out
//! to every registered adapter. This exists only to satisfy the `LanguageAdapter` trait.

use kndo_core::adapter::{ImportSpec, Resolution, ResolveCtx};

pub(crate) fn resolve(_spec: &ImportSpec, _ctx: &ResolveCtx<'_>) -> Resolution {
    Resolution::Unresolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_core::adapter::ProjectPath;
    use rustc_hash::FxHashSet;
    use smol_str::SmolStr;

    #[test]
    fn always_unresolved_regardless_of_specifier_or_known_files() {
        let known: FxHashSet<ProjectPath> = [ProjectPath(SmolStr::new("data.json"))]
            .into_iter()
            .collect();
        let ctx = ResolveCtx::new(&known);
        let spec = ImportSpec {
            specifier: SmolStr::new("data.json"),
            from: ProjectPath(SmolStr::new("src/main.ts")),
        };
        assert_eq!(resolve(&spec, &ctx), Resolution::Unresolved);
    }
}
