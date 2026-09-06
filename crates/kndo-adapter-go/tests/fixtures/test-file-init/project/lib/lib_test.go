package lib

import (
	"testing"

	"example.com/demo/internal/testutil"
)

func TestExported(t *testing.T) {
	if Exported() != testutil.Want() {
		t.Fail()
	}
}
