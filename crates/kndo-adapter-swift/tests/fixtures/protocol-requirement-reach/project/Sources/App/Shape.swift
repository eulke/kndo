// Both directions of one fact: a member on a promised surface cannot narrow.
// `Square.area` is fixed by its overrider; `Cube.volume` is fixed by nothing
// and should be advised — the relation says which is which.
class Square {
    func area() -> Int { 4 }

    // Nothing overrides this one: same reach, same use, and the advice stands.
    func perimeter() -> Int { 8 }
}

class Rounded: Square {
    override func area() -> Int { 3 }
}

protocol Solid {
    func faces() -> Int
}

struct Cube: Solid {
    func faces() -> Int { 6 }

    // A conforming type's OTHER members promise nothing.
    func volume() -> Int { 27 }
}

func describeShapes() -> Int {
    let s = Square()
    let r = Rounded()
    let c = Cube()
    return s.area() + s.perimeter() + r.area() + c.faces() + c.volume()
}
