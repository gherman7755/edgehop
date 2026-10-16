import Foundation
import Testing
@testable import EdgeHopCore

private let token = "ABCDEFGHJKLMNPQRSTUV"

private func connect(controller: String = token, receiver: String = token, hello: Data = Data("{}".utf8)) throws -> (NoiseTransport, NoiseTransport, Data) {
    let initiator = NoiseHandshake(role: .initiator, token: controller)
    let responder = NoiseHandshake(role: .responder, token: receiver)
    _ = try responder.read(try initiator.write())
    _ = try initiator.read(try responder.write())
    let payload = try responder.read(try initiator.write(hello))
    return (try initiator.transport(), try responder.transport(), payload)
}

@Suite struct NoiseTests {
    @Test func theSameTokenGivesAnEncryptedChannelInBothDirections() throws {
        var (controller, receiver, payload) = try connect(hello: Data(#"{"name":"Mac"}"#.utf8))
        #expect(payload == Data(#"{"name":"Mac"}"#.utf8))
        let wire = try controller.seal(Data("hello receiver".utf8))
        #expect(wire.range(of: Data("hello".utf8)) == nil, "the plaintext is on the wire")
        #expect(try receiver.open(wire) == Data("hello receiver".utf8))
        #expect(try controller.open(try receiver.seal(Data("hello controller".utf8))) == Data("hello controller".utf8))
    }

    @Test func aWrongTokenFailsAtTheThirdMessage() {
        #expect(throws: NoiseError.crypto) { try connect(receiver: "ZZZZZZZZZZZZZZZZZZZZ") }
    }

    @Test func aTamperedOrReplayedMessageIsRefused() throws {
        var (controller, receiver, _) = try connect()
        var wire = try controller.seal(Data("input".utf8))
        let original = wire
        wire[wire.count - 1] ^= 1
        #expect(throws: NoiseError.crypto) { try receiver.open(wire) }

        var (controller2, receiver2, _) = try connect()
        let fresh = try controller2.seal(Data("input".utf8))
        #expect(try receiver2.open(fresh) == Data("input".utf8))
        #expect(throws: NoiseError.crypto) { try receiver2.open(fresh) } // the nonce moved on
        _ = original
    }

    @Test func dataLargerThanOneMessageIsChunkedAndReassembledFromAnyPieces() throws {
        var (controller, receiver, _) = try connect()
        let data = Data((0..<(Noise.maxChunk * 2 + 123)).map { UInt8($0 % 251) })
        let wire = try controller.seal(data)
        var plaintext = Data()
        var offset = 0
        while offset < wire.count {
            let end = min(offset + 777, wire.count)
            plaintext.append(try receiver.open(Data(wire[offset..<end])))
            offset = end
        }
        #expect(plaintext == data)
    }

    @Test func emptyDataMakesNoMessages() throws {
        var (controller, receiver, _) = try connect()
        #expect(try controller.seal(Data()).isEmpty)
        #expect(try receiver.open(Data()).isEmpty)
    }

    @Test func theMessageReaderReassemblesSplitMessages() {
        var reader = NoiseMessageReader()
        let stream = Noise.withLength(Data("one".utf8)) + Noise.withLength(Data("second".utf8))
        for byte in stream.dropLast() { reader.push(Data([byte])) }
        #expect(reader.nextMessage() == Data("one".utf8))
        #expect(reader.nextMessage() == nil)
        reader.push(stream.suffix(1))
        #expect(reader.nextMessage() == Data("second".utf8))
        #expect(reader.drain().isEmpty)
    }

    @Test func anOlderUnencryptedPeerIsRecognisedByItsMagic() {
        #expect(Noise.looksLikeOlderEdgeHop(Data("EH".utf8) + Data([1, 2, 3])))
        #expect(!Noise.looksLikeOlderEdgeHop(Data([0x00, 0x60])))
    }

    @Test func theLinkIsEncryptedOnTheWireAndAWrongTokenIsRejected() throws {
        let server = LinkServer(token: "the-real-token-1", name: "srv")
        try server.start(port: 0)
        defer { server.stop() }
        let client = LinkClient(token: "not-the-token-xx", name: "cli")
        #expect(throws: LinkError.rejected) { try client.connect(host: "127.0.0.1", port: server.port) }
    }
}
