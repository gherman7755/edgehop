import Foundation
import Testing
@testable import EdgeHopIPC

/// In-memory transport: `feed` queues input lines, `readLine` blocks until one arrives (or `finish`).
private final class MemoryTransport: LineTransport {
    private let lock = NSLock()
    private let available = DispatchSemaphore(value: 0)
    private var input: [String?] = []
    private var output: [String] = []

    func feed(_ line: String) { lock.lock(); input.append(line); lock.unlock(); available.signal() }
    func finish() { lock.lock(); input.append(nil); lock.unlock(); available.signal() }

    func readLine() -> String? {
        available.wait()
        lock.lock(); defer { lock.unlock() }
        return input.removeFirst()
    }

    func write(_ line: String) { lock.lock(); output.append(line); lock.unlock() }

    var lines: [String] { lock.lock(); defer { lock.unlock() }; return output }

    /// Parsed output lines.
    var messages: [[String: Any]] {
        lines.compactMap { try? JSONSerialization.jsonObject(with: Data($0.utf8)) as? [String: Any] }
    }
}

private final class FakeHandler: IPCHandler {
    var calls: [(String, [String: Any])] = []
    var shutdowns = 0
    var emit: IPCEmit?

    func handle(method: String, params: [String: Any]) throws -> [String: Any] {
        calls.append((method, params))
        switch method {
        case "echo": return ["params": params]
        case "bad": throw IPCError.invalidParams("nope")
        case "busy": throw IPCError.busy("already running")
        case "boom": throw NSError(domain: "x", code: 1, userInfo: [NSLocalizedDescriptionKey: "kaput"])
        case "emit":
            emit?("log", ["line": "hello"])
            return [:]
        default: throw IPCError.unknownMethod("unknown method \(method)")
        }
    }

    func shutdown() { shutdowns += 1 }
}

/// Runs the server on a background thread over a MemoryTransport and returns once it has stopped.
private func session(_ lines: [String], endWithEOF: Bool = true) -> (messages: [[String: Any]], handler: FakeHandler) {
    let transport = MemoryTransport()
    let handler = FakeHandler()
    let server = IPCServer(transport: transport) { emit in handler.emit = emit; return handler }
    let done = DispatchSemaphore(value: 0)
    Thread { server.run(); done.signal() }.start()
    for line in lines { transport.feed(line) }
    if endWithEOF { transport.finish() }
    #expect(done.wait(timeout: .now() + 3) == .success, "server did not stop")
    return (transport.messages, handler)
}

@Suite struct IPCServerTests {
    @Test func answersEachRequestWithItsId() throws {
        let (messages, handler) = session([
            #"{"id":1,"method":"echo","params":{"a":1}}"#,
            #"{"id":"abc","method":"echo"}"#,
        ])
        #expect(messages.count == 2)
        #expect(messages[0]["id"] as? Int == 1)
        #expect(((messages[0]["result"] as? [String: Any])?["params"] as? [String: Any])?["a"] as? Int == 1)
        #expect(messages[1]["id"] as? String == "abc")
        #expect(handler.calls.map(\.0) == ["echo", "echo"])
        #expect(handler.shutdowns == 1) // end of input stops everything
    }

    @Test func mapsHandlerErrorsToCodes() {
        let (messages, _) = session([
            #"{"id":1,"method":"bad"}"#,
            #"{"id":2,"method":"busy"}"#,
            #"{"id":3,"method":"nothing"}"#,
            #"{"id":4,"method":"boom"}"#,
        ])
        let codes = messages.map { ($0["error"] as? [String: Any])?["code"] as? String }
        #expect(codes == ["invalid_params", "busy", "unknown_method", "internal"])
        #expect(((messages[3]["error"] as? [String: Any])?["message"] as? String)?.contains("kaput") == true)
    }

    @Test func survivesMalformedInput() {
        let (messages, handler) = session([
            "not json at all",
            "[1,2,3]",
            #"{"id":1}"#,
            #"{"id":true,"method":"echo"}"#,
            #"{"id":2,"method":"echo","params":[1]}"#,
            "",
            #"{"id":3,"method":"echo"}"#,
        ])
        let errors = messages.map { ($0["error"] as? [String: Any])?["code"] as? String }
        #expect(errors == ["parse_error", "parse_error", "invalid_request", "invalid_request", "invalid_params", nil])
        #expect(messages[0]["id"] is NSNull)
        #expect(messages.last?["id"] as? Int == 3) // still serving after the garbage
        #expect(handler.calls.count == 1)
    }

    @Test func shutdownRespondsThenStops() {
        let (messages, handler) = session([
            #"{"id":9,"method":"core.shutdown"}"#,
            #"{"id":10,"method":"echo"}"#, // never handled
        ], endWithEOF: false)
        #expect(messages.count == 1)
        #expect(messages[0]["id"] as? Int == 9)
        #expect(messages[0]["result"] != nil)
        #expect(handler.shutdowns == 1)
        #expect(handler.calls.isEmpty)
    }

    @Test func emitsEventsAsLinesWithoutAnId() {
        let (messages, _) = session([#"{"id":1,"method":"emit"}"#])
        let event = messages.first { $0["event"] != nil }
        #expect(event?["event"] as? String == "log")
        #expect((event?["data"] as? [String: Any])?["line"] as? String == "hello")
        #expect(event?["id"] == nil)
        #expect(messages.contains { $0["id"] as? Int == 1 })
    }

    @Test func everyOutputLineIsOneJSONObject() {
        let (messages, _) = session([#"{"id":1,"method":"echo","params":{"s":"a\nb \"q\" /x"}}"#])
        #expect(messages.count == 1) // an embedded newline must stay escaped inside the string
    }
}
