package util

// Helper is exported, but internal/ is not externally consumed by definition — never promoted,
// and nothing in-repo calls it, so it's genuinely unused (docs/adapters/go.md §0, §4).
func Helper() string {
	return "unused internal export"
}
