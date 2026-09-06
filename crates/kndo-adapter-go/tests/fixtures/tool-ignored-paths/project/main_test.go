package main

import (
	"testing"

	"example.com/tool/testdata/gen"
)

func TestGen(t *testing.T) {
	if gen.Never() == "" {
		t.Fatal("empty")
	}
}
