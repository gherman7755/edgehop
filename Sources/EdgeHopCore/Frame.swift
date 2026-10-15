import Foundation

public enum FrameType: UInt8, Sendable {
    case hello = 1
    case layout = 2
    case input = 3
    case control = 4
    case ping = 5
    case pong = 6
}

public struct Frame: Equatable, Sendable {
    public var type: FrameType
    public var flags: UInt8
    public var payload: Data

    public init(type: FrameType, flags: UInt8 = 0, payload: Data = Data()) {
        self.type = type
        self.flags = flags
        self.payload = payload
    }
}

public enum FrameError: Error, Equatable {
    case badMagic
    case unknownType(UInt8)
    case payloadTooLarge(Int)
}

/// Logical frame: 8-byte header + payload. Carried inside the (future) encrypted transport.
///
///     0..2  magic   "EH"
///     2     type    FrameType
///     3     flags   reserved
///     4..8  length  big-endian UInt32, <= maxPayload
public enum FrameCodec {
    public static let magic: [UInt8] = [0x45, 0x48]
    public static let headerSize = 8
    public static let maxPayload = 16 * 1024 * 1024

    public static func encode(_ frame: Frame) -> Data {
        precondition(frame.payload.count <= maxPayload, "payload exceeds maxPayload")
        var out = Data(capacity: headerSize + frame.payload.count)
        out.append(contentsOf: magic)
        out.append(frame.type.rawValue)
        out.append(frame.flags)
        let n = UInt32(frame.payload.count)
        out.append(contentsOf: [UInt8(n >> 24), UInt8((n >> 16) & 0xFF), UInt8((n >> 8) & 0xFF), UInt8(n & 0xFF)])
        out.append(frame.payload)
        return out
    }
}

/// Incremental decoder for a byte stream (TCP delivers arbitrary chunks).
/// Throws on a protocol violation; the caller must then drop the connection.
public struct FrameDecoder {
    private var buffer = Data()

    public init() {}

    public mutating func feed(_ chunk: Data) throws -> [Frame] {
        buffer.append(chunk)
        var frames: [Frame] = []
        while buffer.count >= FrameCodec.headerSize {
            let b = [UInt8](buffer.prefix(FrameCodec.headerSize))
            guard b[0] == FrameCodec.magic[0], b[1] == FrameCodec.magic[1] else { throw FrameError.badMagic }
            guard let type = FrameType(rawValue: b[2]) else { throw FrameError.unknownType(b[2]) }
            let length = Int(UInt32(b[4]) << 24 | UInt32(b[5]) << 16 | UInt32(b[6]) << 8 | UInt32(b[7]))
            guard length <= FrameCodec.maxPayload else { throw FrameError.payloadTooLarge(length) }
            guard buffer.count >= FrameCodec.headerSize + length else { break }
            let start = buffer.startIndex + FrameCodec.headerSize
            frames.append(Frame(type: type, flags: b[3], payload: Data(buffer[start..<start + length])))
            buffer = Data(buffer.dropFirst(FrameCodec.headerSize + length))
        }
        return frames
    }
}
