package demo

// Reached from Main.kt. It sits before the break, so it parses normally, and
// reaching this file is what puts the two subjects below in play.
fun cache(name: String): String = name

internal class Cache(val name: String) {
    internal fun invalidate() {}

    internal fun neverCalled() {}

    // `get` as an INFIX function name after a trailing lambda — the shape
    // `infix-get-grammar-gap` pins, and Exposed's own quick-start spelling.
    // Kotlin writes accessors with the soft keyword `get`, the pinned grammar
    // prefers that reading here, and recovery ends its reading of the class:
    // `invalidate` and `neverCalled` above survive as nodes under the ERROR,
    // while everything from this point on is text no node covers.
    fun label(rows: Rows): String {
        val label = rows.take { it } get name
        return label
    }

    fun refresh() {
        invalidate()
    }
}

internal class Rows {
    fun take(f: (String) -> String): Rows = this
}
