package demo.core

// Named only from this module's own test source set, which Gradle makes the
// main set's friend. That friendship is the whole of what keeps this alive.
internal fun probe(): Int = 7
