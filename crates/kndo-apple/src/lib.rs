//! Apple bundle conventions — the two places an app names code as a STRING and
//! the system instantiates it: an Interface Builder document (`customClass`,
//! outlets, actions) and a bundle's `Info.plist` (its principal and delegate
//! class keys). Neither is Swift, so no language adapter can see the reference;
//! each plugin here reads the XML the artifact already is, resolves the names it
//! carries against the declarations the graph found, and anchors roots on them
//! — asserting nothing about a name it cannot point at a real declaration for.
//!
//! Format knowledge only: no Swift is parsed and no module resolved. Xcode
//! writes both formats, and the proof fixture carries its output verbatim.

mod info_plist;
mod interface_builder;
mod names;

pub use info_plist::InfoPlistPlugin;
pub use interface_builder::InterfaceBuilderPlugin;
