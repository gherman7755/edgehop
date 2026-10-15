import Foundation

public struct Point: Equatable, Sendable {
    public var x: Double
    public var y: Double
    public init(x: Double, y: Double) {
        self.x = x
        self.y = y
    }
}

public struct Rect: Equatable, Sendable {
    public var x: Double
    public var y: Double
    public var width: Double
    public var height: Double

    public init(x: Double, y: Double, width: Double, height: Double) {
        self.x = x
        self.y = y
        self.width = width
        self.height = height
    }

    public var minX: Double { x }
    public var minY: Double { y }
    public var maxX: Double { x + width }
    public var maxY: Double { y + height }

    public func contains(_ p: Point) -> Bool {
        p.x >= minX && p.x < maxX && p.y >= minY && p.y < maxY
    }
}

public enum Side: String, Equatable, Sendable, CaseIterable {
    case left, right, top, bottom

    public var opposite: Side {
        switch self {
        case .left: return .right
        case .right: return .left
        case .top: return .bottom
        case .bottom: return .top
        }
    }
}

/// A display of some machine, placed in the shared virtual canvas.
public struct Monitor: Equatable, Sendable {
    public var id: String
    public var deviceId: String
    public var frame: Rect

    public init(id: String, deviceId: String, frame: Rect) {
        self.id = id
        self.deviceId = deviceId
        self.frame = frame
    }
}

/// Explicit adjacency: leaving `from` through `side` enters `to`.
public struct Edge: Equatable, Sendable {
    public var from: String
    public var side: Side
    public var to: String

    public init(from: String, side: Side, to: String) {
        self.from = from
        self.side = side
        self.to = to
    }
}

public struct Layout: Equatable, Sendable {
    public var monitors: [Monitor]
    public var edges: [Edge]

    public init(monitors: [Monitor], edges: [Edge] = []) {
        self.monitors = monitors
        self.edges = edges
    }

    public func monitor(id: String) -> Monitor? {
        monitors.first { $0.id == id }
    }

    public func monitor(containing p: Point) -> Monitor? {
        monitors.first { $0.frame.contains(p) }
    }

    public func target(from monitorId: String, side: Side) -> Monitor? {
        guard let edge = edges.first(where: { $0.from == monitorId && $0.side == side }) else { return nil }
        return monitor(id: edge.to)
    }

    /// Derive edges from geometry: two monitors are adjacent when facing sides are within
    /// `tolerance` points of each other and their ranges overlap along the shared axis.
    /// (CursorHop uses a 2 px tolerance; edges themselves are explicit data there.)
    public static func deriveEdges(from monitors: [Monitor], tolerance: Double = 2) -> [Edge] {
        func overlap(_ a0: Double, _ a1: Double, _ b0: Double, _ b1: Double) -> Bool {
            min(a1, b1) - max(a0, b0) > 0
        }
        var edges: [Edge] = []
        for a in monitors {
            for b in monitors where a.id != b.id {
                if abs(a.frame.maxX - b.frame.minX) <= tolerance,
                   overlap(a.frame.minY, a.frame.maxY, b.frame.minY, b.frame.maxY) {
                    edges.append(Edge(from: a.id, side: .right, to: b.id))
                    edges.append(Edge(from: b.id, side: .left, to: a.id))
                }
                if abs(a.frame.maxY - b.frame.minY) <= tolerance,
                   overlap(a.frame.minX, a.frame.maxX, b.frame.minX, b.frame.maxX) {
                    edges.append(Edge(from: a.id, side: .bottom, to: b.id))
                    edges.append(Edge(from: b.id, side: .top, to: a.id))
                }
            }
        }
        return edges
    }
}

public struct EdgeCrossing: Equatable, Sendable {
    public var from: String
    public var to: String
    public var side: Side
    /// Entry position in canvas coordinates, already clamped to the destination monitor.
    public var entry: Point
}

