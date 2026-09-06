package lib

// Exported is the module's published surface: nothing in this repository has
// to name it for it to be alive.
func Exported() int {
	return helper()
}
