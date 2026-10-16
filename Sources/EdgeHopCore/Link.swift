import Foundation
import Network

// The TCP link between two computers. Every connection starts with an encrypted handshake (`Noise.swift`,
// Noise_XXpsk3 with the shared token as the pre-shared key): only a computer that knows the token gets through,
// and everything after it is encrypted and authenticated.

/// JSON payload of the hellos: the controller's, carried as the payload of the last handshake message, and the
/// receiver's answer, the first encrypted frame (`accepted`, with its name and monitors).
public struct HelloMessage: Codable, Equatable, Sendable {
    public var version: Int
    public var name: String
    public var accepted: Bool?
    public var monitors: [MonitorInfo]?
    /// `"hid"` if the sender sends and understands HID key codes (`Keymap`). Absent: macOS keycodes, so no keys.
    public var keymap: String?
    /// `true` (in a receiver's hello) if this computer cannot apply input right now, e.g. macOS permission missing.
    /// Absent: it can.
    public var inputBlocked: Bool?

    /// True if the sender announced HID key codes.
    public var speaksHIDKeys: Bool { keymap == Keymap.name }

    public init(version: Int = 2, name: String, accepted: Bool? = nil, monitors: [MonitorInfo]? = nil, keymap: String? = nil, inputBlocked: Bool? = nil) {
        self.version = version
        self.name = name
        self.accepted = accepted
        self.monitors = monitors
        self.keymap = keymap
        self.inputBlocked = inputBlocked
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

/// Frames over one `NWConnection`, encrypted: the Noise handshake runs first, then every frame travels inside
/// Noise transport messages. All callbacks run on `queue`.
public final class LinkConnection {
    private let nw: NWConnection
    private let queue: DispatchQueue
    private var decoder = FrameDecoder()
    private var closed = false

    private var handshake: NoiseHandshake?
    private var handshakeReader = NoiseMessageReader()
    private var transport: NoiseTransport?
    private let sendLock = NSLock()
    private var receivedAnything = false

    public var onReady: (() -> Void)?
    /// NWConnection is retrying (no route, refused, unreachable...). The error says why.
    public var onWaiting: ((NWError) -> Void)?
    /// The handshake succeeded: the argument is the payload of the last handshake message, which for the receiver is
    /// the controller's hello (authenticated: only a computer that knows the token can have sent it).
    public var onSecured: ((Data) -> Void)?
    /// What the controller says about itself in its handshake message (the controller's side only).
    public var handshakePayload: (() -> Data)?
    public var onFrame: ((Frame) -> Void)?
    public var onClosed: ((Error?) -> Void)?
    /// True once the handshake has succeeded.
    public var isSecured: Bool { transport != nil }

    public init(_ nw: NWConnection, queue: DispatchQueue, role: NoiseHandshake.Role, token: String) {
        self.nw = nw
        self.queue = queue
        self.handshake = NoiseHandshake(role: role, token: token)
    }

    public func start() {
        nw.stateUpdateHandler = { [weak self] state in
            guard let self else { return }
            switch state {
            case .ready:
                self.onReady?()
                self.beginHandshake()
                self.receiveLoop()
            case let .waiting(error): self.onWaiting?(error)
            case let .failed(error): self.finish(error)
            case .cancelled: self.finish(nil)
            default: break
            }
        }
        nw.start(queue: queue)
    }

    /// Frames can only be sent once the handshake is done.
    public func send(_ frame: Frame, then: (() -> Void)? = nil) {
        sendLock.lock()
        defer { sendLock.unlock() }
        guard var transport else { then?(); return }
        guard let sealed = try? transport.seal(FrameCodec.encode(frame)) else { finish(NoiseError.crypto); nw.cancel(); return }
        self.transport = transport
        nw.send(content: sealed, completion: .contentProcessed { [weak self] error in
            if let error { self?.finish(error) }
            then?()
        })
    }

    public func cancel() { nw.cancel() }

    private func beginHandshake() {
        guard let handshake, handshake.role == .initiator else { return }
        guard let first = try? handshake.write() else { fail(NoiseError.crypto); return }
        nw.send(content: Noise.withLength(first), completion: .contentProcessed { [weak self] error in
            if let error { self?.finish(error) }
        })
    }

    private func fail(_ error: Error) {
        finish(error)
        nw.cancel()
    }

    private func receiveLoop() {
        nw.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) { [weak self] data, _, isComplete, error in
            guard let self, !self.closed else { return }
            if let data, !data.isEmpty {
                do {
                    try self.handle(data)
                } catch {
                    self.fail(error) // protocol violation or a message that does not verify: drop the connection
                    return
                }
            }
            if let error { self.finish(error); return }
            if isComplete { self.finish(nil); self.nw.cancel(); return }
            self.receiveLoop()
        }
    }

    private func handle(_ data: Data) throws {
        if var transport {
            let plaintext = try transport.open(data)
            self.transport = transport
            for frame in try decoder.feed(plaintext) { onFrame?(frame) }
            return
        }
        guard let handshake else { return }
        // A computer from before the encrypted link answers with a hello frame ("EH" magic) where a handshake
        // message length was expected.
        if !receivedAnything, handshake.role == .initiator, Noise.looksLikeOlderEdgeHop(data) {
            throw LinkError.protocolError("the other computer runs an older, unencrypted EdgeHop: update it there")
        }
        receivedAnything = true
        handshakeReader.push(data)
        while let message = handshakeReader.nextMessage(), transport == nil {
            let payload = try handshake.read(message)
            switch (handshake.role, handshake.isFinished) {
            case (.responder, false):
                // message 1 read: answer with message 2
                nw.send(content: Noise.withLength(try handshake.write()), completion: .contentProcessed { [weak self] error in
                    if let error { self?.finish(error) }
                })
            case (.initiator, false):
                // message 2 read: answer with message 3, carrying the controller's hello
                let third = try handshake.write(handshakePayload?() ?? Data())
                nw.send(content: Noise.withLength(third), completion: .contentProcessed { [weak self] error in
                    if let error { self?.finish(error) }
                })
                try secure(handshake)
                onSecured?(Data())
            case (.responder, true):
                try secure(handshake)
                onSecured?(payload)
            case (.initiator, true):
                break
            }
        }
        if transport != nil {
            // Bytes that arrived behind the last handshake message already belong to the encrypted stream.
            let rest = handshakeReader.drain()
            if !rest.isEmpty { try handle(rest) }
        }
    }

    private func secure(_ handshake: NoiseHandshake) throws {
        sendLock.lock()
        transport = try handshake.transport()
        sendLock.unlock()
        self.handshake = nil
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
    /// Asked for every hello: true announces that this computer cannot apply input right now.
    public var isInputBlocked: () -> Bool = { false }

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
        let conn = LinkConnection(nw, queue: queue, role: .responder, token: token)
        active = conn
        var authenticated = false

        // Only a computer that knows the token gets this far: its hello is the payload of the last handshake message.
        conn.onSecured = { [weak self, weak conn] payload in
            guard let self, let conn else { return }
            guard let hello = try? JSONDecoder().decode(HelloMessage.self, from: payload) else { conn.cancel(); return }
            authenticated = true
            let blocked = self.isInputBlocked()
            conn.send(encode(HelloMessage(name: self.name, accepted: true, monitors: LocalDisplays.list(), keymap: Keymap.name, inputBlocked: blocked ? true : nil)))
            self.onPeerConnected?(hello)
        }
        conn.onFrame = { [weak self, weak conn] frame in
            guard let self, let conn else { return }
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
        let conn = LinkConnection(nw, queue: queue, role: .initiator, token: token)
        connection = conn

        let done = DispatchSemaphore(value: 0)
        var result: Result<HelloMessage, LinkError>?
        var phase = 0 // 0 handshaking, 1 waiting for the receiver's answer, 2 connected
        var tcpConnected = false
        var lastWaiting: String?
        conn.onReady = { tcpConnected = true }
        conn.onWaiting = { lastWaiting = "\($0)" }
        func finish(_ r: Result<HelloMessage, LinkError>) {
            if result == nil { result = r; done.signal() }
        }

        // The controller's hello travels inside the last handshake message.
        conn.handshakePayload = { [name] in
            (try? JSONEncoder().encode(HelloMessage(name: name, monitors: LocalDisplays.list(), keymap: Keymap.name))) ?? Data()
        }
        conn.onSecured = { _ in phase = 1 }
        conn.onFrame = { [weak self] frame in
            guard let self else { return }
            if phase == 1 {
                // The first encrypted frame: only a receiver that knows the token can have sent it.
                guard let hello = decodeHello(frame), hello.accepted == true else { return finish(.failure(.protocolError("expected accept"))) }
                phase = 2
                return finish(.success(hello))
            }
            if frame.type == .pong, frame.payload.count == 8 {
                let sent = frame.payload.reduce(UInt64(0)) { $0 << 8 | UInt64($1) }
                self.onPong?(Double(nowMillis() &- sent))
            }
        }
        conn.onClosed = { [weak self] error in
            switch phase {
            case 0:
                if let linkError = error as? LinkError { finish(.failure(linkError)) }
                else if error is NoiseError { finish(.failure(.protocolError("the encrypted handshake failed"))) }
                else { finish(.failure(.connectionFailed(error?.localizedDescription ?? "closed"))) }
            case 1:
                // Closed right after our last handshake message: the receiver could not verify it, which means the
                // token differs.
                finish(.failure(.rejected))
            default:
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
