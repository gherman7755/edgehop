import ApplicationServices
import CoreGraphics
import EdgeHopCore
import Foundation

/// Typed, strict access to a request's `params`. Missing keys fall back to defaults; keys of the wrong
/// type are errors (a boolean is never accepted as a number or the other way round).
struct Params {
    let raw: [String: Any]

    private static func isBoolean(_ value: Any) -> Bool {
        guard let number = value as? NSNumber else { return false }
        return CFGetTypeID(number) == CFBooleanGetTypeID()
    }

    func string(_ key: String) throws -> String? {
        guard let value = raw[key] else { return nil }
        guard let text = value as? String else { throw IPCError.invalidParams("\"\(key)\" must be a string") }
        return text
    }

    func requiredString(_ key: String) throws -> String {
        guard let text = try string(key) else { throw IPCError.invalidParams("\"\(key)\" is required") }
        return text
    }

    func bool(_ key: String, default fallback: Bool) throws -> Bool {
        guard let value = raw[key] else { return fallback }
        guard Self.isBoolean(value), let flag = value as? Bool else {
            throw IPCError.invalidParams("\"\(key)\" must be true or false")
        }
        return flag
    }

    func int(_ key: String, in range: ClosedRange<Int>) throws -> Int? {
        guard let value = raw[key] else { return nil }
        guard !Self.isBoolean(value), let number = value as? NSNumber, number.doubleValue == number.doubleValue.rounded(),
              range.contains(number.intValue)
        else { throw IPCError.invalidParams("\"\(key)\" must be an integer from \(range.lowerBound) to \(range.upperBound)") }
        return number.intValue
    }
}

/// The macOS core: every method of docs/IPC.md on top of the receiver and controller sessions.
public final class CoreService: IPCHandler {
    public static let version = "0.3.0"
    static let minimumTokenLength = 12

    private let emit: IPCEmit
    private let controller = ControllerSession()
    private let lock = NSLock()
    private var receiver: ReceiverSession?
    private var browser: PeerBrowser?
    private var peers: [DiscoveredPeer] = []

    public init(emit: @escaping IPCEmit) {
        self.emit = emit
        controller.onState = { [weak self] state in self?.emit("controller.state", Self.encode(state)) }
        controller.onLog = { [weak self] line in self?.emit("log", ["source": "controller", "line": line]) }
        controller.onLatency = { [weak self] ms in self?.emit("latency", ["ms": ms]) }
    }

    // MARK: Dispatch

    public func handle(method: String, params raw: [String: Any]) throws -> [String: Any] {
        let params = Params(raw: raw)
        switch method {
        case "core.hello":
            return [
                "name": "EdgeHop core", "version": Self.version, "ipcVersion": 1,
                "platform": "macos", "deviceName": Device.name,
            ]
        case "permissions.get":
            return ["accessibility": AXIsProcessTrusted(), "inputMonitoring": CGPreflightListenEventAccess()]
        case "permissions.request":
            switch try params.requiredString("kind") {
            case "accessibility":
                let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
                _ = AXIsProcessTrustedWithOptions(options)
            case "inputMonitoring":
                _ = CGRequestListenEventAccess()
            default:
                throw IPCError.invalidParams("\"kind\" must be \"accessibility\" or \"inputMonitoring\"")
            }
            return [:]
        case "token.generate":
            return ["token": TokenStore.generate()]
        case "peers.watch": return try watchPeers(enabled: params.bool("enabled", default: true))
        case "peers.list": return ["peers": currentPeers().map { ["name": $0.name] }]
        case "controller.start": return try startController(params)
        case "controller.stop":
            controller.stop()
            return [:]
        case "controller.state": return Self.encode(controller.state)
        case "receiver.start": return try startReceiver(params)
        case "receiver.stop":
            stopReceiver()
            return [:]
        case "receiver.state": return Self.encode(receiverSession()?.state ?? .stopped)
        default:
            throw IPCError.unknownMethod("unknown method \(method)")
        }
    }

    public func shutdown() {
        controller.stop()
        stopReceiver()
        lock.lock()
        browser?.stop()
        browser = nil
        lock.unlock()
    }

    // MARK: Peers

    private func currentPeers() -> [DiscoveredPeer] {
        lock.lock(); defer { lock.unlock() }
        return peers
    }

