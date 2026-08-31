// Adds a member to `Base` from a different file — the one member-owner shape unique to this
// adapter: `extra`'s `member_of` is `Base`'s bare name even though it's declared here, not in
// Base.swift.
extension Base {
    public func extra() {
    }
}
