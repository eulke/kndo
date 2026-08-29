# `kndo:info-plist` — Apple bundle conventions plugin

**Status:** Normative for the built-in `kndo:info-plist` plugin ·
**Implements:** `contribute_roots`, content-channel-aware ·
**Crate:** `crates/kndo-plugin-info-plist` · **Convention set versioned against:** Apple's
bundle `Info.plist` schema

## 1. An honest scope statement

An app bundle's `Info.plist` names classes **as strings**, and the system instantiates them at
launch. Nothing in source mentions those classes, so no reference edge can exist and the entry
surface of a whole target reads as unreachable. This is the file-outside-the-code half of
detection-gaps.md's audit: the fact is real, it is declared, and it lives in a file the language
graph never opens.

The plugin content channel exists exactly for this — read the non-source file, contribute the
root. The knowledge here is Apple's bundle schema and nothing else: **which keys carry a class
name**. That is what makes it a plugin for one concrete toolchain rather than a guess about
strings that look like identifiers.

## 2. Detection & what is rooted

**Activation:** `FileExists("**/Info.plist")`. No manifest declares "Apple", so unlike every
conventions plugin gated on a dependency this one gates on the file itself and pays a recursive
glob for it. The file is the signal; there is nothing cheaper to key on.

**Keys read**, curated the same way the Rust adapter's machinery-trait list is — a key earns its
place by naming a class a real project writes by hand and nothing in source mentions:

| Key | Names |
|---|---|
| `NSPrincipalClass` | a bundle's principal class (macOS/iOS apps, loadable bundles) |
| `NSExtensionPrincipalClass` | an app extension's principal class |
| `WKExtensionDelegateClassName` | a watchOS extension delegate |
| `UISceneDelegateClassName` / `UISceneClassName` | a scene manifest's delegate and scene |
| `CLKComplicationPrincipalClass` | a watch complication's data source |

Keys carrying a bundle id, a version, a file name or a capability are absent: they name no code.
`UISceneClassName` usually names a UIKit class rather than the project's, which costs nothing —
a name no symbol carries roots nothing, and a project that *does* subclass the scene gets its
subclass rooted.

**Value shape.** Xcode writes `$(PRODUCT_MODULE_NAME).ExtensionDelegate`; the class's own name
is the last dot-separated segment. A value that is only a build-setting interpolation names no
class and yields nothing.

**Nesting is a non-issue.** A plist is `<key>` followed by its value as the next sibling, at any
depth — a scene delegate sits four levels inside `UIApplicationSceneManifest` — so the scan
walks every element rather than a fixed path.

**Root tier: `Certain`, `RootKind::Production`.** The plist *declares* this; it is a manifest
entry point, not a naming convention someone might follow. What is soft is the name → symbol
match: two targets in one repository may each declare a class of that name, and then both are
rooted. That over-roots in the keep-alive direction, the only direction a root can err in safely.

## 3. Mechanism & recorded limits

- **The DOCTYPE is load-bearing.** Every Apple plist declares the PropertyList DTD, and
  `roxmltree` refuses such a document unless told otherwise. The first version of this plugin
  passed its own tests and found nothing in the field for exactly that reason; the fixtures now
  carry the DOCTYPE verbatim. The same bug was live in both coverage ingesters and is fixed.
- **Binary plists are not read.** `bplist00` is not XML; the plugin contributes nothing rather
  than guessing. Source repositories ship the XML form.
- **Interface Builder is a different tool.** `Main.storyboard`/`Interface.storyboard` name
  classes through `customClass`, with their own schema — Alamofire's `HostingController` and
  everything it reaches. That is `kndo:interface-builder`, a sibling plugin, not this one's
  business.
- **Field:** Alamofire 527 → 526 findings (85.6 → 85.7), `ExtensionDelegate` alive, nothing new.
  One finding is the *complete* effect there: that class references nothing else in the project,
  so there is no tree behind it to revive.
