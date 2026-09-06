package dep

// Greet is what the project calls.
func Greet() string {
	return "hello from " + name()
}

func name() string {
	return "dep"
}

func unusedInVendor() string {
	return "somebody else's dead code"
}
