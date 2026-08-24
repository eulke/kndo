package lib

// PublicAPI is exported from a non-internal, non-test file: library-mode promotion makes it a
// production root with zero in-repo callers.
func PublicAPI() string {
	return "ok"
}
