package demo.waste

// Nothing imports this file, and `internal` keeps it off the module's published
// surface, so no root reaches it: plainly dead, and reported the moment the
// analysis has a root to start from at all.
internal fun orphan(): Int = 0
