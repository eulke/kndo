// Package yaml lives in a directory whose name ("v2") differs from its declared package
// name — the gopkg.in/yaml.v3 shape. main.go's `yaml.Run()` only resolves if assembly
// derives the qualifier from THIS declared name (RFC 0012 §9); the old last-segment guess
// ("v2") left Run unreferenced and falsely unused. Under internal/ so exports are not
// root-promoted — liveness rides entirely on the qualified call.
package yaml

func Run() {}
