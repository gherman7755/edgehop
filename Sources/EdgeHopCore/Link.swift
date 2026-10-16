import CryptoKit
import Foundation
import Network

// M4: plaintext TCP link between two Macs. Authentication only (shared token, challenge-response):
// input events are NOT encrypted. Use on a trusted LAN until M6 (Noise) replaces this transport.

/// JSON payload of `hello` frames, used for the whole handshake:
/// server -> client `{nonce}`; client -> server `{proof, name, monitors}`; server -> client `{accepted}`.
public struct HelloMessage: Codable, Equatable, Sendable {
    public var version: Int
    public var name: String
    public var nonce: String?
    public var proof: String?
    public var accepted: Bool?
    public var monitors: [MonitorInfo]?
    /// `"hid"` if the sender sends and understands HID key codes (`Keymap`). Absent: macOS keycodes, so no keys.
    public var keymap: String?

    /// True if the sender announced HID key codes.
    public var speaksHIDKeys: Bool { keymap == Keymap.name }

    public init(
        version: Int = 1, name: String, nonce: String? = nil, proof: String? = nil,
        accepted: Bool? = nil, monitors: [MonitorInfo]? = nil, keymap: String? = nil
    ) {
        self.version = version
        self.name = name
        self.nonce = nonce
        self.proof = proof
        self.accepted = accepted
        self.monitors = monitors
        self.keymap = keymap
    }
}

/// JSON payload of `control` frames.
public enum ControlMessage: String, Codable, Equatable, Sendable {
    /// Release every key and button the sender pressed here (sent when control returns to the sender).
    case releaseAll = "release_all"
}

private struct ControlFrame: Codable { var t: ControlMessage }

public enum LinkError: Error, Equatable {
    case timeout
    case rejected
    case protocolError(String)
    case connectionFailed(String)
}

/// Challenge-response so the shared token itself never crosses the network.
public enum LinkAuth {
    private static func key(_ token: String) -> SymmetricKey { SymmetricKey(data: Data(token.utf8)) }
    private static func message(_ nonce: Data) -> Data { Data("edgehop-link-v1|".utf8) + nonce }

    public static func makeNonce() -> Data {
        var generator = SystemRandomNumberGenerator()
        return Data((0..<32).map { _ in UInt8.random(in: 0...255, using: &generator) })
    }

    public static func proof(token: String, nonce: Data) -> Data {
        Data(HMAC<SHA256>.authenticationCode(for: message(nonce), using: key(token)))
    }

    /// Constant-time comparison (CryptoKit).
    public static func verify(token: String, nonce: Data, proof: Data) -> Bool {
        HMAC<SHA256>.isValidAuthenticationCode(proof, authenticating: message(nonce), using: key(token))
    }
}

/// The panic chord stops forwarding on either machine: Ctrl + Option + Cmd + Esc.
public enum PanicChord {
    public static let escapeKeyCode: UInt16 = Keymap.hidEscape // events carry HID key codes

    public static func isPanic(_ event: InputEvent) -> Bool {
        guard case let .key(code, isDown, modifiers) = event else { return false }
        return isDown && code == escapeKeyCode && modifiers.isSuperset(of: [.control, .option, .command])
    }
}

enum LinkTransport {
    static let serviceType = "_edgehop._tcp"

    static func parameters() -> NWParameters {
        let tcp = NWProtocolTCP.Options()
        tcp.noDelay = true // small input records: never wait for Nagle
        return NWParameters(tls: nil, tcp: tcp)
    }
}

/// Frames over one `NWConnection`. All callbacks run on `queue`.
public final class LinkConnection {
    private let nw: NWConnection
    private let queue: DispatchQueue
    private var decoder = FrameDecoder()
    private var closed = false

    public var onReady: (() -> Void)?
    /// NWConnection is retrying (no route, refused, unreachable...). The error says why.
    public var onWaiting: ((NWError) -> Void)?
    public var onFrame: ((Frame) -> Void)?
    public var onClosed: ((Error?) -> Void)?

    public init(_ nw: NWConnection, queue: DispatchQueue) {
        self.nw = nw
        self.queue = queue
    }

    public func start() {
        nw.stateUpdateHandler = { [weak self] state in
            guard let self else { return }
            switch state {
            case .ready:
                self.onReady?()
                self.receiveLoop()
            case let .waiting(error): self.onWaiting?(error)
            case let .failed(error): self.finish(error)
            case .cancelled: self.finish(nil)
            default: break
            }
        }
        nw.start(queue: queue)
    }

    public func send(_ frame: Frame, then: (() -> Void)? = nil) {
        nw.send(content: FrameCodec.encode(frame), completion: .contentProcessed { [weak self] error in
            if let error { self?.finish(error) }
            then?()
        })
    }

