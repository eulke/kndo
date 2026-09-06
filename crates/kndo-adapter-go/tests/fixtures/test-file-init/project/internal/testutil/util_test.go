package testutil

import "testing"

func init() {
	// The test binary's own setup — it runs when THIS package's test binary
	// loads, and never in a production build.
}

func TestWant(t *testing.T) {
	if Want() != 1 {
		t.Fail()
	}
}
