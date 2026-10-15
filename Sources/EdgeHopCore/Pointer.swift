import Foundation

/// Where the injected cursor is. Relative moves need a starting point even if no absolute move was
/// ever received (the M4 sender only sends deltas), so on first use (or after being idle) the state is
/// seeded from the real cursor position.
///
/// Regression note: the first injector dropped every relative move until an absolute move had been
/// seen, and posted clicks at (0, 0). See `PointerStateTests`.
public struct PointerState: Sendable {
    public private(set) var position: Point?

    public init() {}

    public mutating func reset() { position = nil }

    public mutating func set(_ point: Point) { position = point }

    /// Current position, seeded from `actual` (the real cursor) when unknown.
    public mutating func current(actual: () -> Point) -> Point {
        if let position { return position }
        let seeded = actual()
        position = seeded
        return seeded
    }

    /// Applies a delta, clamped to `bounds` (the desktop rectangle), and returns the new position.
    public mutating func moved(dx: Double, dy: Double, actual: () -> Point, bounds: Rect) -> Point {
        let start = current(actual: actual)
        let next = Point(
            x: clamp(start.x + dx, bounds.minX, bounds.maxX - 1),
            y: clamp(start.y + dy, bounds.minY, bounds.maxY - 1)
        )
        position = next
        return next
    }
}