    public func cancel() { nw.cancel() }

    private func receiveLoop() {
        nw.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) { [weak self] data, _, isComplete, error in
            guard let self, !self.closed else { return }
            if let data, !data.isEmpty {
                do {
                    for frame in try self.decoder.feed(data) { self.onFrame?(frame) }
                } catch {
                    self.finish(error) // protocol violation: drop the connection
                    self.nw.cancel()
                    return
                }
            }
            if let error { self.finish(error); return }
            if isComplete { self.finish(nil); self.nw.cancel(); return }
            self.receiveLoop()
        }
    }

    private func finish(_ error: Error?) {
        guard !closed else { return }
        closed = true
        onClosed?(error)
    }
}

private func encode(_ hello: HelloMessage) -> Frame {
    Frame(type: .hello, payload: (try? JSONEncoder().encode(hello)) ?? Data())
}

private func decodeHello(_ frame: Frame) -> HelloMessage? {
    guard frame.type == .hello else { return nil }
    return try? JSONDecoder().decode(HelloMessage.self, from: frame.payload)
}

private func nowMillis() -> UInt64 { DispatchTime.now().uptimeNanoseconds / 1_000_000 }

/// Accepts one authenticated peer at a time and delivers its input events.
public final class LinkServer {
    public var onInput: ((InputEvent) -> Void)?
    public var onPeerConnected: ((HelloMessage) -> Void)?
    public var onPeerDisconnected: (() -> Void)?
    public var onControl: ((ControlMessage) -> Void)?
    public private(set) var port: UInt16 = 0

    private let token: String
    private let name: String
    private let queue = DispatchQueue(label: "edgehop.link.server")
    private var listener: NWListener?
    private var active: LinkConnection?
    private let authTimeout: TimeInterval = 5

    public init(token: String, name: String) {
        self.token = token
        self.name = name
    }

    /// Blocks until the listener is ready. `port == 0` picks a free port (see `self.port`).
    /// With `advertiseName`, the service is announced on the LAN as `_edgehop._tcp` (Bonjour).
    public func start(port requested: UInt16 = 0, advertiseName: String? = nil) throws {
        let nwPort = requested == 0 ? NWEndpoint.Port.any : NWEndpoint.Port(rawValue: requested)!
        let listener = try NWListener(using: LinkTransport.parameters(), on: nwPort)
        if let advertiseName {
            listener.service = NWListener.Service(name: advertiseName, type: LinkTransport.serviceType)
        }
        listener.newConnectionHandler = { [weak self] nw in self?.accept(nw) }
        let ready = DispatchSemaphore(value: 0)
        var failure: Error?
        listener.stateUpdateHandler = { state in
            switch state {
            case .ready: ready.signal()
            case let .failed(error): failure = error; ready.signal()
            default: break
            }
        }
        listener.start(queue: queue)
        if ready.wait(timeout: .now() + 5) == .timedOut { listener.cancel(); throw LinkError.timeout }
        if let failure { throw LinkError.connectionFailed(failure.localizedDescription) }
        self.listener = listener
        self.port = listener.port?.rawValue ?? requested
    }

    public func stop() {
        listener?.cancel()
        queue.sync { active?.cancel() }
    }

    private func accept(_ nw: NWConnection) {
        guard active == nil else { nw.cancel(); return } // one peer at a time
        let conn = LinkConnection(nw, queue: queue)
        active = conn
        let nonce = LinkAuth.makeNonce()
        var authenticated = false

        conn.onReady = { [weak conn, name] in
            conn?.send(encode(HelloMessage(name: name, nonce: nonce.base64EncodedString(), keymap: Keymap.name)))
        }
        conn.onFrame = { [weak self, weak conn] frame in
            guard let self, let conn else { return }
            if !authenticated {
                guard let hello = decodeHello(frame),
                      let proofText = hello.proof, let proof = Data(base64Encoded: proofText),
                      LinkAuth.verify(token: self.token, nonce: nonce, proof: proof)
                else {
                    conn.send(encode(HelloMessage(name: self.name, accepted: false))) { conn.cancel() }
                    return
                }
                authenticated = true
                conn.send(encode(HelloMessage(name: self.name, accepted: true, monitors: LocalDisplays.list(), keymap: Keymap.name)))
                self.onPeerConnected?(hello)
                return
            }
            switch frame.type {
            case .input:
                if let event = try? InputEvent.decode(frame.payload) { self.onInput?(event) }
            case .ping:
                conn.send(Frame(type: .pong, payload: frame.payload))
            case .control:
                if let message = try? JSONDecoder().decode(ControlFrame.self, from: frame.payload) {
                    self.onControl?(message.t)
                }
            default:
                break
            }
        }
        conn.onClosed = { [weak self, weak conn] _ in
            guard let self, let conn, self.active === conn else { return }
            self.active = nil
            if authenticated { self.onPeerDisconnected?() }
        }
        conn.start()
        queue.asyncAfter(deadline: .now() + authTimeout) { [weak conn] in
            if !authenticated { conn?.cancel() }
        }
    }
}

