import Foundation
import Testing
@testable import EdgeHopCore

@Suite struct FrameTests {
    @Test func decodesFramesSplitAcrossChunks() throws {
        let a = FrameCodec.encode(Frame(type: .input, payload: InputEvent.mouseMoveDelta(dx: 1, dy: 2).encoded()))
        let b = FrameCodec.encode(Frame(type: .ping))
        let stream = a + b
        var decoder = FrameDecoder()
        var frames: [Frame] = []
        // Feed one byte at a time: worst case for a TCP stream.
        for byte in stream { frames += try decoder.feed(Data([byte])) }
        #expect(frames.count == 2)
        #expect(frames[0].type == .input)
        #expect(try InputEvent.decode(frames[0].payload) == .mouseMoveDelta(dx: 1, dy: 2))
        #expect(frames[1] == Frame(type: .ping))
    }

    @Test func decodesTwoFramesInOneChunk() throws {
        var decoder = FrameDecoder()
        let frames = try decoder.feed(FrameCodec.encode(Frame(type: .ping)) + FrameCodec.encode(Frame(type: .pong)))
        #expect(frames.map(\.type) == [.ping, .pong])
    }

    @Test func rejectsBadMagicUnknownTypeAndHugePayload() {
        var decoder = FrameDecoder()
        #expect(throws: FrameError.badMagic) { try decoder.feed(Data([0, 0, 1, 0, 0, 0, 0, 0])) }

        var d2 = FrameDecoder()
        #expect(throws: FrameError.unknownType(0x7F)) { try d2.feed(Data([0x45, 0x48, 0x7F, 0, 0, 0, 0, 0])) }

        var d3 = FrameDecoder()
        let tooBig = UInt32(FrameCodec.maxPayload + 1)
        let header = Data([0x45, 0x48, 3, 0, UInt8(tooBig >> 24), UInt8((tooBig >> 16) & 0xFF), UInt8((tooBig >> 8) & 0xFF), UInt8(tooBig & 0xFF)])
        #expect(throws: FrameError.payloadTooLarge(Int(tooBig))) { try d3.feed(header) }
    }
}
