//! `kndo:uikit` — the classes, outlets and actions an Interface Builder document wires up.
//!
//! A UIKit app's view controllers are never constructed by any line of Swift. The storyboard
//! names the class as a string (`customClass="DetailViewController"`), UIKit instantiates it
//! at runtime, and the connections panel binds `@IBOutlet` properties and `@IBAction` methods
//! by name. Every one of those is a reference living entirely outside the code graph, so kndo
//! sees a class nothing constructs and members nothing touches outside their own file — and
//! reports them.
//!
//! Measured before this existed: Kingfisher's demo app carries 27 outlets across 15 classes,
//! and kndo called `GIFViewController.imageView`, `DetailImageViewController.infoLabel`,
//! `TransitionViewController.transitionPickerView` and their siblings `internal-only`
//! ("private would suffice"), which is exactly wrong — `private` breaks the connection. In
//! Alamofire the same shape hit `MasterViewController.titleImageView`.
//!
//! This plugin reads the XML those documents already are and nothing else. It parses no
//! Swift, resolves no module, and asserts nothing about a name it cannot point at a real
//! declaration for.
//!
//! ## Why `kndo:uikit` and not `kndo:interface-builder`
//!
//! Interface Builder is the editor; UIKit is what runs the file. A `.storyboard` is also how
//! AppKit and WatchKit apps are laid out, and the three do not share a class hierarchy, a
//! member vocabulary, or the same answer to "who instantiates this". The document itself
//! draws the line — `targetRuntime="iOS.CocoaTouch"` vs `MacOSX.Cocoa` vs `watchKit` — so
//! this plugin reads only the first and leaves the other two to `kndo:appkit` and
//! `kndo:watchkit` siblings, each of which must arrive with its own measured case rather
//! than be assumed into existence here. Alamofire's watchOS `Interface.storyboard` is
//! deliberately untouched for that reason.

use std::collections::HashMap;

use kndo_core::adapter::ProjectPath;
use kndo_core::plugin::{
    ActivationRule, ContentView, EdgeSink, GraphView, Plugin, PluginDescriptor, PluginTarget,
    RootSink,
};
use kndo_core::vocab::{Confidence, RefKind, RootKind};
use smol_str::SmolStr;

pub struct UikitPlugin;

/// The runtime this plugin speaks for, as Interface Builder spells it in the document's root
/// element. AppKit (`MacOSX.Cocoa`) and WatchKit (`watchKit`) documents are the same file
/// format and a different framework; skipping them here is what keeps the id honest.
const UIKIT_RUNTIME: &str = "iOS.CocoaTouch";

impl Plugin for UikitPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("kndo:uikit"),
            version: SmolStr::new("1"),
            // Empty: the rules below ARE the gate, so prose beside them would restate them.
            detection: vec![],
            requested_file_access: vec![SmolStr::new("**/*.storyboard"), SmolStr::new("**/*.xib")],
            // On the files, not on a manifest dependency: UIKit is a platform framework, so
            // no `Package.swift` or `Podfile` ever declares it — the documents are the only
            // signal a project uses it at all.
            activation: vec![
                ActivationRule::FileExists(SmolStr::new("**/*.storyboard")),
                ActivationRule::FileExists(SmolStr::new("**/*.xib")),
            ],
            dependencies: vec![],
        }
    }

    fn mutates_graph(&self) -> bool {
        true
    }

    /// UIKit constructs the class named by `customClass` — nothing in the app does. That is a
    /// production root in the plainest sense: the app's own entry point reaches it and no
    /// call site exists to prove it.
    ///
    /// `Probable`, not `Certain`: the match is by name against the declarations kndo found,
    /// and a document can name a class from a framework or another module. A wrong match
    /// keeps something alive that was already alive; it cannot accuse.
    fn contribute_roots(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut RootSink,
    ) {
        for wiring in wiring(graph, content) {
            for target in wiring.instantiated {
                out.add(target, RootKind::Production, Confidence::Probable);
            }
        }
    }

    /// The document references what it wires: the class it instantiates, and each member it
    /// connects. Edges rather than only roots, because a root answers "is this alive" and the
    /// false positives measured here were `internal-only` — "nothing outside this file uses
    /// it" — which only a reference from the document can answer.
    fn contribute_edges(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut EdgeSink,
    ) {
        for wiring in wiring(graph, content) {
            let from = PluginTarget::file(wiring.document);
            for target in wiring.instantiated {
                out.add(from.clone(), target, RefKind::TypeUse, Confidence::Probable);
            }
            for (target, kind) in wiring.connected {
                out.add(from.clone(), target, kind, Confidence::Probable);
            }
        }
    }
}

