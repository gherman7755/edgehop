import Foundation

/// Modifier state carried with key events. Bits mirror shift/control/option/command.
public struct Modifiers: OptionSet, Equatable, Sendable {
    public let rawValue: UInt8
    public init(rawValue: UInt8) { self.rawValue = rawValue }

    public static let shift = Modifiers(rawValue: 1 << 0)
    public static let control = Modifiers(rawValue: 1 << 1)
    public static let option = Modifiers(rawValue: 1 << 2)
    public static let command = Modifiers(rawValue: 1 << 3)
}

public enum MouseButton: UInt8, Equatable, Sendable {
    case left = 0
    case right = 1
    case middle = 2
    case back = 3
    case forward = 4
}

/// Platform-neutral input event. This is the unit that travels between machines.
///
/// Conventions (see report 01/02 of the CursorHop analysis):
/// - absolute moves are used while input is local (edge detection needs positions),
///   deltas while input is forwarded to a peer;
/// - scroll values are in 1/120 notch units (a multiple of 120 means whole "lines");
/// - `keyCode` is a macOS virtual keycode in this scaffold. A neutral table (e.g. USB HID
///   usage or Windows VK) is a later milestone.
public enum InputEvent: Equatable, Sendable {
    case mouseMoveAbsolute(x: Int32, y: Int32)
    case mouseMoveDelta(dx: Int32, dy: Int32)
    case mouseButton(MouseButton, isDown: Bool)
    case scroll(horizontal: Int32, vertical: Int32)
    case key(code: UInt16, isDown: Bool, modifiers: Modifiers)
}

public enum InputEventCodecError: Error, Equatable {
    case wrongLength(Int)
    case unknownKind(UInt8)
    case unknownButton(UInt8)
}

/// Fixed 16-byte wire record, big-endian numbers:
///
///     0      kind  (0 moveAbs, 1 moveDelta, 2 button, 3 scroll, 4 key)
///     1      flag  (button/key: 1 = down)
///     2      aux   (button: MouseButton raw value; key: modifiers)
///     3      reserved (0)
///     4..8   a     (x / dx / horizontal scroll / key code)
///     8..12  b     (y / dy / vertical scroll)
///     12..16 reserved (0)
extension InputEvent {
    public static let recordSize = 16

    public func encoded() -> Data {
        var bytes = [UInt8](repeating: 0, count: Self.recordSize)
        func put(_ value: Int32, at offset: Int) {
            let u = UInt32(bitPattern: value)
            bytes[offset] = UInt8(truncatingIfNeeded: u >> 24)
            bytes[offset + 1] = UInt8(truncatingIfNeeded: u >> 16)
            bytes[offset + 2] = UInt8(truncatingIfNeeded: u >> 8)
            bytes[offset + 3] = UInt8(truncatingIfNeeded: u)
        }
        switch self {
        case let .mouseMoveAbsolute(x, y):
            bytes[0] = 0
            put(x, at: 4)
            put(y, at: 8)
        case let .mouseMoveDelta(dx, dy):
            bytes[0] = 1
            put(dx, at: 4)
            put(dy, at: 8)
        case let .mouseButton(button, isDown):
            bytes[0] = 2
            bytes[1] = isDown ? 1 : 0
            bytes[2] = button.rawValue
        case let .scroll(horizontal, vertical):
            bytes[0] = 3
            put(horizontal, at: 4)
            put(vertical, at: 8)
        case let .key(code, isDown, modifiers):
            bytes[0] = 4
            bytes[1] = isDown ? 1 : 0
            bytes[2] = modifiers.rawValue
            put(Int32(code), at: 4)
        }
        return Data(bytes)
    }

    public static func decode(_ data: Data) throws -> InputEvent {
        guard data.count == recordSize else { throw InputEventCodecError.wrongLength(data.count) }
        let b = [UInt8](data)
        func int32(at offset: Int) -> Int32 {
            let u = UInt32(b[offset]) << 24 | UInt32(b[offset + 1]) << 16
                | UInt32(b[offset + 2]) << 8 | UInt32(b[offset + 3])
            return Int32(bitPattern: u)
        }
        switch b[0] {
        case 0: return .mouseMoveAbsolute(x: int32(at: 4), y: int32(at: 8))
        case 1: return .mouseMoveDelta(dx: int32(at: 4), dy: int32(at: 8))
        case 2:
            guard let button = MouseButton(rawValue: b[2]) else {
                throw InputEventCodecError.unknownButton(b[2])
            }
            return .mouseButton(button, isDown: b[1] != 0)
        case 3: return .scroll(horizontal: int32(at: 4), vertical: int32(at: 8))
        case 4:
            return .key(
                code: UInt16(truncatingIfNeeded: int32(at: 4)),
                isDown: b[1] != 0,
                modifiers: Modifiers(rawValue: b[2])
            )
        default: throw InputEventCodecError.unknownKind(b[0])
        }
    }
}
