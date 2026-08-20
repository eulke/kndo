package a

// Hello is module a's public API — library-promoted, and also genuinely called from the
// sibling module b through the go.work workspace.
func Hello() string {
	return hi()
}

func hi() string {
	return "hi"
}

// deadHelper is unexported with no caller anywhere in the workspace — the deliberate finding
// proving multi-module analysis stays precise, not silently conservative.
func deadHelper() {}
