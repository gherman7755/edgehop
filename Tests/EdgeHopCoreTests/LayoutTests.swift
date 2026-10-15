import Testing
@testable import EdgeHopCore

private func rect(_ x: Double, _ y: Double, _ w: Double, _ h: Double) -> Rect {
    Rect(x: x, y: y, width: w, height: h)
}

/// Mac on the left (1920x1080), remote on the right, shifted down by 200 px.
private let mac = Monitor(id: "mac", deviceId: "A", frame: rect(0, 0, 1920, 1080))
private let remote = Monitor(id: "remote", deviceId: "B", frame: rect(1920, 200, 1920, 1080))

@Suite struct LayoutTests {
    @Test func derivesEdgesWithinTolerance() {
        let nearlyTouching = Monitor(id: "r2", deviceId: "B", frame: rect(1921, 0, 800, 600))
        let edges = Layout.deriveEdges(from: [mac, nearlyTouching], tolerance: 2)
        #expect(edges.contains(Edge(from: "mac", side: .right, to: "r2")))
        #expect(edges.contains(Edge(from: "r2", side: .left, to: "mac")))
        #expect(Layout.deriveEdges(from: [mac, nearlyTouching], tolerance: 0).isEmpty)
    }

    @Test func noEdgeWithoutOverlapAlongTheSharedAxis() {
        let diagonal = Monitor(id: "d", deviceId: "B", frame: rect(1920, 1080, 800, 600))
        #expect(Layout.deriveEdges(from: [mac, diagonal]).isEmpty)
    }
}

@Suite struct EdgeDetectorTests {
    private func detector(deadCorner: Double = 0) -> EdgeDetector {
        let monitors = [mac, remote]
        return EdgeDetector(
            layout: Layout(monitors: monitors, edges: Layout.deriveEdges(from: monitors)),
            deadCorner: deadCorner
        )
    }

    @Test func crossesRightEdgeAndClampsEntryIntoTheDestination() throws {
        // y = 100 is above the remote's top (200): entry is clamped to 200.
        let crossing = try #require(detector().detect(position: Point(x: 1919, y: 100), delta: Point(x: 5, y: 0), monitorId: "mac"))
        #expect(crossing.to == "remote")
        #expect(crossing.side == .right)
        #expect(crossing.entry == Point(x: 1920, y: 200))
    }

    @Test func preservesAlongEdgeCoordinateInsideTheSharedRange() throws {
        let crossing = try #require(detector().detect(position: Point(x: 1919, y: 600), delta: Point(x: 5, y: 0), monitorId: "mac"))
        #expect(crossing.entry == Point(x: 1920, y: 600))
    }

    @Test func doesNotCrossWhenMovingAwayFromTheEdge() {
        #expect(detector().detect(position: Point(x: 1919, y: 600), delta: Point(x: -5, y: 0), monitorId: "mac") == nil)
    }

    @Test func doesNotCrossWithoutAMappedEdge() {
        // Left edge of the Mac has no neighbour.
        #expect(detector().detect(position: Point(x: 0, y: 600), delta: Point(x: -5, y: 0), monitorId: "mac") == nil)
    }

    @Test func ignoresDeadCorners() {
        let d = detector(deadCorner: 50)
        #expect(d.detect(position: Point(x: 1919, y: 1070), delta: Point(x: 5, y: 0), monitorId: "mac") == nil)
        #expect(d.detect(position: Point(x: 1919, y: 600), delta: Point(x: 5, y: 0), monitorId: "mac") != nil)
    }
}

@Suite struct VirtualCursorTests {
    private func cursor() -> VirtualCursor {
        let monitors = [mac, remote]
        let detector = EdgeDetector(layout: Layout(monitors: monitors, edges: Layout.deriveEdges(from: monitors)))
        return VirtualCursor(detector: detector, monitorId: "remote", position: Point(x: 1920, y: 600))
    }

    @Test func integratesDeltasAndClampsToTheMonitor() {
        var c = cursor()
        #expect(c.apply(dx: 10, dy: -30) == nil)
        #expect(c.position == Point(x: 1930, y: 570))
        _ = c.apply(dx: 0, dy: -10_000)
        #expect(c.position.y == 200)
        #expect(c.monitorId == "remote")
    }

    @Test func returnsToTheMacThroughTheReturnEdge() {
        var c = cursor()
        let crossing = c.apply(dx: -50, dy: 0)
        #expect(crossing?.to == "mac")
        #expect(c.monitorId == "mac")
        #expect(c.position == Point(x: 1919, y: 600))
    }
}
