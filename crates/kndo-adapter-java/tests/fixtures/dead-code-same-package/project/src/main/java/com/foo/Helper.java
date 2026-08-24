package com.foo;

class Helper {
    // No import needed for Main to call this — same package (the unit mechanism).
    static void live() {}

    // Never called from anywhere — genuinely dead.
    static void dead() {}
}
