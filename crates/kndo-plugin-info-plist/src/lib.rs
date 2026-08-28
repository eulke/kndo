//! `kndo:info-plist` — the built-in Apple-bundle conventions plugin.
//!
//! An app bundle's `Info.plist` names classes **as strings**, and the system instantiates them
//! at launch: `NSPrincipalClass`, `WKExtensionDelegateClassName`, the scene delegate of a
//! `UIApplicationSceneManifest`. No source file mentions those classes at all, so the whole
//! entry surface of a target reads as unreachable — Alamofire's watchOS example, where
//! `ExtensionDelegate` and everything it reaches die together.
//!
//! This is the exact shape the plugin content channel exists for: read the non-source file the
//! language graph never sees, and contribute the root. The knowledge here is Apple's bundle
//! schema and nothing else — which keys carry a class name — so it is a plugin for one concrete
//! toolchain rather than a guess about strings that look like identifiers.

use kndo_core::plugin::{
    ActivationRule, ContentView, GraphView, Plugin, PluginDescriptor, PluginTarget, RootSink,
};
use kndo_core::vocab::{Confidence, RootKind, SymbolKind};
use smol_str::SmolStr;

pub struct InfoPlistPlugin;

/// The keys whose value NAMES A CLASS the system instantiates on the project's behalf.
///
/// Curated the same way the Rust adapter's machinery-trait list is: a key earns its place by
/// naming a class a real project writes by hand and nothing in source ever mentions. Keys that
/// carry a bundle id, a version, a file name or a capability are not here — they name no code.
/// `UISceneClassName` usually names a UIKit class rather than the project's, which costs
/// nothing: a name no symbol carries roots nothing, and a project that DOES subclass the scene
/// gets its subclass rooted.
const CLASS_NAMING_KEYS: &[&str] = &[
    // Bundle principal class (macOS/iOS apps, loadable bundles).
    "NSPrincipalClass",
    // App extensions (`NSExtension` → `NSExtensionPrincipalClass`).
    "NSExtensionPrincipalClass",
    // watchOS extension delegate.
    "WKExtensionDelegateClassName",
    // Scene manifest: one delegate (and optionally scene) class per configuration.
    "UISceneDelegateClassName",
    "UISceneClassName",
    // Watch complications.
    "CLKComplicationPrincipalClass",
];

impl Plugin for InfoPlistPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("kndo:info-plist"),
            version: SmolStr::new("1"),
            // No manifest declares "Apple", so unlike every conventions plugin gated on a
            // dependency this one gates on the file itself and pays a recursive glob for it.
            // The file is the signal; there is nothing cheaper to key on — which `activation`
            // below cannot say, so it is said here.
            detection: vec![SmolStr::new(
                "an Info.plist anywhere under the project root",
            )],
            requested_file_access: vec![SmolStr::new("**/Info.plist")],
            activation: vec![ActivationRule::FileExists(SmolStr::new("**/Info.plist"))],
            dependencies: vec![],
        }
    }

    fn mutates_graph(&self) -> bool {
        true
    }

    fn contribute_roots(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut RootSink,
    ) {
        for path in content.matching_paths() {
            let Some(bytes) = content.read(path) else {
                continue;
            };
            let Ok(text) = std::str::from_utf8(&bytes) else {
                continue; // a binary plist (`bplist00`): nothing to read, contribute nothing
            };
            for name in class_names(text) {
                for target in declarations_named(graph, &name) {
                    // `Certain`, because the plist DECLARES this — it is a manifest entry
                    // point, not a naming convention someone might follow. What is soft is the
                    // name → symbol match: two targets in one repo may each declare a class of
                    // that name, and then both are rooted. That over-roots in the keep-alive
                    // direction, which is the only direction a root can err in safely.
                    out.add(target, RootKind::Production, Confidence::Certain);
                }
            }
        }
    }
}

/// Every class name the plist's class-naming keys carry.
///
/// A plist is `<key>` followed by its value as the NEXT SIBLING, at any depth — the scene
/// delegate sits four levels inside `UIApplicationSceneManifest` — so this walks every element
/// rather than a fixed path, which is what makes nesting a non-issue.
fn class_names(xml: &str) -> Vec<String> {
    // EVERY Apple plist declares the PropertyList DTD, and `roxmltree` refuses a document
    // with one unless told otherwise — parsing without this option finds nothing in the field
    // while a DOCTYPE-less fixture passes. Safe: roxmltree never resolves external entities.
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let Ok(doc) = roxmltree::Document::parse_with_options(xml, options) else {
        return Vec::new(); // not XML (a binary plist, a truncated file): degrade to silence
    };
    let mut out = Vec::new();
    for node in doc.descendants() {
        if node.tag_name().name() != "key" {
            continue;
        }
        if !node
            .text()
            .is_some_and(|k| CLASS_NAMING_KEYS.contains(&k.trim()))
        {
            continue;
        }
        let Some(value) = node
            .next_siblings()
            .find(|n| n.is_element() && n.tag_name().name() == "string")
        else {
            continue;
        };
        if let Some(name) = value.text().and_then(bare_class_name) {
            out.push(name.to_string());
        }
    }
    out
}

