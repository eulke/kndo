package com.foo;

class Impl implements Greeter {
    // Called only through dispatch (Runner never names `go` directly) — the @Override
    // rooting rule (spec §0) is what keeps this alive, not a duck-typed fallback hit.
    @Override
    public void go() {
        helper();
    }

    private void helper() {}
}
