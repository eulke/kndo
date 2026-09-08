package demo.core

// The module's published surface: `app` calls it across the project dependency.
fun greet(): String = format("hello")

// Used from this file alone, though `internal` widens it to the whole module —
// a judgment only a unit can make, and the unit is what the build scripts state.
internal fun format(text: String): String = text.uppercase()
