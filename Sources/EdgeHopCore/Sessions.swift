import CoreGraphics
import Foundation
import Network

public enum Device {
    public static var name: String { Host.current().localizedName ?? ProcessInfo.processInfo.hostName }
}

enum Cue {
    /// Audible hand-off cue (the original draws overlay windows; a sound is the cheap version).
    static func play(_ name: String) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/afplay")
        process.arguments = ["/System/Library/Sounds/\(name).aiff"]
        try? process.run()
    }
}

/// Small thread-safe holder for state that callbacks from several queues touch.
private final class Locked<T> {
    private let lock = NSLock()
    private var value: T
    init(_ value: T) { self.value = value }
    func with<R>(_ body: (inout T) -> R) -> R { lock.lock(); defer { lock.unlock() }; return body(&value) }
}

// MARK: - Receiver (the computer being controlled)

public enum ReceiverState: Equatable, Sendable {
    case stopped
    case listening(port: UInt16)
    case connected(peer: String)
}

public final class ReceiverSession {
    public var onState: ((ReceiverState) -> Void)?
    public var onLog: ((String) -> Void)?
    /// The panic chord was typed on this computer: the session has already stopped.
    public var onPanic: (() -> Void)?

    private let token: String
    private let name: String
    private let allowKeys: Bool
    private let debug: Bool
    private let current = Locked(ReceiverState.stopped)
    private var server: LinkServer?
    private var panicTap: EventTap?
    private let injector = EventInjector() // only touched on the server's queue (and after it stopped)
    private var received = 0
    /// The connected controller announced HID key codes (an older one sends macOS keycodes we cannot read).
    private var peerSpeaksHIDKeys = false

    public init(token: String, name: String = Device.name, allowKeys: Bool = false, debug: Bool = false) {
        self.token = token
        self.name = name
        self.allowKeys = allowKeys
        self.debug = debug
    }

    public var state: ReceiverState { current.with { $0 } }

    private func set(_ state: ReceiverState) {
        let changed = current.with { value -> Bool in
            guard value != state else { return false }
            value = state
            return true
        }
        if changed { onState?(state) }
    }

    private func log(_ text: String) { onLog?(text) }

    public func start(port: UInt16 = 47474) throws {
        guard server == nil else { return }
        let server = LinkServer(token: token, name: name)
        server.onInput = { [weak self] event in self?.apply(event) }
        server.onControl = { [weak self] message in
            if message == .releaseAll { self?.injector.releaseAll(); self?.log("peer asked to release all keys/buttons") }
        }
        server.onPeerConnected = { [weak self] hello in
            guard let self else { return }
            self.received = 0
            self.peerSpeaksHIDKeys = hello.speaksHIDKeys
            self.log("peer connected: \(hello.name), keymap \(hello.keymap ?? "none")")
            if self.allowKeys, !hello.speaksHIDKeys {
                self.log("this peer does not announce the shared key codes (update EdgeHop there): its keys are dropped")
            }
            self.set(.connected(peer: hello.name))
        }
        server.onPeerDisconnected = { [weak self] in
            guard let self else { return }
            self.injector.releaseAll()
            self.log("peer disconnected (all keys/buttons released)")
            self.set(.listening(port: server.port))
        }
        try server.start(port: port, advertiseName: name)
        self.server = server

        // Local panic chord: typed on THIS computer's real keyboard (our injected events carry the marker).
        let tap = EventTap()
        tap.onEvent = { [weak self] event in
            guard PanicChord.isPanic(event) else { return }
            DispatchQueue.global().async { self?.stop(); self?.onPanic?() }
        }
        do { try tap.start(); panicTap = tap } catch {
            log("WARNING: no local panic chord (event tap unavailable: grant Accessibility and Input Monitoring)")
        }

        log("listening on port \(server.port) as \"\(name)\" (keys: \(allowKeys ? "ALLOWED, unencrypted!" : "dropped"))")
        set(.listening(port: server.port))
    }

    public func stop() {
        guard let server else { return }
        self.server = nil
        server.stop()
        panicTap?.stop()
        panicTap = nil
        injector.releaseAll()
        log("stopped")
        set(.stopped)
    }

