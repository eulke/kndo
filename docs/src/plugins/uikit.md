# `kndo:uikit`

**Status:** Shipped, built-in. Activation: any `**/*.storyboard` or `**/*.xib`.

## The gap

A UIKit app's view controllers are never constructed by any line of Swift. The storyboard
names the class as a string, UIKit instantiates it at launch, and the connections panel binds
`@IBOutlet` properties and `@IBAction` methods by name:

```xml
<viewController id="wco-eY-gNu" customClass="PHPickerResultViewController" customModule="Kingfisher_Demo">
  <view key="view">
    <subviews>
      <button id="btn-1">
        <connections>
          <action selector="onTapButton" destination="wco-eY-gNu" eventType="touchUpInside"/>
        </connections>
      </button>
    </subviews>
    <connections>
      <outlet property="imageView" destination="img-1"/>
    </connections>
  </view>
</viewController>
```

Every one of those is a reference living entirely outside the code graph. Without the plugin
kndo sees a class nothing constructs and members nothing touches outside their own file.

## Measured

| repo | before | after | what went away |
|---|---|---|---|
| Kingfisher | 474 findings | 460 | 13 `internal-only` on `@IBOutlet` fields across 8 view controllers, plus `AutoSizingTableViewCell` itself |
| Alamofire | 505 findings | 503 | `DetailViewController` (`internal-only` class) and `MasterViewController.titleImageView` |

Every one is a false positive in the strict sense: `private` — the remediation `internal-only`
prints — breaks the connection at runtime. Nothing new appeared in either repo, and the four
non-Apple corpus repos measured `+0/−0`.

## The three rules

1. **`customClass` → a Production root, and a reference from the document.** The root answers
   "is this alive"; the reference answers "is it used outside its own file", which is what the
   measured `internal-only` findings actually asked.
2. **`<outlet>` / `<outletCollection>` → a member of the nearest enclosing `customClass`.**
   The nesting *is* the ownership: `titleImageView` is written three elements below the
   controller that declares it.
3. **`<action>` → a member of the object its `destination` names**, not of the element it is
   written inside. A button's connection panel is where an action is authored; the selector
   belongs to the controller. The selector is ObjC's (`doThing:withValue:`), so the first
   segment is the declaration name in every arity.

## Why the id is a framework, not the editor

Interface Builder is the editor; UIKit is what runs the file. The same format lays out AppKit
and WatchKit apps, and the three share neither a class hierarchy nor an answer to "who
instantiates this". The document draws the line itself — `targetRuntime="iOS.CocoaTouch"` vs
`MacOSX.Cocoa` vs `watchKit` — so this plugin reads only the first. Alamofire's watchOS
`Interface.storyboard` and Kingfisher's macOS `Main.storyboard` are deliberately untouched:
`kndo:appkit` and `kndo:watchkit` are siblings that must arrive with their own measured cases.

## The ambiguity, and how it is resolved

A document names a class as a bare string, and one app can declare that name twice.
Kingfisher does: `SwiftUIViewController` exists in both `Kingfisher-Demo/` and
`Kingfisher-macOS-Demo/`. Matching by name alone had the iOS storyboard speaking for the macOS
class — measured as `+2` findings that only happened to be correct.

The plugin resolves to the declarations under the **deepest ancestor directory of the document
that contains one**, falling back to every match when none shares an ancestor short of the
project root. The document's own `customModule` would be the exact answer, but a Swift module
is not something kndo's graph models, so a rule written on it could not be verified. Proximity
is what both sides of the question can see.

The fallback is the keep-alive direction (RFC 0012 §2): with nothing to choose between
candidates, contributing to all of them can only keep something alive, never accuse it.

## What it does not do

- No AppKit, no WatchKit (above).
- No `customModule`-based module resolution (above).
- Nothing about a name it cannot point at a real declaration for — a `dataSource` outlet on a
  plain `UITableView` resolves to nothing and is dropped core-side, silently and correctly.
