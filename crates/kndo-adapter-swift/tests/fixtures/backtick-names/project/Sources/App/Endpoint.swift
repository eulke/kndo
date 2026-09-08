struct Endpoint {
    // A keyword needs the backticks; the name is `default` either way, and the
    // use below reaches it through the same bare name.
    static func `default`() -> Endpoint { Endpoint() }

    // Backticks are permitted on any identifier, not only keywords.
    static func `plain`() -> Endpoint { Endpoint() }
}

let seed = Endpoint.`default`()
let other = Endpoint.plain()
