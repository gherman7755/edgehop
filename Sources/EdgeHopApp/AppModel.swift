import AppKit
import ApplicationServices
import CoreGraphics
import EdgeHopCore
import Foundation

enum AppMode: String, CaseIterable, Identifiable {
    case controller
    case receiver
    var id: String { rawValue }
    var title: String { self == .controller ? "Control another computer" : "Be controlled" }
}

/// Everything the window and the menu bar show. Sessions call back on arbitrary queues; every mutation
/// here is hopped onto the main queue.
final class AppModel: ObservableObject {
    static let shared = AppModel()

    // MARK: Persisted settings
    @Published var mode: AppMode { didSet { defaults.set(mode.rawValue, forKey: "mode") } }
    @Published var manualHost: String { didSet { defaults.set(manualHost, forKey: "host") } }
    @Published var side: Side { didSet { defaults.set(side.rawValue, forKey: "side") } }
    @Published var switching: Bool { didSet { defaults.set(switching, forKey: "switching") } }
    @Published var forwardKeys: Bool { didSet { defaults.set(forwardKeys, forKey: "forwardKeys") } }
    @Published var allowKeys: Bool { didSet { defaults.set(allowKeys, forKey: "allowKeys") } }
    @Published var debugLog: Bool { didSet { defaults.set(debugLog, forKey: "debug") } }
    /// Token typed in on the controlling side (kept in the Keychain).
    @Published var peerToken: String { didSet { TokenStore.save(peerToken, account: "peer") } }
    /// This computer's own token, shown on the receiving side (Keychain).
    @Published var ownToken: String

    // MARK: Live state
    @Published var controllerState: ControllerState = .idle
    @Published var receiverState: ReceiverState = .stopped
    @Published var latency: Double?
    @Published var peers: [DiscoveredPeer] = []
    @Published var selectedPeer: String? // name of a discovered peer; nil = use the manual host
    @Published var logLines: [String] = []
    /// Kept here, not in a view `@State`: the Command Line Tools have no SwiftUI macro plugin.
    @Published var logExpanded = false
    @Published var accessibilityOK = false
    @Published var inputMonitoringOK = false

    private let defaults = UserDefaults.standard
    private let controller = ControllerSession()
    private var receiver: ReceiverSession?
    private let browser = PeerBrowser(ownName: Device.name)
    private var permissionTimer: Timer?

    private init() {
        mode = AppMode(rawValue: defaults.string(forKey: "mode") ?? "") ?? .controller
        manualHost = defaults.string(forKey: "host") ?? ""
        side = Side(rawValue: defaults.string(forKey: "side") ?? "") ?? .right
        switching = defaults.object(forKey: "switching") as? Bool ?? true
        forwardKeys = defaults.bool(forKey: "forwardKeys")
        allowKeys = defaults.bool(forKey: "allowKeys")
        debugLog = defaults.bool(forKey: "debug")
        peerToken = TokenStore.load(account: "peer") ?? ""
        ownToken = TokenStore.ownToken()

        controller.onLog = { [weak self] in self?.append($0) }
        controller.onLatency = { [weak self] ms in DispatchQueue.main.async { self?.latency = ms } }
        controller.onState = { [weak self] state in
            DispatchQueue.main.async {
                self?.controllerState = state
                if case .idle = state { self?.latency = nil }
            }
        }
        browser.onChange = { [weak self] peers in
            DispatchQueue.main.async {
                guard let self else { return }
                self.peers = peers
                if let selected = self.selectedPeer, !peers.contains(where: { $0.name == selected }) { self.selectedPeer = nil }
                if self.selectedPeer == nil, self.manualHost.isEmpty { self.selectedPeer = peers.first?.name }
            }
        }
        browser.start()
        refreshPermissions()
        permissionTimer = Timer.scheduledTimer(withTimeInterval: 1.5, repeats: true) { [weak self] _ in self?.refreshPermissions() }
    }

