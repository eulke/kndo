package com.foo;

public class Widget {
    // Used only within this file (main below) — already the tightest rung for its level.
    private void a() {}

    // Used from main() AND Sibling (same package) — required scope is Unit, which matches
    // its OWN declared level exactly — already tightest.
    void b() {}

    // Used ONLY from Sibling (same package, different file) — required is Unit, strictly
    // narrower than protected's declared Public — should flag "package-private would suffice".
    protected void c() {}

    // Same evidence as c(), but declared public — should flag "package-private would suffice".
    public void d() {}

    public static void main(String[] args) {
        Widget w = new Widget();
        w.a();
        w.b();
        Inner in = new Inner();
        in.go();
        new Sibling().run();
    }

    class Inner {
        void go() {
            helper();
        }

        void helper() {}
    }
}
