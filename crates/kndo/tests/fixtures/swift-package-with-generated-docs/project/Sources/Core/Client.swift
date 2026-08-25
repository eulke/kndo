public struct Client {
    public init() {}

    public func send() -> String {
        return helper()
    }

    func helper() -> String {
        return "sent"
    }
}
