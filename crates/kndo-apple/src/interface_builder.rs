//! `kndo:interface-builder` — the classes, outlets and actions an Interface
//! Builder document wires up.
//!
//! A storyboard or xib names a class as a string (`customClass="…"`), the
//! runtime instantiates it, and the connections panel binds outlet properties
//! and action methods by name. Every one of those is a reference living outside
//! the code graph: without it the class is a type nothing constructs and its
//! outlets are members nothing outside their file touches — `unused` and
//! `internal-only`, exactly wrong, since making an outlet private breaks the
//! connection.
//!
//! What is read here is the DOCUMENT FORMAT, one format whichever runtime the
//! document targets (`targetRuntime` is `iOS.CocoaTouch`, `watchKit` or
//! `MacOSX.Cocoa`): `customClass` is instantiated by the runtime and
//! connections bind by name under all three, which is why the coordinate names
//! the editor that writes the file rather than one framework that runs it. The
//! measured cases are Alamofire's iOS `Main.storyboard` (`MasterViewController`
//! and its `titleImageView` outlet) and its watchKit `Interface.storyboard`
//! (`HostingController`).

use crate::names::TypeIndex;
use kndo_contract::evidence::RootKind;
use kndo_contract::plugin::{
    Activation, ActivationRule, ContentAccess, GraphAccess, MutatesGraph, Plugin, PluginSink,
    PluginSpec, PluginTarget,
};
use kndo_contract::vocab::Confidence;
use smol_str::SmolStr;
use std::collections::HashMap;
use std::sync::LazyLock;

static SPEC: LazyLock<PluginSpec> = LazyLock::new(|| {
    PluginSpec::builder("kndo:interface-builder", 1)
        // On the documents, never on a manifest dependency: the frameworks that
        // run them ship with the platform, so no manifest ever declares them —
        // the documents are the only signal a project has any.
        .conduct(
            Activation::AnyRule(vec![
                ActivationRule::FileExists(SmolStr::new_static("**/*.storyboard")),
                ActivationRule::FileExists(SmolStr::new_static("**/*.xib")),
            ]),
            MutatesGraph::Yes,
        )
        .requested_file_access(&["**/*.storyboard", "**/*.xib"])
        .build()
});

pub struct InterfaceBuilderPlugin;

impl Plugin for InterfaceBuilderPlugin {
    fn spec(&self) -> &PluginSpec {
        &SPEC
    }

    /// Every class a document instantiates and every member it connects,
    /// resolved by name against the graph's own declarations. `Probable`: the
    /// document's claim is certain, but a bare name is all it gives and the
    /// graph knows no module to match it by — a wrong match keeps alive what
    /// was already alive, and can never accuse.
    fn contribute_roots(
        &self,
        graph: &dyn GraphAccess,
        content: &dyn ContentAccess,
        out: &mut PluginSink,
    ) {
        let types = TypeIndex::build(graph);
        for path in content.readable_paths() {
            let Some(document) = content
                .read(path)
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .and_then(parse_document)
            else {
                continue;
            };
            for class in &document.classes {
                for file in types.nearest(class, path) {
                    out.root(
                        PluginTarget::Symbol {
                            path: file.clone(),
                            name: SmolStr::new(class),
                        },
                        RootKind::Production,
                        Confidence::Probable,
                    );
                }
            }
            for connection in &document.connections {
                for file in types.nearest(&connection.owner, path) {
                    if types.declares_member(file, &connection.owner, &connection.member) {
                        out.root(
                            PluginTarget::Symbol {
                                path: file.clone(),
                                name: SmolStr::new(&connection.member),
                            },
                            RootKind::Production,
                            Confidence::Probable,
                        );
                    }
                }
            }
        }
    }
}

/// One document, reduced to the two things it knows that the code does not.
struct Document {
    /// Every distinct `customClass`, in document order.
    classes: Vec<String>,
    connections: Vec<Connection>,
}

/// One connection the document declares: a member of `owner`, bound by name.
struct Connection {
    owner: String,
    member: String,
}

