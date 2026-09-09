package demo

// Reached from Main.kt. It sits before the break, so it parses normally, and
// reaching this file is what puts the two subjects below in play.
fun cache(name: String): String = name

internal class Cache(val name: String) {
    internal fun invalidate() {}

    internal fun neverCalled() {}

    // A `when` guard (Kotlin 2.1). The pinned grammar does not know it, and
    // recovery ends its reading of the class here: `invalidate` and
    // `neverCalled` above survive as nodes under the ERROR, while everything
    // from this point on is text no node covers.
    fun label(x: Any): String {
        return when (x) {
            is String if x.isEmpty() -> "empty"
            else -> "other"
        }
    }

    fun refresh() {
        invalidate()
    }
}
