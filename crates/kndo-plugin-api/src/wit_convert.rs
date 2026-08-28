//! See the module declaration in `lib.rs` for why these are macros rather than functions.

/// Defines `from_wit_activation_rule` over the given generated bindings module. The
/// `activation-rule` record is identical in the `adapter` and `plugin` worlds; the generated
/// enums are not the same Rust type, so the conversion is expanded per world instead of
/// copied per world.
macro_rules! wit_activation_rule_conversion {
    ($w:ident) => {
        fn from_wit_activation_rule(rule: $w::ActivationRule) -> kndo_core::plugin::ActivationRule {
            match rule {
                $w::ActivationRule::FileExists(glob) => {
                    kndo_core::plugin::ActivationRule::FileExists(SmolStr::new(&glob))
                }
                $w::ActivationRule::ManifestDependency(name) => {
                    kndo_core::plugin::ActivationRule::ManifestDependency(SmolStr::new(&name))
                }
            }
        }
    };
}
