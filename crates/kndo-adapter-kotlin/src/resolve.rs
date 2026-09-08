//! Resolution by the NAMESPACE, on the same toolkit mechanism Java uses —
//! Kotlin and Java share one package namespace, so a mixed module's import
//! crosses between them without noticing, and two implementations would be two
//! answers to one question.
//!
//! Kotlin does not even RECOMMEND the directory mirror outside
//! `src/main/kotlin`, and a file may declare any package whatever it is called,
//! which is why reading a path here was always the wrong question. A top-level
//! function, a property, a type alias and a nested type are all reached the
//! same way: the package answers with its files and the import's binding picks
//! the name among them.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;

pub fn resolve(from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
    kndo_toolkit::resolve_in_namespace(from, specifier, cx)
}
