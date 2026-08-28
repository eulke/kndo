package main

import "example.com/store/internal/store"

func main() {
	s := store.New()
	s.Put("a", "b")
}
