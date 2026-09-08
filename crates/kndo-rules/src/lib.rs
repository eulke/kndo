//! What a FRAMEWORK means, as data.
//!
//! A rule pack is an extension that claims no files and declares nothing but
//! [`DispatchRule`]s. It owns no language and parses nothing: the language
//! adapter already reported the markers and relations a file carries, and a
//! pack says what one of them MEANS to a runtime the graph cannot see — a
//! container that constructs an annotated class, a runner that invokes an
//! annotated method, a canvas that instantiates a conforming type.
//!
//! The trigger is the gate. A marker no file in the project carries fires
//! nowhere, so a pack needs no activation rule and no manifest to read: a
//! project that does not use the framework is untouched because the evidence
//! is not there, not because something decided it was not.
//!
//! Each pack states its population in its own module doc — what it silences on
//! the pinned corpus — and a pack with none is not here.
//!
//! [`DispatchRule`]: kndo_contract::extension::DispatchRule

mod spring;
mod swiftui;

pub use spring::SpringRules;
pub use swiftui::SwiftUiRules;
