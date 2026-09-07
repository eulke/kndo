package gram

// A grouped `var` block: the grammar wraps these in a `var_spec_list` that a
// one-level walk never reached, so every name here was invisible.
var (
	deadGrouped = 1
	usedGrouped = 2
)

// A multi-name const: the grammar labels the separating comma with the `name`
// field too, and only the first name was treated as a binding position.
const first, second = 3, 4

// A function named like its own package clause. The clause carries no field,
// so it was reading as a use of this.
func gram() {}

// A type whose only mention outside its own declaration is the receiver of its
// method — which Go requires to be in this package, so it is part of the
// type's definition and not a use of it.
type unreferenced struct{}

func (u unreferenced) Method() {}

type referenced struct{}

func Use() int {
	// A `:=` binds a local; naming it here is not a use of the package-level
	// declaration that happens to share the name.
	usedGrouped := usedGrouped
	var r referenced
	_ = r
	return usedGrouped + second
}
