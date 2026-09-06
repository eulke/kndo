package sub

// Format wraps s in brackets.
func Format(s string) string {
	return "[" + s + "]"
}

func stale() string {
	return "never called"
}
