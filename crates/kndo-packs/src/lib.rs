//! What a FRAMEWORK means, as data.
//!
//! A rule pack is a conduct extension that claims no files, runs no code and
//! declares nothing but its [`Activation`] and its [`DispatchRule`]s. It owns
//! no language and parses nothing: the language adapter already reported the
//! markers and relations a file carries, and a pack says what one of them
//! MEANS to a runtime the graph cannot see — a container that constructs an
//! annotated class, a runner that invokes an annotated method, a canvas that
//! instantiates a conforming type.
//!
//! Two gates, and both are load-bearing. ACTIVATION decides whether the
//! project uses this framework at all, from what its manifests declare or what
//! its files import; the pack is inert everywhere else, and appears in the
//! run's contributions where it is not. QUALIFICATION decides whether a
//! particular marker is this framework's: a rule names the full path, and the
//! engine resolves the marker a file carries through that file's own bindings
//! before comparing. Neither gate is redundant — a bare name is the same six
//! letters in two ecosystems, and a trigger alone would let a JVM rule speak
//! about a Swift file.
//!
//! Each pack states its measured population in its own module doc, and every
//! shipped pack carries the baseline-then-pack proof `builtin_conduct_proofs`
//! demands of every conducting coordinate.
//!
//! [`Activation`]: kndo_contract::extension::Activation
//! [`DispatchRule`]: kndo_contract::extension::DispatchRule

mod spring;

pub use spring::SpringRules;
