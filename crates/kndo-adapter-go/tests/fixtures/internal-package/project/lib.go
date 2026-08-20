package lib

// PublicAPI is exported from a non-internal, non-test file: library-mode promotion makes it a
// production root with zero in-repo callers (docs/adapters/go.md §4).
func PublicAPI() string {
	return "ok"
}
