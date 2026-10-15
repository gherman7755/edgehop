import Testing
@testable import EdgeHopCore

@Suite struct PointerStateTests {
    private let desktop = Rect(x: 0, y: 0, width: 1000, height: 800)

    /// The M4 bug: the sender only sends deltas, so the receiver never had a position and dropped every move.
    @Test func relativeMoveWorksBeforeAnyAbsoluteMove() {
        var pointer = PointerState()
        let p = pointer.moved(dx: 30, dy: -10, actual: { Point(x: 500, y: 400) }, bounds: desktop)
        #expect(p == Point(x: 530, y: 390))
        #expect(pointer.position == p)
    }

    @Test func seedsFromTheRealCursorOnlyOnce() {
        var pointer = PointerState()
        var reads = 0
        let actual = { () -> Point in reads += 1; return Point(x: 100, y: 100) }
        _ = pointer.moved(dx: 1, dy: 0, actual: actual, bounds: desktop)
        _ = pointer.moved(dx: 1, dy: 0, actual: actual, bounds: desktop)
        #expect(reads == 1)
        #expect(pointer.position == Point(x: 102, y: 100))
    }

    @Test func clampsToTheDesktop() {
        var pointer = PointerState()
        let p = pointer.moved(dx: 5000, dy: -5000, actual: { Point(x: 10, y: 10) }, bounds: desktop)
        #expect(p == Point(x: 999, y: 0))
    }

    @Test func clicksUseTheRealCursorNotTheOrigin() {
        var pointer = PointerState()
        #expect(pointer.current(actual: { Point(x: 640, y: 360) }) == Point(x: 640, y: 360))
    }

    @Test func resetReseedsFromTheRealCursor() {
        var pointer = PointerState()
        _ = pointer.moved(dx: 10, dy: 10, actual: { Point(x: 0, y: 0) }, bounds: desktop)
        pointer.reset()
        #expect(pointer.current(actual: { Point(x: 300, y: 300) }) == Point(x: 300, y: 300))
    }
}
