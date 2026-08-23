package util

// Helper is exported, but internal/ is not externally consumed by definition — never promoted,
// and nothing in-repo calls it, so it's genuinely unused.
func Helper() string {
	return "unused internal export"
}
