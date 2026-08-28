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

## Two hooks, one input — and why the work runs twice

`contribute_roots` and `contribute_edges` need the same derivation: read every Interface
Builder document, resolve every name against the declarations kndo found. The `Plugin` trait
offers no per-run scratch space to share it through, which leaves three options and rules one
out immediately.

**Not a cache inside the plugin.** The native `Plugin` trait is `Send + Sync` and every hook
takes `&self`, so a cache is shared mutable state behind a lock, holding one run's answer on a
value the engine reuses. A WASM guest is genuinely different — RFC 0017 §4 gives it one
instance per graph-mutation round, and statics across the three hooks are contractual there —
but the native trait makes no such promise, and a built-in must not read as if it did.

**A pure function both hooks call** is what shipped. `wiring()` computes the whole answer and
each hook projects the part it needs. The derivation therefore runs twice per run, and that is
the accepted cost, measured: Kingfisher's six documents are 215 KB of XML, and parsing them
twice plus building the declaration index twice is a few milliseconds inside a 220 ms run.
What the shared function buys is the thing that actually costs — **one description of the
wiring**, so the two hooks cannot drift, and so `kndo:vite` and `kndo:rollup`, which have the
same two-hooks-one-input shape, copy a structure rather than a duplication.

**A `prepare` hook on the trait** is the answer if a plugin ever appears whose derivation is
expensive enough to matter. It is a deliberate contract change — the native trait *and* the
WIT world — and belongs in an RFC, not behind a cell in one plugin.

## What it does not do

- No AppKit, no WatchKit (above).
- No `customModule`-based module resolution (above).
- Nothing about a name it cannot point at a real declaration for — a `dataSource` outlet on a
  plain `UITableView` resolves to nothing and is dropped core-side, silently and correctly.