    private func watchPeers(enabled: Bool) -> [String: Any] {
        lock.lock(); defer { lock.unlock() }
        if enabled, browser == nil {
            let browser = PeerBrowser(ownName: Device.name)
            browser.onChange = { [weak self] found in
                guard let self else { return }
                self.lock.lock()
                self.peers = found
                self.lock.unlock()
                self.emit("peers.changed", ["peers": found.map { ["name": $0.name] }])
            }
            browser.start()
            self.browser = browser
        } else if !enabled {
            browser?.stop()
            browser = nil
            peers = []
        }
        return [:]
    }

    // MARK: Controller

    private func startController(_ params: Params) throws -> [String: Any] {
        let token = try params.requiredString("token")
        guard token.count >= Self.minimumTokenLength else {
            throw IPCError.invalidParams("token must be at least \(Self.minimumTokenLength) characters")
        }
        let peerName = try params.string("peer")
        let host = try params.string("host")
        guard (peerName == nil) != (host == nil) else {
            throw IPCError.invalidParams("give exactly one of \"peer\" and \"host\"")
        }
        let port = UInt16(try params.int("port", in: 1...65535) ?? 47474)
        guard let side = Side(rawValue: try params.string("side") ?? "right") else {
            throw IPCError.invalidParams("\"side\" must be left, right, top or bottom")
        }
        let target: ControllerConfig.Target
        if let peerName {
            guard let peer = currentPeers().first(where: { $0.name == peerName }) else {
                throw IPCError.invalidParams("no computer named \"\(peerName)\" was found nearby (is peers.watch on?)")
            }
            target = .peer(peer)
        } else if let host {
            let trimmed = host.trimmingCharacters(in: .whitespaces)
            guard !trimmed.isEmpty, !trimmed.contains("@") else {
                throw IPCError.invalidParams("\"host\" must be an address without \"user@\"")
            }
            target = .host(trimmed, port: port)
        } else {
            throw IPCError.invalidParams("give exactly one of \"peer\" and \"host\"")
        }
        let config = ControllerConfig(
            target: target, token: token, side: side,
            switching: try params.bool("switching", default: true),
            forwardKeys: try params.bool("forwardKeys", default: false),
            debug: try params.bool("debug", default: false)
        )
        switch controller.state {
        case .connecting, .connected: throw IPCError.busy("a controller session is already running")
        case .idle, .failed: break
        }
        controller.start(config)
        return [:]
    }

    // MARK: Receiver

    private func receiverSession() -> ReceiverSession? {
        lock.lock(); defer { lock.unlock() }
        return receiver
    }

    private func startReceiver(_ params: Params) throws -> [String: Any] {
        let token = try params.requiredString("token")
        guard token.count >= Self.minimumTokenLength else {
            throw IPCError.invalidParams("token must be at least \(Self.minimumTokenLength) characters")
        }
        let port = UInt16(try params.int("port", in: 0...65535) ?? 47474)
        let allowKeys = try params.bool("allowKeys", default: false)
        let debug = try params.bool("debug", default: false)
        if let existing = receiverSession(), existing.state != .stopped {
            throw IPCError.busy("the receiver is already running")
        }
        let session = ReceiverSession(token: token, allowKeys: allowKeys, debug: debug)
        session.onState = { [weak self] state in self?.emit("receiver.state", Self.encode(state)) }
        session.onLog = { [weak self] line in self?.emit("log", ["source": "receiver", "line": line]) }
        session.onPanic = { [weak self] in self?.emit("log", ["source": "receiver", "line": "panic chord: stopped"]) }
        do {
            try session.start(port: port)
        } catch {
            throw IPCError.internal("cannot listen on port \(port): \(error)")
        }
        lock.lock()
        receiver = session
        lock.unlock()
        if case let .listening(actual) = session.state { return ["port": Int(actual)] }
        return ["port": Int(port)]
    }

    private func stopReceiver() {
        lock.lock()
        let session = receiver
        receiver = nil
        lock.unlock()
        session?.stop()
    }

    // MARK: State encoding

    static func encode(_ state: ControllerState) -> [String: Any] {
        switch state {
        case .idle: return ["state": "idle"]
        case .connecting: return ["state": "connecting"]
        case let .connected(peer, remoteActive): return ["state": "connected", "peer": peer, "remoteActive": remoteActive]
        case let .failed(message): return ["state": "failed", "message": message]
        }
    }

    static func encode(_ state: ReceiverState) -> [String: Any] {
        switch state {
        case .stopped: return ["state": "stopped"]
        case let .listening(port): return ["state": "listening", "port": Int(port)]
        case let .connected(peer): return ["state": "connected", "peer": peer]
        }
    }
}