    // MARK: Derived
    var isRunning: Bool {
        switch mode {
        case .controller:
            switch controllerState {
            case .connecting, .connected: return true
            case .idle, .failed: return false
            }
        case .receiver:
            return receiverState != .stopped
        }
    }

    var statusText: String {
        switch mode {
        case .controller:
            switch controllerState {
            case .idle: return "Not connected"
            case .connecting: return "Connecting…"
            case let .connected(peer, active): return active ? "Controlling \(peer)" : "Connected to \(peer) — move the cursor through the \(side.rawValue) edge"
            case let .failed(text): return text
            }
        case .receiver:
            switch receiverState {
            case .stopped: return "Not listening"
            case let .listening(port): return "Waiting for a computer to connect (port \(port))"
            case let .connected(peer): return "Being controlled by \(peer)"
            }
        }
    }

    var statusIsError: Bool {
        if case .failed = controllerState, mode == .controller { return true }
        return false
    }

    var menuSymbol: String {
        if case .connected(_, true) = controllerState, mode == .controller { return "arrow.left.arrow.right.circle.fill" }
        return isRunning ? "dot.radiowaves.left.and.right" : "cursorarrow.rays"
    }

    var canConnect: Bool {
        guard peerToken.count >= 12 else { return false }
        return selectedPeer != nil || !manualHost.trimmingCharacters(in: .whitespaces).isEmpty
    }

    // MARK: Actions
    func startController() {
        let target: ControllerConfig.Target
        if let name = selectedPeer, let peer = peers.first(where: { $0.name == name }) {
            target = .peer(peer)
        } else {
            let host = manualHost.trimmingCharacters(in: .whitespaces)
            guard !host.isEmpty, !host.contains("@") else {
                append("Enter the other computer's address without \"user@\" (for example 192.168.1.20).")
                return
            }
            target = .host(host, port: 47474)
        }
        controller.start(ControllerConfig(
            target: target, token: peerToken.trimmingCharacters(in: .whitespaces), side: side,
            switching: switching, forwardKeys: forwardKeys, debug: debugLog
        ))
    }

    func stopController() { controller.stop() }

    func startReceiver() {
        let session = ReceiverSession(token: ownToken, allowKeys: allowKeys, debug: debugLog)
        session.onLog = { [weak self] in self?.append($0) }
        session.onState = { [weak self] state in DispatchQueue.main.async { self?.receiverState = state } }
        session.onPanic = { [weak self] in self?.append("panic chord: stopped") }
        do {
            try session.start()
            receiver = session
        } catch {
            append("Cannot start listening: \(error)")
        }
    }

    func stopReceiver() {
        receiver?.stop()
        receiver = nil
    }

    func regenerateOwnToken() {
        guard receiverState == .stopped else { return }
        ownToken = TokenStore.generate()
        TokenStore.save(ownToken, account: "own")
    }

    /// Called when the app quits: always give the cursor back.
    func shutdown() {
        controller.stop()
        receiver?.stop()
    }

    func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    func clearLog() { logLines.removeAll() }

    // MARK: Permissions
    func refreshPermissions() {
        let accessibility = AXIsProcessTrusted()
        let input = CGPreflightListenEventAccess()
        if accessibility != accessibilityOK { accessibilityOK = accessibility }
        if input != inputMonitoringOK { inputMonitoringOK = input }
    }

    func requestAccessibility() {
        let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
        _ = AXIsProcessTrustedWithOptions(options)
        openSettings("Privacy_Accessibility")
    }

    func requestInputMonitoring() {
        _ = CGRequestListenEventAccess()
        openSettings("Privacy_ListenEvent")
    }

    private func openSettings(_ pane: String) {
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?\(pane)") {
            NSWorkspace.shared.open(url)
        }
    }

    // MARK: Log
    private static let timeFormat: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "HH:mm:ss"
        return f
    }()

    func append(_ text: String) {
        let line = "\(Self.timeFormat.string(from: Date()))  \(text)"
        DispatchQueue.main.async {
            self.logLines.append(line)
            if self.logLines.count > 400 { self.logLines.removeFirst(self.logLines.count - 400) }
        }
    }
}
