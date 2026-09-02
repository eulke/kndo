package cov

// Add is exercised by the test.
func Add(a, b int) int { return a + b }

// Sub is exercised too.
func Sub(a, b int) int {
	return a - b
}

// NeverRan is reachable and never executed.
func NeverRan(n int) int {
	if n > 0 {
		return n * 2
	}
	return -n
}
