import Foundation
import Testing
@testable import EdgeHopIPC

/// Only methods that start no session, open no socket and show no prompt: those need hardware and
/// permissions and are covered by the manual two-computer run.
@Suite struct CoreServiceTests {
    private func service() -> CoreService { CoreService { _, _ in } }

    private func call(_ method: String, _ params: [String: Any] = [:]) throws -> [String: Any] {
        try service().handle(method: method, params: params)
    }

    private func expectInvalid(_ method: String, _ params: [String: Any], containing text: String, sourceLocation: SourceLocation = #_sourceLocation) {
        do {
            _ = try call(method, params)
            Issue.record("expected invalid_params for \(params)", sourceLocation: sourceLocation)
        } catch let IPCError.invalidParams(message) {
            #expect(message.contains(text), "message was: \(message)", sourceLocation: sourceLocation)
        } catch {
            Issue.record("wrong error \(error)", sourceLocation: sourceLocation)
        }
    }

    @Test func helloDescribesTheCore() throws {
        let result = try call("core.hello")
        #expect(result["ipcVersion"] as? Int == 1)
        #expect(result["platform"] as? String == "macos")
        #expect((result["deviceName"] as? String)?.isEmpty == false)
    }

    @Test func tokensAreTwentyCharactersAndDifferent() throws {
        let a = try call("token.generate")["token"] as? String
        let b = try call("token.generate")["token"] as? String
        #expect(a?.count == 20)
        #expect(a != b)
    }

    @Test func permissionsAreBooleans() throws {
        let result = try call("permissions.get")
        #expect(result["accessibility"] is Bool)
        #expect(result["inputMonitoring"] is Bool)
    }

    @Test func idleStatesAndHarmlessStops() throws {
        #expect(try call("controller.state")["state"] as? String == "idle")
        #expect(try call("receiver.state")["state"] as? String == "stopped")
        #expect(try call("controller.stop").isEmpty)
        #expect(try call("receiver.stop").isEmpty)
        #expect((try call("peers.list")["peers"] as? [Any])?.isEmpty == true)
    }

    @Test func unknownMethodIsReported() {
        #expect(throws: IPCError.unknownMethod("unknown method nope")) { try call("nope") }
    }

    @Test func controllerStartValidatesParams() {
        let token = "ABCDEFGHJKLMNPQRSTUV"
        expectInvalid("controller.start", [:], containing: "token")
        expectInvalid("controller.start", ["token": "short", "host": "10.0.0.2"], containing: "at least 12")
        expectInvalid("controller.start", ["token": 12345], containing: "string")
        expectInvalid("controller.start", ["token": token], containing: "exactly one")
        expectInvalid("controller.start", ["token": token, "host": "10.0.0.2", "peer": "Mac"], containing: "exactly one")
        expectInvalid("controller.start", ["token": token, "host": "me@10.0.0.2"], containing: "user@")
        expectInvalid("controller.start", ["token": token, "host": "10.0.0.2", "side": "up"], containing: "side")
        expectInvalid("controller.start", ["token": token, "host": "10.0.0.2", "port": 70000], containing: "port")
        expectInvalid("controller.start", ["token": token, "host": "10.0.0.2", "port": true], containing: "port")
        expectInvalid("controller.start", ["token": token, "host": "10.0.0.2", "switching": 1], containing: "switching")
        expectInvalid("controller.start", ["token": token, "host": "10.0.0.2", "forwardKeys": "yes"], containing: "forwardKeys")
        expectInvalid("controller.start", ["token": token, "peer": "Nobody"], containing: "no computer named")
    }

    @Test func receiverStartValidatesParams() {
        expectInvalid("receiver.start", [:], containing: "token")
        expectInvalid("receiver.start", ["token": "short"], containing: "at least 12")
        expectInvalid("receiver.start", ["token": "ABCDEFGHJKLMNPQRSTUV", "port": -1], containing: "port")
        expectInvalid("receiver.start", ["token": "ABCDEFGHJKLMNPQRSTUV", "allowKeys": "no"], containing: "allowKeys")
    }

    @Test func permissionRequestRejectsUnknownKinds() {
        expectInvalid("permissions.request", ["kind": "camera"], containing: "kind")
        expectInvalid("permissions.request", [:], containing: "kind")
    }
}
