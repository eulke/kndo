//! `kndo:info-plist` — the classes an Apple bundle's `Info.plist` names for the
//! system to instantiate at launch.
//!
//! `NSPrincipalClass`, `WKExtensionDelegateClassName`, the scene delegate of a
//! `UIApplicationSceneManifest`: the value is a class name as a STRING, no
//! source file mentions the class, and so the whole entry surface of a target
//! reads as unreachable — Alamofire's watchOS extension, where
//! `ExtensionDelegate` and everything it reaches die together. The knowledge
//! here is Apple's bundle schema and nothing else: which keys carry a class
//! name.

use crate::names::TypeIndex;
use kndo_contract::evidence::RootKind;
use kndo_contract::extension::{
    Activation, ActivationRule, ConductSink, ConductTarget, ContentAccess, Extension,
    ExtensionSpec, GraphAccess, MutatesGraph,
};
use kndo_contract::vocab::Confidence;
use smol_str::SmolStr;
use std::sync::LazyLock;

/// The keys whose value NAMES A CLASS the system instantiates on the project's
/// behalf. A key earns its place by naming a class a real project writes by
/// hand and nothing in source ever mentions; keys carrying a bundle id, a
/// version, a file name or a capability name no code. `UISceneClassName`
/// usually names a UIKit class, which costs nothing: a name no declaration
/// carries roots nothing, and a project that subclasses the scene gets its
/// subclass rooted.
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

static SPEC: LazyLock<ExtensionSpec> = LazyLock::new(|| {
    ExtensionSpec::builder("kndo:info-plist", 1)
        // No manifest declares "Apple": the file itself is the signal, and a
        // recursive glob is the cheapest thing that can see it.
        .conduct(
            Activation::AnyRule(vec![ActivationRule::FileExists(SmolStr::new_static(
                "**/Info.plist",
            ))]),
            MutatesGraph::Yes,
        )
        .requested_file_access(&["**/Info.plist"])
        .build()
});

pub struct InfoPlistPlugin;

impl Extension for InfoPlistPlugin {
    fn spec(&self) -> &ExtensionSpec {
        &SPEC
    }

    /// `Probable` for the same reason as an Interface Builder class: the plist's
    /// claim is certain, the match from a bare name to a declaration is by name
    /// alone, and a wrong match can only keep something alive.
    fn contribute_roots(
        &self,
        graph: &dyn GraphAccess,
        content: &dyn ContentAccess,
        out: &mut ConductSink,
    ) {
        let types = TypeIndex::build(graph);
        for path in content.readable_paths() {
            // A binary plist (`bplist00`) is not text: nothing to read here.
            let Some(text) = content
                .read(path)
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
            else {
                continue;
            };
            for name in class_names(text) {
                for file in types.nearest(&name, path) {
                    out.root(
                        ConductTarget::Symbol {
                            path: file.clone(),
                            name: SmolStr::new(&name),
                        },
                        RootKind::Production,
                        Confidence::Probable,
                    );
                }
            }
        }
    }
}

/// Every class name the plist's class-naming keys carry. A plist is `<key>`
/// followed by its value as the NEXT SIBLING, at any depth — the scene delegate
/// sits four levels inside `UIApplicationSceneManifest` — so this walks every
/// element rather than a fixed path.
fn class_names(xml: &str) -> Vec<String> {
    // Every Apple plist declares the PropertyList DTD, and roxmltree refuses a
    // document with one unless told otherwise. Safe: it never resolves external
    // entities.
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let Ok(doc) = roxmltree::Document::parse_with_options(xml, options) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for node in doc.descendants() {
        if node.tag_name().name() != "key"
            || !node
                .text()
                .is_some_and(|k| CLASS_NAMING_KEYS.contains(&k.trim()))
        {
            continue;
        }
        let value = node
            .next_siblings()
            .find(|n| n.is_element() && n.tag_name().name() == "string");
        if let Some(name) = value.and_then(|v| v.text()).and_then(bare_class_name) {
            out.push(name.to_string());
        }
    }
    out
}

/// The class's own name, with the module qualifier Xcode writes stripped:
/// `$(PRODUCT_MODULE_NAME).ExtensionDelegate` and `MyApp.AppDelegate` both give
/// the last segment, and a bare `NSApplication` gives itself. A value that is
/// only a build-setting interpolation names no class.
fn bare_class_name(value: &str) -> Option<&str> {
    let last = value.trim().rsplit('.').next()?.trim();
    let plausible = !last.is_empty()
        && !last.contains('$')
        && last
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '`');
    plausible.then_some(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Alamofire's `watchOS Example WatchKit Extension/Info.plist`, in shape.
    /// The DOCTYPE is not decoration: every Apple plist carries it, so a fixture
    /// without one proves nothing about a real plist.
    #[test]
    fn the_watchos_delegate_is_named_through_its_module_qualifier() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>$(PRODUCT_NAME)</string>
    <key>WKExtensionDelegateClassName</key>
    <string>$(PRODUCT_MODULE_NAME).ExtensionDelegate</string>
</dict>
</plist>"#;
        assert_eq!(class_names(xml), ["ExtensionDelegate"]);
    }

    #[test]
    fn the_scene_delegate_is_found_at_any_depth() {
        let xml = r#"<plist version="1.0"><dict>
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
          <string>$(PRODUCT_MODULE_NAME).SceneDelegate</string>
        </dict>
      </array>
    </dict>
  </dict>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
</dict></plist>"#;
        assert_eq!(class_names(xml), ["SceneDelegate", "NSApplication"]);
    }

    #[test]
    fn a_value_that_names_no_class_yields_nothing() {
        assert_eq!(bare_class_name("$(PRODUCT_MODULE_NAME)"), None);
        assert_eq!(bare_class_name("  "), None);
        assert_eq!(bare_class_name("MyApp.AppDelegate"), Some("AppDelegate"));
        assert!(
            class_names("bplist00\x00\x01").is_empty(),
            "binary: silence"
        );
    }
}
