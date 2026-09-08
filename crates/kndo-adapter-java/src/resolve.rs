//! Resolution by the NAMESPACE, which is what a Java import names: `import
//! com.foo.Bar;` says the type `Bar`, in the package `com.foo`. The package is
//! a clause every file in it declares — `Nesting::Flat`, the spec's word for it
//! — so the mechanism is the toolkit's, shared with Kotlin because the two
//! share one package namespace and a mixed module's import crosses between them
//! without noticing.
//!
//! Nothing here looks at a path. A directory mirroring the package is javac's
//! convention for FINDING sources on disk, not the language's rule for what a
//! name means: a package can live in two source trees, a source tree can hold a
//! package it does not mirror, and neither changes the answer.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;

pub fn resolve(from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
    kndo_toolkit::resolve_in_namespace(from, specifier, cx)
}
