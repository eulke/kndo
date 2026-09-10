struct Box {
    func read() -> Int { 1 }

    // `unsafe` as an expression modifier (Swift 6.2, strict memory safety).
    // tree-sitter-swift 0.7.3 does not know the form, and the keyword is the
    // one token it cannot place: the call after it is still read, which is why
    // `Sink.forgotten` comes back `internal-only` rather than unjudged.
    func go(_ sink: Sink) {
        unsafe sink.forgotten()
    }
}

struct Sink {
    func forgotten() {}
}