/// What one document wires, resolved against the graph — the plugin's whole answer, computed
/// in one place and projected differently by each hook.
///
/// Both graph-mutation hooks need the same derivation (read every Interface Builder document,
/// resolve every name against the declarations kndo found), and the `Plugin` trait offers no
/// per-run scratch space to share it through. A **native** plugin cannot simply cache one in
/// itself either: the trait is `Send + Sync` and every hook takes `&self`, so a cache is
/// shared mutable state behind a lock, holding one run's answer on a value the engine reuses.
/// (A WASM guest is a different case — RFC 0017 §4 gives it one instance per round and statics
/// across the three hooks are contractual there. The native trait makes no such promise, and a
/// built-in must not read as if it did.)
///
/// So: a pure function both hooks call. The derivation runs twice per run and that is the
/// accepted cost, measured — Kingfisher's six documents are 215 KB of XML, and parsing them
/// twice plus building the declaration index twice is a few milliseconds inside a 220 ms run.
/// What the shared function buys is the thing that actually costs: **one description of the
/// wiring**, so the two hooks cannot drift, and so `kndo:vite` and `kndo:rollup` — the same
/// two-hooks-one-input shape — copy a structure rather than a duplication.
///
/// If a plugin ever appears whose derivation is expensive enough to matter, a `prepare` hook
/// is the answer, and it is a deliberate contract change (the native trait AND the WIT world),
/// not something to smuggle in behind a lock.
struct Wiring {
    document: ProjectPath,
    /// The classes UIKit instantiates from this document, already resolved to declarations.
    instantiated: Vec<PluginTarget>,
    /// Each `@IBOutlet`/`@IBAction` the document connects, with the reference kind it implies.
    connected: Vec<(PluginTarget, RefKind)>,
}

fn wiring(graph: &GraphView<'_>, content: &ContentView<'_>) -> Vec<Wiring> {
    let classes = declared_classes(graph);
    let mut out = Vec::new();
    for document in uikit_documents(content) {
        let mut instantiated = Vec::new();
        for class in &document.classes {
            for path in nearest_declarations(&classes, class, &document.path) {
                instantiated.push(PluginTarget::symbol(path.clone(), class.as_str()));
            }
        }
        let mut connected = Vec::new();
        for connection in &document.connections {
            for path in nearest_declarations(&classes, &connection.owner, &document.path) {
                // `Owner.member` is the qualified spelling `PluginTarget` resolves against; an
                // unresolvable one is dropped core-side, which is what happens to every
                // `dataSource`/`delegate` outlet a document declares on a UIKit view rather
                // than on the app's own class.
                connected.push((
                    PluginTarget::symbol(
                        path.clone(),
                        format!("{}.{}", connection.owner, connection.member),
                    ),
                    connection.kind,
                ));
            }
        }
        out.push(Wiring {
            document: document.path,
            instantiated,
            connected,
        });
    }
    out
}

/// One connection the document declares: a member of `owner`, bound by name.
struct Connection {
    owner: String,
    member: String,
    kind: RefKind,
}

/// One Interface Builder document, reduced to the two things it knows that the code does not.
struct Document {
    path: ProjectPath,
    /// Every distinct `customClass`, in document order.
    classes: Vec<String>,
    connections: Vec<Connection>,
}

/// Every accessible `.storyboard`/`.xib` whose root element declares the UIKit runtime.
fn uikit_documents(content: &ContentView<'_>) -> Vec<Document> {
    let mut out = Vec::new();
    for path in content.matching_paths() {
        let Some(bytes) = content.read(path) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        if let Some(document) = parse_document(path.clone(), text) {
            out.push(document);
        }
    }
    out
}

