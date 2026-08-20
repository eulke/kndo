package sub

// Greeting calls format, declared in the sibling file below — no import needed, same package.
func Greeting() string {
	return format("hello")
}
