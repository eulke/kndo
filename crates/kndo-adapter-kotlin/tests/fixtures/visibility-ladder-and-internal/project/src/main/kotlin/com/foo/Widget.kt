package com.foo

class Widget {
    private fun a() {
    }

    internal fun b() {
    }

    protected fun c() {
    }

    public fun d() {
    }

    fun useOwn() {
        a()
    }

    companion object {
        internal fun factory(): Widget {
            return Widget()
        }
    }

    class Inner {
        public fun go() {
        }
    }
}
