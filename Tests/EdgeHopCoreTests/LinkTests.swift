import Foundation
import Testing
@testable import EdgeHopCore

private final class Box<T> {
    private let lock = NSLock()
    private var value: T
    init(_ value: T) { self.value = value }
    func mutate(_ body: (inout T) -> Void) { lock.lock(); body(&value); lock.unlock() }
    var current: T { lock.lock(); defer { lock.unlock() }; return value }
}

/// Real TCP on 127.0.0.1 (random port).
@Suite(.serialized) struct LinkLoopbackTests {
    @Test func authenticatesAndDeliversInputInOrder() throws {
        let server = LinkServer(token: "loopback-secret-1", name: "srv")
        let received = Box<[InputEvent]>([])
        let enough = DispatchSemaphore(value: 0)
        server.onInput = { event in
            received.mutate { $0.append(event) }
            if received.current.count == 3 { enough.signal() }
        }
        try server.start(port: 0)
        defer { server.stop() }

        let client = LinkClient(token: "loopback-secret-1", name: "cli")
        let hello = try client.connect(host: "127.0.0.1", port: server.port)
        #expect(hello.name == "srv")
        defer { client.close() }

        let sent: [InputEvent] = [
            .mouseMoveDelta(dx: 3, dy: -4),
            .mouseButton(.left, isDown: true),
            .scroll(horizontal: 0, vertical: 120),
        ]
        for event in sent { client.send(event) }
        #expect(enough.wait(timeout: .now() + 3) == .success)
        #expect(received.current == sent)
    }

    @Test func bothSidesLearnThatTheOtherSpeaksHIDKeyCodes() throws {
        let server = LinkServer(token: "loopback-secret-3", name: "srv")
        let peerKeymap = Box<String?>(nil)
        let connected = DispatchSemaphore(value: 0)
        server.onPeerConnected = { hello in
            peerKeymap.mutate { $0 = hello.keymap }
            connected.signal()
        }
        try server.start(port: 0)
        defer { server.stop() }
        let client = LinkClient(token: "loopback-secret-3", name: "cli")
        let hello = try client.connect(host: "127.0.0.1", port: server.port)
        defer { client.close() }
        #expect(hello.speaksHIDKeys)
        #expect(connected.wait(timeout: .now() + 3) == .success)
        #expect(peerKeymap.current == "hid")
    }

    @Test func rejectsAWrongToken() throws {
        let server = LinkServer(token: "the-real-token-1", name: "srv")
        let got = Box(0)
        server.onInput = { _ in got.mutate { $0 += 1 } }
        try server.start(port: 0)
        defer { server.stop() }

        let client = LinkClient(token: "not-the-token-xx", name: "cli")
        #expect(throws: LinkError.rejected) { try client.connect(host: "127.0.0.1", port: server.port) }
        #expect(got.current == 0)
    }

    @Test func answersPingWithTheSameTimestamp() throws {
        let server = LinkServer(token: "loopback-secret-2", name: "srv")
        try server.start(port: 0)
        defer { server.stop() }

        let client = LinkClient(token: "loopback-secret-2", name: "cli")
        let rtt = Box<Double?>(nil)
        let got = DispatchSemaphore(value: 0)
        client.onPong = { ms in rtt.mutate { $0 = ms }; got.signal() }
        _ = try client.connect(host: "127.0.0.1", port: server.port)
        defer { client.close() }

        client.sendPing()
        #expect(got.wait(timeout: .now() + 3) == .success)
        let value = try #require(rtt.current)
        #expect(value >= 0 && value < 1000)
    }

    @Test func serverReportsDisconnectAndAcceptsANewPeer() throws {
        let server = LinkServer(token: "loopback-secret-3", name: "srv")
        let connected = DispatchSemaphore(value: 0)
        let disconnected = DispatchSemaphore(value: 0)
        server.onPeerConnected = { _ in connected.signal() }
        server.onPeerDisconnected = { disconnected.signal() }
        try server.start(port: 0)
        defer { server.stop() }

        let first = LinkClient(token: "loopback-secret-3", name: "one")
        _ = try first.connect(host: "127.0.0.1", port: server.port)
        #expect(connected.wait(timeout: .now() + 3) == .success)
        first.close()
        #expect(disconnected.wait(timeout: .now() + 3) == .success)

        let second = LinkClient(token: "loopback-secret-3", name: "two")
        _ = try second.connect(host: "127.0.0.1", port: server.port)
        #expect(connected.wait(timeout: .now() + 3) == .success)
        second.close()
    }
}

@Suite struct LinkAuthTests {
    @Test func acceptsOnlyTheMatchingTokenAndNonce() {
        let nonce = LinkAuth.makeNonce()
        let proof = LinkAuth.proof(token: "correct-horse-battery", nonce: nonce)
        #expect(LinkAuth.verify(token: "correct-horse-battery", nonce: nonce, proof: proof))
        #expect(!LinkAuth.verify(token: "wrong-token-xxxxxxx", nonce: nonce, proof: proof))
        #expect(!LinkAuth.verify(token: "correct-horse-battery", nonce: LinkAuth.makeNonce(), proof: proof))
        #expect(nonce.count == 32)
        #expect(nonce != LinkAuth.makeNonce())
    }

    @Test func helloRoundTripsThroughJSON() throws {
        let hello = HelloMessage(name: "mac", proof: "abc", monitors: [MonitorInfo(id: "d1", x: 0, y: 0, width: 1920, height: 1080)])
        let decoded = try JSONDecoder().decode(HelloMessage.self, from: JSONEncoder().encode(hello))
        #expect(decoded == hello)
    }

    @Test func panicChordNeedsAllThreeModifiersAndEscapeDown() {
        let all: Modifiers = [.control, .option, .command]
        let escape = Keymap.hidEscape // key events carry HID usages
        #expect(PanicChord.isPanic(.key(code: escape, isDown: true, modifiers: all)))
        #expect(!PanicChord.isPanic(.key(code: escape, isDown: false, modifiers: all)))
        #expect(!PanicChord.isPanic(.key(code: escape, isDown: true, modifiers: [.control, .option])))
        #expect(!PanicChord.isPanic(.key(code: 0x14, isDown: true, modifiers: all))) // Q
        #expect(!PanicChord.isPanic(.mouseButton(.left, isDown: true)))
    }
}
