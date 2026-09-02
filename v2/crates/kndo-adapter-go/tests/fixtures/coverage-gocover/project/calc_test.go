package cov

import "testing"

func TestAdd(t *testing.T) {
	if Add(1, 2) != 3 {
		t.Fatal("add")
	}
}

func TestSub(t *testing.T) {
	if Sub(5, 2) != 3 {
		t.Fatal("sub")
	}
}
