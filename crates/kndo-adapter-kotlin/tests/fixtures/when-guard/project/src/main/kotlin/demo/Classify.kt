package demo

fun classify(x: Any): String = when (x) {
    is String if lengthy(x) -> "long string"
    is String -> "short string"
    else -> "other"
}

private fun lengthy(text: String): Boolean = text.length > 8

fun main() {
    println(classify("hello there"))
}