/// Decides when a moving cursor leaves a monitor through an edge that has a mapping.
///
/// This is *our* design (report 04 section [D]), not CursorHop's exact algorithm:
/// - trigger band: the cursor is within `band` px of the edge and moving outward;
/// - dead corners: ignore the first `deadCorner` px at both ends of an edge;
/// - entry: the coordinate along the edge is preserved in canvas space and clamped to the
///   destination; the perpendicular coordinate is the destination's facing edge.
public struct EdgeDetector: Sendable {
    public var layout: Layout
    public var band: Double
    public var deadCorner: Double

    public init(layout: Layout, band: Double = 1, deadCorner: Double = 0) {
        self.layout = layout
        self.band = band
        self.deadCorner = deadCorner
    }

    public func detect(position p: Point, delta d: Point, monitorId: String) -> EdgeCrossing? {
        guard let src = layout.monitor(id: monitorId) else { return nil }
        let f = src.frame
        for side in Side.allCases {
            let atEdge: Bool
            let movingOut: Bool
            let alongCoord: Double
            let alongRange: (Double, Double)
            switch side {
            case .left: atEdge = p.x < f.minX + band; movingOut = d.x < 0; alongCoord = p.y; alongRange = (f.minY, f.maxY)
            case .right: atEdge = p.x >= f.maxX - band; movingOut = d.x > 0; alongCoord = p.y; alongRange = (f.minY, f.maxY)
            case .top: atEdge = p.y < f.minY + band; movingOut = d.y < 0; alongCoord = p.x; alongRange = (f.minX, f.maxX)
            case .bottom: atEdge = p.y >= f.maxY - band; movingOut = d.y > 0; alongCoord = p.x; alongRange = (f.minX, f.maxX)
            }
            guard atEdge, movingOut else { continue }
            if deadCorner > 0,
               alongCoord < alongRange.0 + deadCorner || alongCoord >= alongRange.1 - deadCorner {
                continue
            }
            guard let dst = layout.target(from: monitorId, side: side) else { continue }
            let entry: Point
            switch side {
            case .left: entry = Point(x: dst.frame.maxX - 1, y: clamp(p.y, dst.frame.minY, dst.frame.maxY - 1))
            case .right: entry = Point(x: dst.frame.minX, y: clamp(p.y, dst.frame.minY, dst.frame.maxY - 1))
            case .top: entry = Point(x: clamp(p.x, dst.frame.minX, dst.frame.maxX - 1), y: dst.frame.maxY - 1)
            case .bottom: entry = Point(x: clamp(p.x, dst.frame.minX, dst.frame.maxX - 1), y: dst.frame.minY)
            }
            return EdgeCrossing(from: monitorId, to: dst.id, side: side, entry: entry)
        }
        return nil
    }
}

func clamp(_ v: Double, _ lo: Double, _ hi: Double) -> Double {
    min(max(v, lo), hi)
}

/// Cursor position on a remote screen while input is forwarded: integrates deltas, clamps to
/// the current monitor and reports crossings (to another remote monitor or back).
public struct VirtualCursor: Sendable {
    public var detector: EdgeDetector
    public private(set) var monitorId: String
    public private(set) var position: Point

    public init(detector: EdgeDetector, monitorId: String, position: Point) {
        self.detector = detector
        self.monitorId = monitorId
        self.position = position
    }

    /// Returns the crossing if this move left the current monitor through a mapped edge.
    @discardableResult
    public mutating func apply(dx: Double, dy: Double) -> EdgeCrossing? {
        guard let monitor = detector.layout.monitor(id: monitorId) else { return nil }
        let f = monitor.frame
        let moved = Point(
            x: clamp(position.x + dx, f.minX, f.maxX - 1),
            y: clamp(position.y + dy, f.minY, f.maxY - 1)
        )
        if let crossing = detector.detect(position: moved, delta: Point(x: dx, y: dy), monitorId: monitorId) {
            monitorId = crossing.to
            position = crossing.entry
            return crossing
        }
        position = moved
        return nil
    }
}
