package com.pkga

// Used from pkgb by fully-qualified name — no import: only the module REGION's
// reference pool can keep it.
internal fun helper() {}

// Nothing in the module names it: the region is bounded, so this is judgeable —
// the exact declaration `internal` folded-to-Exported used to hide.
internal fun lonely() {}

private fun fileGhost() {}
