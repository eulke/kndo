package demo

// A name that appears ONLY inside the multi-dollar string's body below. If that
// body were read as a Kotlin template, `spring` would have a use and be alive.
// It is dead, and that is what this fixture is for.
internal fun spring(): String = "not the placeholder's"

// Kotlin 2.0's multi-dollar string, in the spelling that motivates it: a run of
// two or more `$` raises how many dollars an interpolation needs, so
// `${spring.exposed.url}` here is TEXT — the placeholder has to reach Spring
// unexpanded.
internal class Config(
    val url: String = $$"${spring.exposed.url}",
)

// A plain template beside it, where `$` DOES interpolate: `label` is a use.
internal fun describe(label: String): String = "config for $label"

fun main() {
    println(describe(Config().url))
}