/// One document's XML into the two facts it holds, or `None` when it is not this plugin's to
/// read. A document kndo cannot parse says nothing and reports nothing: these are
/// editor-generated files, so a parse failure means a format this plugin does not understand,
/// never a project defect worth accusing anyone of.
fn parse_document(path: ProjectPath, text: &str) -> Option<Document> {
    let doc = roxmltree::Document::parse(text).ok()?;
    if doc.root_element().attribute("targetRuntime") != Some(UIKIT_RUNTIME) {
        return None;
    }
    Some(read_document(path, &doc))
}

fn read_document(path: ProjectPath, doc: &roxmltree::Document<'_>) -> Document {
    // An `<action>` names the object that IMPLEMENTS the selector through `destination`,
    // while an `<outlet>` names a property of the object it is nested in. Both need this map;
    // only the first needs it to be complete before the walk, hence two passes.
    let mut class_by_id: HashMap<&str, &str> = HashMap::new();
    for node in doc.descendants() {
        if let (Some(id), Some(class)) = (node.attribute("id"), node.attribute("customClass")) {
            class_by_id.insert(id, class);
        }
    }

    let mut classes: Vec<String> = Vec::new();
    let mut connections = Vec::new();
    walk(
        doc.root_element(),
        None,
        &class_by_id,
        &mut classes,
        &mut connections,
    );
    Document {
        path,
        classes,
        connections,
    }
}

fn walk<'a>(
    node: roxmltree::Node<'a, 'a>,
    owner: Option<&'a str>,
    class_by_id: &HashMap<&'a str, &'a str>,
    classes: &mut Vec<String>,
    connections: &mut Vec<Connection>,
) {
    // The nearest enclosing `customClass` owns everything under it — the nesting IS the
    // ownership, which is why this is a walk rather than a flat scan.
    let owner = match node.attribute("customClass") {
        Some(class) => {
            if !classes.iter().any(|c| c == class) {
                classes.push(class.to_string());
            }
            Some(class)
        }
        None => owner,
    };

    match node.tag_name().name() {
        // `outletCollection` is the plural form of the same binding — one property, several
        // destinations — and reaches the property exactly the same way.
        "outlet" | "outletCollection" => {
            if let (Some(owner), Some(property)) = (owner, node.attribute("property")) {
                connections.push(Connection {
                    owner: owner.to_string(),
                    member: property.to_string(),
                    kind: RefKind::Read,
                });
            }
        }
        "action" => {
            // The selector is ObjC's: `doThing:withValue:` for a method Swift declares as
            // `doThing(_:withValue:)` and kndo records as `doThing`. The first segment is the
            // declaration's name in every arity, including the no-argument `onTapButton`.
            let target = node
                .attribute("destination")
                .and_then(|id| class_by_id.get(id).copied())
                .or(owner);
            if let (Some(target), Some(selector)) = (target, node.attribute("selector")) {
                let name = selector.split(':').next().unwrap_or(selector);
                if !name.is_empty() {
                    connections.push(Connection {
                        owner: target.to_string(),
                        member: name.to_string(),
                        kind: RefKind::Call,
                    });
                }
            }
        }
        _ => {}
    }

    for child in node.children().filter(roxmltree::Node::is_element) {
        walk(child, owner, class_by_id, classes, connections);
    }
}

/// Top-level declaration name → the files declaring it. A storyboard names a class as a bare
/// string with no path, so the only way to point a contribution at it is by name.
fn declared_classes<'a>(graph: &GraphView<'a>) -> HashMap<&'a str, Vec<&'a ProjectPath>> {
    let mut out: HashMap<&str, Vec<&ProjectPath>> = HashMap::new();
    for file in graph.files() {
        for symbol in graph.symbols_in(&file.path) {
            if symbol.member_of.is_none() {
                out.entry(symbol.name.as_str())
                    .or_default()
                    .push(&file.path);
            }
        }
    }
    out
}

