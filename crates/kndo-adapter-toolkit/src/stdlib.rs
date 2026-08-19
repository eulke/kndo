//! Shared stdlib/builtins mechanism — one implementation for every adapter (RFC 0002 §6).
//!
//! Every language has runtime-provided module namespaces whose set tracks the runtime's
//! release cadence (Node builtins, Go std packages, Java modules). The *classification logic*
//! differs per language; the *data lifecycle* must not: one file format, one loader, one
//! validation, one precedence rule. An adapter ships a generated `stdlib.txt` (checked-in
//! generator querying the authoritative source), embeds it with `include_str!`, and calls
//! [`classify_bare_specifier`] — nothing stdlib-shaped is ever hand-maintained code again.
//!
//! # Data format (`kndo-stdlib v1`)
//!
//! ```text
//! # kndo-stdlib v1
//! # language: js-ts
//! # source: node -p require('module').builtinModules
//! # source-version: v22.22.2
//! # regenerate: node scripts/gen-stdlib-js.mjs
//! fs
//! fs/promises
//! ...
//! ```
//!
//! Header keys are machine-readable so `kndo doctor` can report data provenance. Entries are
//! sorted unique names. The format is versioned by its magic line: a future v2 can add
//! per-entry annotations (version ranges, deprecation) without breaking v1 loaders.

use std::collections::HashSet;

use kndo_core::adapter::ResolveCtx;
use kndo_core::vocab::Confidence;
use smol_str::SmolStr;

const MAGIC: &str = "# kndo-stdlib v1";