public final class LinkClient {
    public var onClosed: ((Error?) -> Void)?
    /// Round-trip time of a ping, in milliseconds.
    public var onPong: ((Double) -> Void)?

    private let token: String
    private let name: String
    private let queue = DispatchQueue(label: "edgehop.link.client")
    private var connection: LinkConnection?

    public init(token: String, name: String) {
        self.token = token
        self.name = name
    }

    /// Connects and authenticates (blocking). Returns the server's hello (name, monitors).
    public func connect(host: String, port: UInt16, timeout: TimeInterval = 8) throws -> HelloMessage {
        guard let nwPort = NWEndpoint.Port(rawValue: port) else { throw LinkError.protocolError("bad port") }
        return try connect(to: .hostPort(host: NWEndpoint.Host(host), port: nwPort), label: "\(host):\(port)", timeout: timeout)
    }

    /// Same, for any endpoint (a Bonjour service found by `PeerBrowser`, or host + port).
    public func connect(to endpoint: NWEndpoint, label: String, timeout: TimeInterval = 8) throws -> HelloMessage {
        let nw = NWConnection(to: endpoint, using: LinkTransport.parameters())
        let conn = LinkConnection(nw, queue: queue)
        connection = conn

        let done = DispatchSemaphore(value: 0)
        var result: Result<HelloMessage, LinkError>?
        var phase = 0 // 0 waiting for challenge, 1 waiting for accept, 2 connected
        var tcpConnected = false
        var lastWaiting: String?
        conn.onReady = { tcpConnected = true }
        conn.onWaiting = { lastWaiting = "\($0)" }
        func finish(_ r: Result<HelloMessage, LinkError>) {
            if result == nil { result = r; done.signal() }
        }

        conn.onFrame = { [weak self, weak conn] frame in
            guard let self, let conn else { return }
            switch phase {
            case 0:
                guard let hello = decodeHello(frame), let nonceText = hello.nonce,
                      let nonce = Data(base64Encoded: nonceText)
                else { return finish(.failure(.protocolError("expected challenge"))) }
                let proof = LinkAuth.proof(token: self.token, nonce: nonce).base64EncodedString()
                phase = 1
                conn.send(encode(HelloMessage(name: self.name, proof: proof, monitors: LocalDisplays.list(), keymap: Keymap.name)))
            case 1:
                guard let hello = decodeHello(frame), let accepted = hello.accepted
                else { return finish(.failure(.protocolError("expected accept"))) }
                if accepted { phase = 2; finish(.success(hello)) } else { finish(.failure(.rejected)) }
            default:
                if frame.type == .pong, frame.payload.count == 8 {
                    let sent = frame.payload.reduce(UInt64(0)) { $0 << 8 | UInt64($1) }
                    self.onPong?(Double(nowMillis() &- sent))
                }
            }
        }
        conn.onClosed = { [weak self] error in
            if phase < 2 {
                finish(.failure(.connectionFailed(error?.localizedDescription ?? "closed")))
            } else {
                self?.onClosed?(error)
            }
        }
        conn.start()

        if done.wait(timeout: .now() + timeout) == .timedOut {
            conn.cancel()
            if tcpConnected {
                throw LinkError.connectionFailed("TCP connected but the server never answered the handshake (is `edgehop-link serve` running there, same port and version?)")
            }
            throw LinkError.connectionFailed("could not reach \(label) within \(Int(timeout)) s" + (lastWaiting.map { " (network says: \($0))" } ?? " (no response: wrong address, firewall, or different network)"))
        }
        switch result! {
        case let .success(hello): return hello
        case let .failure(error):
            conn.cancel()
            throw error
        }
    }

    public func send(_ event: InputEvent) {
        connection?.send(Frame(type: .input, payload: event.encoded()))
    }

    public func sendControl(_ message: ControlMessage) {
        guard let payload = try? JSONEncoder().encode(ControlFrame(t: message)) else { return }
        connection?.send(Frame(type: .control, payload: payload))
    }

    public func sendPing() {
        var ms = nowMillis().bigEndian
        connection?.send(Frame(type: .ping, payload: Data(bytes: &ms, count: 8)))
    }

    public func close() { connection?.cancel() }
}