    private func apply(_ event: InputEvent) {
        received += 1
        if case .key = event, !(allowKeys && peerSpeaksHIDKeys) { return }
        injector.inject(event)
        if debug, received <= 8 || received % 100 == 0 {
            let at = CGEvent(source: nil)?.location ?? .zero
            log("inject #\(received) \(event) -> cursor now (\(Int(at.x)), \(Int(at.y)))")
        }
    }
}

// MARK: - Controller (the computer whose keyboard and mouse are shared)

public struct ControllerConfig {
    public enum Target {
        case host(String, port: UInt16)
        case peer(DiscoveredPeer)
    }

    public var target: Target
    public var token: String
    public var side: Side
    /// Edge switching (the cursor leaves through `side`). Off = mirror mode (both cursors move).
    public var switching: Bool
    public var forwardKeys: Bool
    public var debug: Bool

    public init(target: Target, token: String, side: Side = .right, switching: Bool = true, forwardKeys: Bool = false, debug: Bool = false) {
        self.target = target
        self.token = token
        self.side = side
        self.switching = switching
        self.forwardKeys = forwardKeys
        self.debug = debug
    }
}

public enum ControllerState: Equatable, Sendable {
    case idle
    case connecting
    case connected(peer: String, remoteActive: Bool)
    case failed(String)
}

public final class ControllerSession {
    public var onState: ((ControllerState) -> Void)?
    public var onLog: ((String) -> Void)?
    public var onLatency: ((Double) -> Void)?

    private struct Parts {
        var client: LinkClient
        var tap: EventTap?
        var timer: DispatchSourceTimer?
        var controller: SwitchController?
    }

    private let current = Locked(ControllerState.idle)
    private let parts = Locked<Parts?>(nil)
    private let starting = Locked(false)

    public init() {}

    public var state: ControllerState { current.with { $0 } }

    private func set(_ state: ControllerState) {
        let changed = current.with { value -> Bool in
            guard value != state else { return false }
            value = state
            return true
        }
        if changed { onState?(state) }
    }

    private func log(_ text: String) { onLog?(text) }

    public func start(_ config: ControllerConfig) {
        let alreadyRunning = starting.with { flag -> Bool in
            if flag { return true }
            flag = true
            return false
        }
        guard !alreadyRunning else { return }
        set(.connecting)
        DispatchQueue.global().async { [self] in
            do {
                try run(config)
            } catch {
                log("failed: \(message(for: error))")
                teardown()
                set(.failed(message(for: error)))
            }
        }
    }

    public func stop() {
        teardown()
        set(.idle)
    }

    // MARK: Internals

    private func message(for error: Error) -> String {
        switch error {
        case LinkError.rejected: return "The other computer rejected the token. Check that both use the same one."
        case let LinkError.connectionFailed(text): return text
        case LinkError.timeout: return "Timed out."
        case LinkError.protocolError(let text): return "Protocol error: \(text)"
        case EventTapError.createFailed:
            return "Cannot capture input. Grant Accessibility and Input Monitoring to this app in System Settings → Privacy & Security."
        default: return "\(error)"
        }
    }

