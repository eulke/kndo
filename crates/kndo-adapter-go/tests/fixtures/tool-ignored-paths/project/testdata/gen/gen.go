package gen

// Never is an input to a test, not a package the tool builds.
func Never() string {
	return helper()
}

func helper() string {
	return "testdata"
}
