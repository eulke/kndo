package demo

fun main() {
    val newId = Rows.insert { it.id } get Rows.id
    println(newId)
}
