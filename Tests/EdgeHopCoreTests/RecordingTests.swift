import Foundation
import Testing
@testable import EdgeHopCore

@Suite struct RecordingTests {
    @Test func formatsAndParsesBack() throws {
        let events = [
            RecordedEvent(millis: 0, event: .mouseMoveAbsolute(x: 10, y: 20)),
            RecordedEvent(millis: 16, event: .mouseButton(.left, isDown: true)),
            RecordedEvent(millis: 1200, event: .key(code: 0, isDown: false, modifiers: [.control])),
        ]
        let text = "# header\n\n" + events.map(Recording.line).joined(separator: "\n") + "\n"
        #expect(try Recording.parse(text) == events)
    }

    @Test func reportsTheMalformedLineNumber() {
        let good = Recording.line(RecordedEvent(millis: 1, event: .scroll(horizontal: 0, vertical: 120)))
        #expect(throws: RecordingError.malformed(line: 3)) {
            try Recording.parse("# c\n\(good)\n5 not-hex\n")
        }
        #expect(throws: RecordingError.malformed(line: 1)) { try Recording.parse("-1 00") }
    }
}
