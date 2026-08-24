open class Widget {
    // Used only within this file — already the tightest rung for its level.
    private func a() {
    }

    // Used only within this file too — `fileprivate` maps to the same `File` scope as
    // `private`, so neither accuses the other even though their ladder indices differ.
    fileprivate func b() {
    }

    // Used from Sibling.swift (same target, different file) — required scope is `Unit`,
    // which the ladder covers at the "internal" rung — already tightest.
    internal func c() {
    }

    // Same evidence as `c`, but declared `public` — wider than the target-only usage
    // requires.
    public func d() {
    }

    // Same evidence as `c`, but declared `open` — wider than the target-only usage
    // requires.
    open func e() {
    }

    // No modifier at all — Swift's default is `internal`. Same target-only evidence as `c`,
    // proving the no-modifier default computes the same tightest-sufficient check as an
    // explicit `internal` would.
    func useOwn() {
    }

    func run() {
        a()
        b()
    }
}
