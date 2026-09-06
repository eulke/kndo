package main

import (
	"fmt"

	"example.com/dep"
	scratch "example.com/tool/_scratch"
	"example.com/tool/sub"
)

func main() {
	fmt.Println(dep.Greet(), sub.Format("x"), scratch.Old())
}
