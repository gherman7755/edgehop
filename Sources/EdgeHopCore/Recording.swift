import Foundation

/// One captured event with its time offset from the start of the recording.
public struct RecordedEvent: Equatable, Sendable {
    public var millis: Int
    public var event: InputEvent

    public init(millis: Int, event: InputEvent) {
        self.millis = millis
        self.event = event
    }
}

public enum RecordingError: Error, Equatable {
    /// 1-based line number of the malformed line.
    case malformed(line: Int)
}

/// Text recording format, one event per line: `<millis> <32 hex chars of the wire record>`.
/// Blank lines and lines starting with `#` are ignored.
public enum Recording {
    public static func line(_ recorded: RecordedEvent) -> String {
        let hex = recorded.event.encoded().map { String(format: "%02x", $0) }.joined()
        return "\(recorded.millis) \(hex)"
    }

    public static func parse(_ text: String) throws -> [RecordedEvent] {
        var result: [RecordedEvent] = []
        for (index, raw) in text.split(separator: "\n", omittingEmptySubsequences: false).enumerated() {
            let line = raw.trimmingCharacters(in: .whitespaces)
            if line.isEmpty || line.hasPrefix("#") { continue }
            let parts = line.split(separator: " ")
            guard parts.count == 2,
                  let millis = Int(parts[0]), millis >= 0,
                  let data = data(fromHex: String(parts[1])),
                  let event = try? InputEvent.decode(data)
            else { throw RecordingError.malformed(line: index + 1) }
            result.append(RecordedEvent(millis: millis, event: event))
        }
        return result
    }

    static func data(fromHex hex: String) -> Data? {
        guard hex.count % 2 == 0 else { return nil }
        var bytes: [UInt8] = []
        var index = hex.startIndex
        while index < hex.endIndex {
            let next = hex.index(index, offsetBy: 2)
            guard let byte = UInt8(hex[index..<next], radix: 16) else { return nil }
            bytes.append(byte)
            index = next
        }
        return Data(bytes)
    }
}