/// The declarations of `class` **closest to the document** — every match under the deepest
/// ancestor directory of the document that contains one, falling back to every match when
/// none shares an ancestor short of the project root.
///
/// A name is all the document gives, and one app can declare the same name twice. Kingfisher
/// is the worked case: `SwiftUIViewController` exists in both `Kingfisher-Demo/` and
/// `Kingfisher-macOS-Demo/`, and the iOS storyboard naming it must not speak for the macOS
/// one — that is `kndo:appkit`'s document to read, from its own file, which this plugin
/// deliberately skips. Proximity is the only discriminator both sides of the question can
/// see: the document itself carries `customModule="Kingfisher_Demo"`, but a module is not
/// something kndo's graph knows for Swift, so a rule written on it would be unverifiable.
///
/// The fallback is the keep-alive direction (RFC 0012 §2): with nothing to choose between
/// candidates, contributing to all of them can only keep something alive, never accuse it.
fn nearest_declarations<'a>(
    classes: &HashMap<&str, Vec<&'a ProjectPath>>,
    class: &str,
    document: &ProjectPath,
) -> Vec<&'a ProjectPath> {
    let Some(candidates) = classes.get(class) else {
        return Vec::new();
    };
    let mut dir = parent_dir(document.0.as_str());
    loop {
        let near: Vec<&ProjectPath> = candidates
            .iter()
            .copied()
            .filter(|p| under(dir, p.0.as_str()))
            .collect();
        if !near.is_empty() {
            return near;
        }
        match dir {
            "" => return candidates.clone(),
            _ => dir = parent_dir(dir),
        }
    }
}

fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

