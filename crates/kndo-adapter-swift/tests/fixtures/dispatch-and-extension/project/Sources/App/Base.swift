class Base {
    // Declared here; `Impl` overrides it below. Reached only through the duck-typed
    // member fallback (Runner calls `go()` by name, with no receiver-type info available
    // to extraction) — not a strong enough reference to justify its declared width, so this
    // reads `internal-only` rather than `unused`.
    func go() {
    }
}