/// A document's XML into the facts it holds, or `None` when it is not this
/// plugin's to read. These are editor-generated files: a parse failure is a
/// format this plugin does not understand, never a project defect to accuse
/// anyone of, so it says nothing.
fn parse_document(text: &str) -> Option<Document> {
    let doc = roxmltree::Document::parse(text).ok()?;
    let root = doc.root_element();
    if root.tag_name().name() != "document"
        || !root
            .attribute("type")
            .is_some_and(|t| t.starts_with("com.apple.InterfaceBuilder"))
    {
        return None;
    }
    // An `<action>` names the object that implements its selector by id, so
    // the id → class map must be complete before the walk.
    let class_by_id: HashMap<&str, &str> = doc
        .descendants()
        .filter_map(|n| Some((n.attribute("id")?, n.attribute("customClass")?)))
        .collect();

    let mut classes: Vec<String> = Vec::new();
    let mut connections = Vec::new();
    for node in doc.descendants().filter(roxmltree::Node::is_element) {
        if let Some(class) = node.attribute("customClass")
            && !classes.iter().any(|c| c == class)
        {
            classes.push(class.to_string());
        }
        match node.tag_name().name() {
            // An outlet is a property of the object whose `<connections>` hold
            // it — the outlet's grandparent. An object without a `customClass`
            // is the framework's own (a table view wiring its `dataSource`),
            // and names no code of the project's. `outletCollection` is the
            // plural form of the same binding.
            "outlet" | "outletCollection" => {
                let owner = node
                    .parent()
                    .and_then(|connections| connections.parent())
                    .and_then(|object| object.attribute("customClass"));
                if let (Some(owner), Some(property)) = (owner, node.attribute("property")) {
                    connections.push(Connection {
                        owner: owner.to_string(),
                        member: property.to_string(),
                    });
                }
            }
            // The destination implements the selector. The selector is
            // Objective-C's — `doThing:withValue:` for a method Swift declares
            // as `doThing(_:withValue:)` — and its first segment is the
            // declaration's name in every arity, the no-argument `onTap`
            // included. A destination without a class (the first-responder
            // placeholder) names nothing.
            "action" => {
                let owner = node
                    .attribute("destination")
                    .and_then(|id| class_by_id.get(id).copied());
                let name = node
                    .attribute("selector")
                    .map(|s| s.split(':').next().unwrap_or(s))
                    .filter(|n| !n.is_empty());
                if let (Some(owner), Some(name)) = (owner, name) {
                    connections.push(Connection {
                        owner: owner.to_string(),
                        member: name.to_string(),
                    });
                }
            }
            _ => {}
        }
    }
    Some(Document {
        classes,
        connections,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connections(document: &Document) -> Vec<(&str, &str)> {
        document
            .connections
            .iter()
            .map(|c| (c.owner.as_str(), c.member.as_str()))
            .collect()
    }

    /// Alamofire's `Main.storyboard`, trimmed to the nesting that decides
    /// ownership: the table view's own outlets sit under an object without a
    /// class, the controller's outlet under the controller.
    const IOS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<document type="com.apple.InterfaceBuilder3.CocoaTouch.Storyboard.XIB" version="3.0" targetRuntime="iOS.CocoaTouch">
  <scenes>
    <scene sceneID="smW-Zh-WAh">
      <objects>
        <tableViewController id="7bK-jq-Zjz" customClass="MasterViewController" customModule="iOS_Example">
          <tableView key="view" id="r7i-6Z-zg0">
            <connections>
              <outlet property="dataSource" destination="7bK-jq-Zjz" id="Gho-Na-rnu"/>
              <outlet property="delegate" destination="7bK-jq-Zjz" id="RA6-mI-bju"/>
            </connections>
          </tableView>
          <connections>
            <outlet property="titleImageView" destination="9c8-WZ-jVF" id="jvG-Sa-nSG"/>
          </connections>
        </tableViewController>
        <placeholder placeholderIdentifier="IBFirstResponder" id="Rux-fX-hf1" sceneMemberID="firstResponder"/>
      </objects>
    </scene>
  </scenes>
</document>"#;

    #[test]
    fn the_object_holding_the_connections_owns_the_outlet() {
        let document = parse_document(IOS).expect("an Interface Builder document");
        assert_eq!(document.classes, ["MasterViewController"]);
        assert_eq!(
            connections(&document),
            [("MasterViewController", "titleImageView")],
            "the table view's dataSource/delegate are the framework's, not the app's"
        );
    }

    #[test]
    fn an_action_names_its_destination_by_id_and_its_method_by_selector_head() {
        let xml = r#"<document type="com.apple.InterfaceBuilder3.CocoaTouch.Storyboard.XIB" targetRuntime="iOS.CocoaTouch">
  <objects>
    <viewController id="vc1" customClass="DetailViewController">
      <view key="view" id="v1">
        <subviews>
          <button id="b1">
            <connections>
              <action selector="refreshTapped:" destination="vc1" eventType="touchUpInside" id="a1"/>
              <action selector="dismiss:" destination="fr" eventType="touchUpInside" id="a2"/>
              <action selector="onTap" destination="vc1" eventType="touchUpInside" id="a3"/>
            </connections>
          </button>
        </subviews>
      </view>
    </viewController>
    <placeholder placeholderIdentifier="IBFirstResponder" id="fr" sceneMemberID="firstResponder"/>
  </objects>
</document>"#;
        let document = parse_document(xml).expect("parses");
        assert_eq!(
            connections(&document),
            [
                ("DetailViewController", "refreshTapped"),
                ("DetailViewController", "onTap")
            ],
            "the first responder implements no code of the project's"
        );
    }

    #[test]
    fn every_runtime_the_editor_writes_for_is_read_and_nothing_else_is() {
        let watch = r#"<document type="com.apple.InterfaceBuilder.WatchKit.Storyboard" version="3.0" targetRuntime="watchKit">
  <scenes><scene sceneID="s"><objects>
    <hostingController id="h" customClass="HostingController" customModuleProvider="target"/>
  </objects></scene></scenes>
</document>"#;
        assert_eq!(
            parse_document(watch).expect("a WatchKit document").classes,
            ["HostingController"]
        );
        let plist = r#"<plist version="1.0"><dict><key>NSPrincipalClass</key><string>App</string></dict></plist>"#;
        assert!(
            parse_document(plist).is_none(),
            "not an Interface Builder document"
        );
        assert!(
            parse_document("<document>").is_none(),
            "unparseable: silence"
        );
    }
}
