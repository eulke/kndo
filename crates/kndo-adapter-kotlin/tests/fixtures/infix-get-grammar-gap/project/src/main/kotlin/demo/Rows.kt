package demo

internal object Rows {
    val id: Int = 0
    fun insert(build: (Rows) -> Unit): Rows = this
    infix fun get(column: Int): Int = column
}
