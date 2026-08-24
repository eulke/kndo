package com.foo

fun main() {
    val w = Widget()
    w.useOwn()
    w.b()
    w.c()
    w.d()
    Widget.factory()
    Widget.Inner().go()
}
