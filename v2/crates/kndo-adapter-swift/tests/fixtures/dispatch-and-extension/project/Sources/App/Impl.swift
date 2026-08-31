class Impl: Base {
    // Called only through dispatch (no call site names `Base.go`/`Impl.go` specifically) —
    // the `override` rooting rule is what keeps this alive.
    override func go() {
    }
}
