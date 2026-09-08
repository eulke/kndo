// The grammar leaves an operator anonymous on BOTH sides: `func +` takes no
// name field, and `a + b` spends none. Declared and used, they are one name.
struct Vector {
    let x: Int

    static func + (l: Vector, r: Vector) -> Vector { Vector(x: l.x &+ r.x) }

    // Nothing in the project writes `*`: declared, never used. Its body must
    // not spell it either — an operator's own body is a use like any other.
    static func * (l: Vector, r: Vector) -> Vector { Vector(x: l.x) }
}

private extension Vector {
    // A `private extension`'s members are the FILE's, not the module's.
    static var zero: Vector { Vector(x: 0) }
}

func sum(_ a: Vector, _ b: Vector) -> Vector {
    a + b
}

func origin() -> Vector { .zero }
