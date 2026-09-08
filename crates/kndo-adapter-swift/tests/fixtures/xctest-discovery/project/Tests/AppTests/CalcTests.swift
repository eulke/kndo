import XCTest
@testable import App

final class CalcTests: XCTestCase {
    func testAdd() {
        XCTAssertEqual(Calc().add(1, 2), 3)
    }

    private func helper() -> Int { 0 }
}
