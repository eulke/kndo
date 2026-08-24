package sub

func format(s string) string {
	return s + "!"
}

// deadHelper is never called from anywhere — the one real finding this fixture pins.
func deadHelper() string {
	return "dead"
}