    private func run(_ config: ControllerConfig) throws {
        let client = LinkClient(token: config.token, name: Device.name)
        let hello: HelloMessage
        switch config.target {
        case let .host(host, port):
            log("connecting to \(host):\(port)…")
            hello = try client.connect(host: host, port: port)
        case let .peer(peer):
            log("connecting to \(peer.name)…")
            hello = try client.connect(to: peer.endpoint, label: peer.name)
        }
        log("connected to \"\(hello.name)\" \((hello.monitors ?? []).map { "\(Int($0.width))x\(Int($0.height))" }.joined(separator: " "))")
        client.onClosed = { [weak self] _ in
            self?.log("link closed by the other computer")
            self?.teardown()
            self?.set(.idle)
        }
        client.onPong = { [weak self] rtt in self?.onLatency?(rtt) }

        // Keys only go to a computer that reads them the same way.
        let forwardKeys = config.forwardKeys && hello.speaksHIDKeys
        if config.forwardKeys, !forwardKeys {
            log("the other computer does not announce the shared key codes (update EdgeHop there): the keyboard is not shared")
        }

        var controller: SwitchController?
        let tap = EventTap()
        var motionLogs = 0
        var sendLogs = 0

        if config.switching {
            let monitors = hello.monitors ?? []
            guard let remote = monitors.first(where: { $0.x == 0 && $0.y == 0 }) ?? monitors.first else {
                client.onClosed = nil
                client.close()
                throw LinkError.protocolError("the other computer reported no displays")
            }
            let displays = LocalDisplays.list().map { Rect(x: $0.x, y: $0.y, width: $0.width, height: $0.height) }
            log("local displays: \(displays.map { "\(Int($0.width))x\(Int($0.height))@\(Int($0.x)),\(Int($0.y))" }.joined(separator: " ")); remote on the \(config.side.rawValue)")
            let peerName = hello.name
            let debug = config.debug
            let sessionController = SwitchController(
                displays: displays, side: config.side,
                remoteWidth: remote.width, remoteHeight: remote.height,
                forwardKeys: forwardKeys,
                actions: SwitchController.Actions(
                    send: { [weak self] event in
                        client.send(event)
                        if debug { sendLogs += 1; if sendLogs <= 8 || sendLogs % 60 == 0 { self?.log("send #\(sendLogs) \(event)") } }
                    },
                    enterRemote: { [weak self] in
                        let rc = CGAssociateMouseAndMouseCursorPosition(0) // freeze the local cursor; raw deltas keep coming
                        motionLogs = 0
                        sendLogs = 0
                        self?.log("-> REMOTE (\(peerName)); CGAssociateMouseAndMouseCursorPosition(0) returned \(rc.rawValue)")
                        self?.set(.connected(peer: peerName, remoteActive: true))
                        Cue.play("Tink")
                    },
                    leaveRemote: { [weak self] point in
                        CGWarpMouseCursorPosition(CGPoint(x: point.x, y: point.y))
                        let rc = CGAssociateMouseAndMouseCursorPosition(1)
                        self?.log("<- LOCAL at (\(Int(point.x)), \(Int(point.y))); re-associate returned \(rc.rawValue)")
                        self?.set(.connected(peer: peerName, remoteActive: false))
                        Cue.play("Pop")
                    },
                    releaseRemote: { client.sendControl(.releaseAll) }
                )
            )
            controller = sessionController
            tap.onMotion = { [weak self] position, delta in
                sessionController.handleMotion(position: position, delta: delta)
                if debug, sessionController.mode == .remote {
                    motionLogs += 1
                    if motionLogs <= 8 || motionLogs % 60 == 0 {
                        let local = CGEvent(source: nil)?.location ?? .zero
                        self?.log("motion pos=(\(Int(position.x)),\(Int(position.y))) delta=(\(Int(delta.x)),\(Int(delta.y))) local cursor=(\(Int(local.x)),\(Int(local.y)))")
                    }
                }
            }
            tap.shouldSwallow = { sessionController.shouldSwallow($0) }
        } else {
            tap.reportsDeltas = { true } // mirror mode: relative motion, screens may differ in size
        }

        tap.onEvent = { [weak self] event in
            if PanicChord.isPanic(event) {
                self?.log("panic chord: stopping")
                DispatchQueue.global().async { self?.stop() }
                return
            }
            if let controller {
                controller.handle(event)
            } else {
                if case .key = event, !forwardKeys { return }
                client.send(event)
            }
        }

        do { try tap.start() } catch {
            client.onClosed = nil // the failure below is the story, not the close that follows
            client.close()
            throw error
        }

        let timer = DispatchSource.makeTimerSource(queue: .global())
        timer.schedule(deadline: .now() + 2, repeating: 2)
        timer.setEventHandler { client.sendPing() }
        timer.resume()

        parts.with { $0 = Parts(client: client, tap: tap, timer: timer, controller: controller) }
        set(.connected(peer: hello.name, remoteActive: false))
        log(config.switching
            ? "switching on: push the cursor through the \(config.side.rawValue) edge. Panic chord: Ctrl+Option+Cmd+Esc"
            : "mirror mode: both cursors move. Panic chord: Ctrl+Option+Cmd+Esc")
    }

    /// Always gives the cursor back, whatever ends the session. Idempotent.
    private func teardown() {
        let taken = parts.with { value -> Parts? in
            defer { value = nil }
            return value
        }
        starting.with { $0 = false }
        guard let taken else { return }
        taken.controller?.returnToLocal()
        CGAssociateMouseAndMouseCursorPosition(1)
        taken.timer?.cancel()
        taken.tap?.stop()
        taken.client.close()
        log("session ended")
    }
}
