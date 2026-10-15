import CoreGraphics
import Foundation

/// A display as advertised to the other computer: global points, the primary display at (0, 0).
public struct MonitorInfo: Codable, Equatable, Sendable {
    public var id: String
    public var x: Double
    public var y: Double
    public var width: Double
    public var height: Double

    public init(id: String, x: Double, y: Double, width: Double, height: Double) {
        self.id = id
        self.x = x
        self.y = y
        self.width = width
        self.height = height
    }
}

public enum LocalDisplays {
    public static func list() -> [MonitorInfo] {
        var count: UInt32 = 0
        guard CGGetActiveDisplayList(0, nil, &count) == .success, count > 0 else { return [] }
        var ids = [CGDirectDisplayID](repeating: 0, count: Int(count))
        guard CGGetActiveDisplayList(count, &ids, &count) == .success else { return [] }
        return ids.prefix(Int(count)).map { id in
            let b = CGDisplayBounds(id)
            return MonitorInfo(id: "d\(id)", x: b.origin.x, y: b.origin.y, width: b.width, height: b.height)
        }
    }
}
