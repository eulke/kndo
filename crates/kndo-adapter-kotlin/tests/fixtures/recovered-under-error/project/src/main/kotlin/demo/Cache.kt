package demo

// Reached from Main.kt. It sits before the break, so it parses normally, and
// reaching this file is what puts the two subjects below in play.
fun cache(name: String): String = name

internal class Cache(val name: String) {
    internal fun invalidate() {}

    internal fun neverCalled() {}

    // A context parameter (Kotlin 2.2). The pinned grammar does not know the
    // clause, and recovery ends its reading of the class here: `invalidate` and
    // `neverCalled` above survive as `function_declaration`s under the ERROR,
    // while everything from this point on is text no node covers.
    context(logger: Logger)
    fun label(): String = name

    fun refresh() {
        invalidate()
    }
}

internal interface Logger
