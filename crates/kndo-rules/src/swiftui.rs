//! `kndo:swiftui` — the types SwiftUI's own runtime instantiates.
//!
//! Three protocols in SwiftUI are entry points rather than interfaces a caller
//! holds: `App` is what the launcher starts, `PreviewProvider` is what Xcode's
//! canvas instantiates while you edit, and `Scene`/`View` are what the
//! framework renders. The conformance is the whole of the evidence — nothing in
//! the project constructs any of them — and each protocol's required member
//! (`body`, `previews`) is a witness of its owner, exactly as a JVM interface's
//! requirements are.
//!
//! Measured on the pinned corpus: 2 findings in Alamofire's `watchOS Example`
//! — `ContentViewPreviews` and its `previews`, a canvas-only type reported
//! `unused` with no way to say so. `@main` and `App` carry no corpus
//! population and are here because the same sentence covers them: a rule that
//! stops at the measured case would have to be widened by the first project
//! that ships an app rather than a library.

use kndo_contract::evidence::{RelationKind, RootKind};
use kndo_contract::extension::{DispatchRule, Effect, Extension, ExtensionSpec, Trigger};
use kndo_contract::vocab::Confidence;
use std::sync::LazyLock;

static SPEC: LazyLock<ExtensionSpec> = LazyLock::new(|| {
    let rules = vec![
        // The canvas instantiates a `PreviewProvider` while the file is open in
        // Xcode and never in a shipped build: TOOLING, which is what that
        // colour is for — reached, and no production promise.
        DispatchRule {
            when: Trigger::Relation {
                kind: RelationKind::Implements,
                to: "PreviewProvider".into(),
            },
            then: Effect::Root(RootKind::Tooling),
            confidence: Confidence::Certain,
        },
        // The launcher starts the `App`: production, and the one entry a
        // SwiftUI executable has.
        DispatchRule {
            when: Trigger::Relation {
                kind: RelationKind::Implements,
                to: "App".into(),
            },
            then: Effect::Root(RootKind::Production),
            confidence: Confidence::Certain,
        },
        // What each protocol REQUIRES, alive while its owner is — the
        // framework holds the conformer and reads the member through it.
        DispatchRule {
            when: Trigger::required_by("PreviewProvider", &["previews"]),
            then: Effect::Witness,
            confidence: Confidence::Certain,
        },
        DispatchRule {
            when: Trigger::required_by("App", &["body"]),
            then: Effect::Witness,
            confidence: Confidence::Certain,
        },
        DispatchRule {
            when: Trigger::required_by("View", &["body"]),
            then: Effect::Witness,
            confidence: Confidence::Certain,
        },
        DispatchRule {
            when: Trigger::required_by("Scene", &["body"]),
            then: Effect::Witness,
            confidence: Confidence::Certain,
        },
    ];
    ExtensionSpec::builder("kndo:swiftui", 1)
        .rules_for(&["kndo:swift"])
        .dispatch(rules)
        .build()
});

/// The SwiftUI pack: the runtime's entry protocols root, their requirements witness.
pub struct SwiftUiRules;

impl Extension for SwiftUiRules {
    fn spec(&self) -> &ExtensionSpec {
        &SPEC
    }
}
