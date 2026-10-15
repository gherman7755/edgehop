import Foundation
import Testing
@testable import EdgeHopCore

@Suite struct InputEventCodecTests {
    @Test func roundTripsEveryKind() throws {
        let events: [InputEvent] = [
            .mouseMoveAbsolute(x: 1919, y: -3),
            .mouseMoveDelta(dx: -7, dy: 12),
            .mouseButton(.left, isDown: true),
            .mouseButton(.forward, isDown: false),
            .scroll(horizontal: -120, vertical: 360),
            .key(code: 53, isDown: true, modifiers: [.command, .shift]),
        ]
        for event in events {
            let data = event.encoded()
            #expect(data.count == InputEvent.recordSize)
            #expect(try InputEvent.decode(data) == event)
        }
    }

    @Test func encodesBigEndian() {
        let data = InputEvent.mouseMoveAbsolute(x: 0x0102_0304, y: 5).encoded()
        #expect([UInt8](data)[4...7] == [0x01, 0x02, 0x03, 0x04])
    }

    @Test func rejectsMalformedRecords() {
        #expect(throws: InputEventCodecError.wrongLength(3)) { try InputEvent.decode(Data([1, 2, 3])) }
        var bad = InputEvent.mouseMoveDelta(dx: 0, dy: 0).encoded()
        bad[0] = 99
        #expect(throws: InputEventCodecError.unknownKind(99)) { try InputEvent.decode(bad) }
    }
}