/// A parsed, validated stdlib dataset. Construct once per adapter (in a `LazyLock`).
#[derive(Debug)]
pub struct StdlibIndex<'a> {
    entries: HashSet<&'a str>,
    language: &'a str,
    source: &'a str,
    source_version: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StdlibDataError {
    MissingMagic,
    MissingHeader(&'static str),
    Empty,
    Unsorted,
    Duplicate,
}

impl<'a> StdlibIndex<'a> {
    /// Parses and validates `kndo-stdlib v1` data. Adapters call this in a `LazyLock`
    /// initializer over `include_str!` data (`StdlibIndex<'static>`); a malformed *shipped*
    /// dataset is a build defect, so failing loudly at first use (`expect`) is correct.
    /// The borrowed lifetime also lets the generator (xtask) validate freshly-built strings
    /// without leaking.
    pub fn parse(data: &'a str) -> Result<StdlibIndex<'a>, StdlibDataError> {
        let mut lines = data.lines().map(str::trim);
        if lines.next() != Some(MAGIC) {
            return Err(StdlibDataError::MissingMagic);
        }

        let mut language = None;
        let mut source = None;
        let mut source_version = None;
        let mut entries: Vec<&'a str> = Vec::new();

        for line in data.lines().skip(1).map(str::trim) {
            if line.is_empty() {
                continue;
            }
            if let Some(header) = line.strip_prefix('#') {
                let header = header.trim();
                if let Some((key, value)) = header.split_once(':') {
                    let value = value.trim();
                    match key.trim() {
                        "language" => language = Some(value),
                        "source" => source = Some(value),
                        "source-version" => source_version = Some(value),
                        _ => {} // unknown header keys are fine (forward compatibility)
                    }
                }
                continue;
            }
            entries.push(line);
        }

        if entries.is_empty() {
            return Err(StdlibDataError::Empty);
        }
        if !entries.windows(2).all(|w| w[0] <= w[1]) {
            return Err(StdlibDataError::Unsorted);
        }
        let set: HashSet<&'a str> = entries.iter().copied().collect();
        if set.len() != entries.len() {
            return Err(StdlibDataError::Duplicate);
        }

        Ok(StdlibIndex {
            entries: set,
            language: language.ok_or(StdlibDataError::MissingHeader("language"))?,
            source: source.ok_or(StdlibDataError::MissingHeader("source"))?,
            source_version: source_version
                .ok_or(StdlibDataError::MissingHeader("source-version"))?,
        })
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains(name)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Provenance for `kndo doctor`: which language, source command, and runtime version
    /// this dataset was generated from.
    pub fn provenance(&self) -> (&str, &str, &str) {
        (self.language, self.source, self.source_version)
    }
}

/// The canonical bare-specifier precedence, written once for every language:
///
/// 1. **Structural stdlib signal** (Node's `node:` prefix, and future equivalents) — wins over
///    everything: unambiguous by the language's own construction, version-proof.
/// 2. **Manifest-declared dependency** — wins over the stdlib *list*: declared intent beats
///    shipped data, neutralizing stale-list shadowing (the userland `punycode` package).
/// 3. **Stdlib list** — the generated dataset.
/// 4. **External dependency** — everything else.
///
/// The adapter supplies what only it knows (the structural check result and the
/// subpath→package mapping); this function owns the ordering so no adapter re-derives it.
pub fn classify_bare_specifier(
    specifier: &str,
    package_name: SmolStr,
    is_structural_stdlib: bool,
    stdlib: &StdlibIndex<'_>,
    ctx: &ResolveCtx<'_>,
) -> kndo_core::adapter::Resolution {
    use kndo_core::adapter::Resolution;

    if is_structural_stdlib {
        return Resolution::Stdlib;
    }
    if ctx.is_declared_dependency(&package_name) {
        return Resolution::Dependency(package_name, Confidence::Certain);
    }
    if stdlib.contains(specifier) {
        return Resolution::Stdlib;
    }
    Resolution::Dependency(package_name, Confidence::Certain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_core::adapter::Resolution;

    const GOOD: &str = "# kndo-stdlib v1\n# language: test-lang\n# source: test cmd\n# source-version: v1.0\nalpha\nbeta\ngamma\n";

    #[test]
    fn parses_valid_data_with_provenance() {
        let idx = StdlibIndex::parse(GOOD).unwrap();
        assert!(idx.contains("beta"));
        assert!(!idx.contains("delta"));
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.provenance(), ("test-lang", "test cmd", "v1.0"));
    }

    #[test]
    fn rejects_missing_magic_unsorted_duplicates_and_empty() {
        assert_eq!(
            StdlibIndex::parse("# language: x\nalpha\n").unwrap_err(),
            StdlibDataError::MissingMagic
        );
        assert_eq!(
            StdlibIndex::parse(
                "# kndo-stdlib v1\n# language: x\n# source: s\n# source-version: v\nbeta\nalpha\n"
            )
            .unwrap_err(),
            StdlibDataError::Unsorted
        );
        assert_eq!(
            StdlibIndex::parse(
                "# kndo-stdlib v1\n# language: x\n# source: s\n# source-version: v\nalpha\nalpha\n"
            )
            .unwrap_err(),
            StdlibDataError::Duplicate
        );
        assert_eq!(
            StdlibIndex::parse(
                "# kndo-stdlib v1\n# language: x\n# source: s\n# source-version: v\n"
            )
            .unwrap_err(),
            StdlibDataError::Empty
        );
    }

    #[test]
    fn precedence_structural_beats_all() {
        let idx = StdlibIndex::parse(GOOD).unwrap();
        let files = std::collections::HashSet::new();
        let mut deps = std::collections::HashSet::new();
        deps.insert(SmolStr::new("alpha"));
        let ctx = ResolveCtx::new(&files).with_declared_dependencies(&deps);
        // Even a declared dep named like the specifier loses to a structural signal.
        let r = classify_bare_specifier("alpha", SmolStr::new("alpha"), true, &idx, &ctx);
        assert_eq!(r, Resolution::Stdlib);
    }

    #[test]
    fn precedence_declared_dependency_shadows_stdlib_list() {
        let idx = StdlibIndex::parse(GOOD).unwrap();
        let files = std::collections::HashSet::new();
        let mut deps = std::collections::HashSet::new();
        deps.insert(SmolStr::new("alpha")); // userland package shadowing a stdlib name
        let ctx = ResolveCtx::new(&files).with_declared_dependencies(&deps);
        let r = classify_bare_specifier("alpha", SmolStr::new("alpha"), false, &idx, &ctx);
        assert_eq!(
            r,
            Resolution::Dependency(SmolStr::new("alpha"), Confidence::Certain)
        );
    }

    #[test]
    fn precedence_stdlib_list_then_dependency() {
        let idx = StdlibIndex::parse(GOOD).unwrap();
        let files = std::collections::HashSet::new();
        let ctx = ResolveCtx::new(&files);
        let r = classify_bare_specifier("beta", SmolStr::new("beta"), false, &idx, &ctx);
        assert_eq!(r, Resolution::Stdlib);
        let r = classify_bare_specifier("delta", SmolStr::new("delta"), false, &idx, &ctx);
        assert_eq!(
            r,
            Resolution::Dependency(SmolStr::new("delta"), Confidence::Certain)
        );
    }
}