fn under(dir: &str, path: &str) -> bool {
    dir.is_empty() || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Option<Document> {
        parse_document(ProjectPath(SmolStr::new("Main.storyboard")), text)
    }

    /// Alamofire's own `Main.storyboard`, trimmed to the nesting that decides ownership: the
    /// outlet sits several elements below the controller that owns it.
    const ALAMOFIRE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<document type="com.apple.InterfaceBuilder3.CocoaTouch.Storyboard.XIB" targetRuntime="iOS.CocoaTouch">
  <scenes>
    <scene sceneID="smW-Zh-WAh">
      <objects>
        <tableViewController id="7bK-jq-Zjz" customClass="MasterViewController" customModule="iOS_Example">
          <tableView key="view" id="r7i-6Z-zg0">
            <connections>
              <outlet property="dataSource" destination="7bK-jq-Zjz" id="Gho-Na-rnu"/>
              <outlet property="titleImageView" destination="9c8-WZ-jVF" id="jvG-Sa-nSG"/>
            </connections>
          </tableView>
        </tableViewController>
      </objects>
    </scene>
  </scenes>
</document>"#;

    /// Kingfisher's `Main.storyboard`, trimmed to its one `<action>`: the selector's owner is
    /// the `destination` object, not the button the connection is written inside.
    const KINGFISHER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<document type="com.apple.InterfaceBuilder3.CocoaTouch.Storyboard.XIB" targetRuntime="iOS.CocoaTouch">
  <scenes>
    <scene>
      <objects>
        <viewController id="wco-eY-gNu" customClass="PHPickerResultViewController">
          <view key="view">
            <subviews>
              <button id="btn-1">
                <connections>
                  <action selector="onTapButton" destination="wco-eY-gNu" eventType="touchUpInside" id="QyB-8Y-1Oc"/>
                </connections>
              </button>
            </subviews>
            <connections>
              <outlet property="imageView" destination="img-1" id="o-1"/>
            </connections>
          </view>
        </viewController>
      </objects>
    </scene>
  </scenes>
</document>"#;

    #[test]
    fn the_descriptor_claims_the_reserved_namespace_and_gates_on_the_documents() {
        let d = UikitPlugin.descriptor();
        assert_eq!(d.id, "kndo:uikit");
        assert!(kndo_core::plugin::is_reserved_id(&d.id));
        assert_eq!(
            d.activation,
            vec![
                ActivationRule::FileExists(SmolStr::new("**/*.storyboard")),
                ActivationRule::FileExists(SmolStr::new("**/*.xib")),
            ],
            "UIKit is a platform framework — no manifest ever declares it, so the documents \
             are the only signal"
        );
        assert!(UikitPlugin.mutates_graph());
    }

    #[test]
    fn an_outlet_belongs_to_the_nearest_enclosing_custom_class() {
        // The nesting IS the ownership: `titleImageView` is written three elements below the
        // controller that declares it, and a flat scan would have no owner to attribute it to.
        let doc = parse(ALAMOFIRE).expect("an iOS document");
        assert_eq!(doc.classes, vec!["MasterViewController".to_string()]);
        let members: Vec<(&str, &str)> = doc
            .connections
            .iter()
            .map(|c| (c.owner.as_str(), c.member.as_str()))
            .collect();
        assert_eq!(
            members,
            vec![
                ("MasterViewController", "dataSource"),
                ("MasterViewController", "titleImageView"),
            ],
            "`dataSource` is UIKit's own and resolves to no declaration — dropped core-side, \
             never a reason to skip the outlet that matters"
        );
    }

    #[test]
    fn an_action_belongs_to_its_destination_not_to_the_button_it_is_written_in() {
        // The one place outlets and actions genuinely differ. Attributing the selector to the
        // enclosing `<button>` would resolve to nothing and leave `@IBAction func onTapButton`
        // reported as unreachable — the exact finding this plugin exists to remove.
        let doc = parse(KINGFISHER).expect("an iOS document");
        let actions: Vec<(&str, &str)> = doc
            .connections
            .iter()
            .filter(|c| c.kind == RefKind::Call)
            .map(|c| (c.owner.as_str(), c.member.as_str()))
            .collect();
        assert_eq!(
            actions,
            vec![("PHPickerResultViewController", "onTapButton")]
        );
    }

    #[test]
    fn an_objc_selector_reduces_to_the_declaration_name() {
        // `doThing:withValue:` is one method Swift declares as `doThing(_:withValue:)` and
        // kndo records as `doThing`. Matching the whole selector would match nothing.
        let doc = parse(&KINGFISHER.replace("onTapButton", "doThing:withValue:")).unwrap();
        assert!(doc
            .connections
            .iter()
            .any(|c| c.kind == RefKind::Call && c.member == "doThing"));
    }

    #[test]
    fn a_watchkit_or_appkit_document_is_not_this_plugins_to_read() {
        // The honest discriminator, and the reason the id is a framework rather than an
        // editor. Alamofire's watchOS `Interface.storyboard` names `HostingController` and
        // this plugin must not claim to know what WatchKit does with it.
        for runtime in ["watchKit", "MacOSX.Cocoa"] {
            let text = ALAMOFIRE.replace("iOS.CocoaTouch", runtime);
            assert!(parse(&text).is_none(), "{runtime} is a sibling's business");
        }
    }

    #[test]
    fn one_name_two_targets_resolves_to_the_declaration_beside_the_document() {
        // Kingfisher's own ambiguity: `SwiftUIViewController` exists in both demo targets, and
        // the iOS storyboard that names it must not speak for the macOS one — that document is
        // `kndo:appkit`'s to read, and this plugin skips it. Proximity is the only
        // discriminator both sides can see; `customModule` is a Swift module, which kndo's
        // graph does not model.
        let ios = ProjectPath(SmolStr::new(
            "Demo/Demo/Kingfisher-Demo/ViewControllers/SwiftUIViewController.swift",
        ));
        let macos = ProjectPath(SmolStr::new(
            "Demo/Demo/Kingfisher-macOS-Demo/SwiftUIViewController.swift",
        ));
        let mut classes = HashMap::new();
        classes.insert("SwiftUIViewController", vec![&ios, &macos]);

        let document = ProjectPath(SmolStr::new(
            "Demo/Demo/Kingfisher-Demo/Base.lproj/Main.storyboard",
        ));
        assert_eq!(
            nearest_declarations(&classes, "SwiftUIViewController", &document),
            vec![&ios],
            "the storyboard's own target directory contains exactly one of the two"
        );
    }

    #[test]
    fn a_name_with_no_close_declaration_falls_back_to_every_match() {
        // With nothing to choose between candidates, contributing to all of them can only keep
        // something alive — never accuse it (RFC 0012 §2). Silence would be the accusing
        // direction.
        let a = ProjectPath(SmolStr::new("Sources/A/Thing.swift"));
        let b = ProjectPath(SmolStr::new("Sources/B/Thing.swift"));
        let mut classes = HashMap::new();
        classes.insert("Thing", vec![&a, &b]);

        let document = ProjectPath(SmolStr::new("Resources/Main.storyboard"));
        assert_eq!(
            nearest_declarations(&classes, "Thing", &document),
            vec![&a, &b]
        );
        assert!(nearest_declarations(&classes, "Absent", &document).is_empty());
    }

    #[test]
    fn a_document_that_does_not_parse_is_silence_not_a_diagnostic() {
        assert!(parse("<document targetRuntime=\"iOS.CocoaTouch\"").is_none());
        assert!(parse("").is_none());
    }
}