/// The class's own name, with the module qualifier Xcode writes stripped:
/// `$(PRODUCT_MODULE_NAME).ExtensionDelegate` and `MyApp.AppDelegate` both give
/// `ExtensionDelegate`/`AppDelegate`, and a bare `NSApplication` gives itself. A value that is
/// only a build-setting interpolation names no class and yields nothing.
fn bare_class_name(value: &str) -> Option<&str> {
    let last = value.trim().rsplit('.').next()?.trim();
    let plausible = !last.is_empty()
        && !last.contains('$')
        && last
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '`');
    plausible.then_some(last)
}

/// Every type declaration in the graph carrying this name. Types only: the plist names a class
/// to instantiate, so a same-named function is not what it meant.
fn declarations_named(graph: &GraphView<'_>, name: &str) -> Vec<PluginTarget> {
    let mut out = Vec::new();
    for file in graph.files() {
        for symbol in graph.symbols_in(&file.path) {
            let is_type = matches!(
                symbol.kind,
                SymbolKind::Class | SymbolKind::Struct | SymbolKind::Enum
            );
            if is_type && symbol.member_of.is_none() && symbol.name == name {
                out.push(PluginTarget::symbol(file.path.clone(), symbol.name.clone()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_claims_the_reserved_namespace_and_gates_on_the_file() {
        let d = InfoPlistPlugin.descriptor();
        assert_eq!(d.id, "kndo:info-plist");
        assert!(kndo_core::plugin::is_reserved_id(&d.id));
        assert_eq!(
            d.activation,
            vec![ActivationRule::FileExists(SmolStr::new("**/Info.plist"))]
        );
        assert!(InfoPlistPlugin.mutates_graph());
    }

    #[test]
    fn the_watchos_shape_that_motivated_this_yields_its_delegate() {
        // Alamofire's `watchOS Example WatchKit Extension/Info.plist`, verbatim in shape.
        // The DOCTYPE is not decoration: every Apple plist carries it, and leaving it out of
        // a fixture is what let the first version of this plugin pass its tests while finding
        // nothing in the field.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>watchOS Example</string>
    <key>WKExtensionDelegateClassName</key>
    <string>$(PRODUCT_MODULE_NAME).ExtensionDelegate</string>
</dict>
</plist>"#;
        assert_eq!(class_names(xml), vec!["ExtensionDelegate"]);
    }

    #[test]
    fn a_scene_delegate_nested_four_levels_deep_is_still_found() {
        // The reason this walks every element instead of a fixed path.
        let xml = r#"<plist><dict>
    <key>UIApplicationSceneManifest</key>
    <dict>
        <key>UISceneConfigurations</key>
        <dict>
            <key>UIWindowSceneSessionRoleApplication</key>
            <array>
                <dict>
                    <key>UISceneConfigurationName</key>
                    <string>Default Configuration</string>
                    <key>UISceneDelegateClassName</key>
                    <string>MyApp.SceneDelegate</string>
                </dict>
            </array>
        </dict>
    </dict>
</dict></plist>"#;
        assert_eq!(class_names(xml), vec!["SceneDelegate"]);
    }

    #[test]
    fn keys_that_name_no_code_contribute_nothing() {
        let xml = r#"<plist><dict>
    <key>CFBundleIdentifier</key>
    <string>org.alamofire.Example</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>NSPrincipalClass</key>
    <string></string>
    <key>UISceneDelegateClassName</key>
    <string>$(PRODUCT_MODULE_NAME)</string>
</dict></plist>"#;
        assert!(
            class_names(xml).is_empty(),
            "a bundle id, a version, an empty value and a bare build setting name no class"
        );
    }

    #[test]
    fn a_binary_or_unparseable_plist_is_silence_not_an_error() {
        assert!(class_names("bplist00\u{0}\u{1}garbage").is_empty());
        assert!(class_names("").is_empty());
    }

    #[test]
    fn the_module_qualifier_is_stripped_and_a_bare_name_survives() {
        assert_eq!(bare_class_name("$(PRODUCT_MODULE_NAME).Foo"), Some("Foo"));
        assert_eq!(bare_class_name("MyApp.Bar"), Some("Bar"));
        assert_eq!(bare_class_name("NSApplication"), Some("NSApplication"));
        assert_eq!(bare_class_name("  Spaced  "), Some("Spaced"));
        assert_eq!(bare_class_name("$(VAR)"), None);
        assert_eq!(bare_class_name(""), None);
    }
}
