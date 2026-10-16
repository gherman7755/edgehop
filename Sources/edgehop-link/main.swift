import CoreGraphics
import EdgeHopCore
import Foundation

// Command-line front end (the desktop app uses the same sessions).
//
//   edgehop-link token                                                    print a fresh random token
//   edgehop-link serve   [--port 47474] [--seconds 300] [--allow-keys] [--debug]
//   edgehop-link connect HOST [--port 47474] [--seconds 120] [--keys] [--debug]
//                        [--switch [--side right|left|top|bottom]]
//   edgehop-link browse  [--seconds 10] [--connect NAME]      list computers found nearby (Bonjour)
//
// Token: pass it through the EDGEHOP_TOKEN environment variable (preferred; --token shows up in `ps`).
// Without --switch, `connect` mirrors the mouse (both cursors move). Keys are OFF unless you opt in on BOTH
// sides (--keys / --allow-keys). Panic chord: Ctrl + Option + Cmd + Esc.

setvbuf(stdout, nil, _IOLBF, 0) // line-buffered even when redirected: logs show up immediately

func argument(_ name: String) -> String? {
    guard let i = CommandLine.arguments.firstIndex(of: name), i + 1 < CommandLine.arguments.count else { return nil }
    return CommandLine.arguments[i + 1]
}

func flag(_ name: String) -> Bool { CommandLine.arguments.contains(name) }

func fail(_ message: String, code: Int32 = 1) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(code)
}

func requireToken() -> String {
    guard let token = argument("--token") ?? ProcessInfo.processInfo.environment["EDGEHOP_TOKEN"], token.count >= 12 else {
        fail("Missing token (at least 12 characters). Generate one with `edgehop-link token` and export EDGEHOP_TOKEN.", code: 2)
    }
    return token
}

let port = argument("--port").flatMap(UInt16.init) ?? 47474

/// Runs until the semaphore is signalled by the session, Ctrl+C / SIGTERM, or the timeout.
func waitUntilDone(_ done: DispatchSemaphore, seconds: Double, cleanup: @escaping () -> Void) {
    var sources: [DispatchSourceSignal] = []
    for sig in [SIGINT, SIGTERM] {
        signal(sig, SIG_IGN)
        let source = DispatchSource.makeSignalSource(signal: sig, queue: .global())
        source.setEventHandler { cleanup(); done.signal() }
        source.resume()
        sources.append(source)
    }
    _ = done.wait(timeout: .now() + seconds)
    cleanup()
    _ = sources
}

func serve() {
    let token = requireToken()
    let seconds = argument("--seconds").flatMap(Double.init) ?? 300
    let done = DispatchSemaphore(value: 0)
    let session = ReceiverSession(token: token, allowKeys: flag("--allow-keys"), debug: flag("--debug"))
    session.onLog = { print($0) }
    session.onPanic = { done.signal() }
    do { try session.start(port: port) } catch { fail("Cannot listen on port \(port): \(error)") }
    print("Panic chord: Ctrl+Option+Cmd+Esc. Auto-exit in \(Int(seconds)) s.")
    waitUntilDone(done, seconds: seconds) { session.stop() }
    print("Stopped.")
}

func connect() {
    let token = requireToken()
    guard CommandLine.arguments.count >= 3, !CommandLine.arguments[2].hasPrefix("--") else {
        fail("usage: edgehop-link connect HOST [--port N] [--seconds N] [--keys] [--debug] [--switch [--side right|left|top|bottom]]", code: 2)
    }
    let host = CommandLine.arguments[2]
    if host.contains("@") {
        fail("HOST must be just the address (e.g. 192.168.18.57 or name.local), without \"user@\": that prefix is only for ssh/scp.", code: 2)
    }
    guard let side = Side(rawValue: argument("--side") ?? "right") else {
        fail("--side must be one of: right, left, top, bottom", code: 2)
    }
    let seconds = argument("--seconds").flatMap(Double.init) ?? 120

    let done = DispatchSemaphore(value: 0)
    let session = ControllerSession()
    session.onLog = { print($0) }
    session.onLatency = { print(String(format: "ping %.0f ms", $0)) }
    session.onState = { state in
        switch state {
        case .idle, .failed: done.signal()
        default: break
        }
    }
    session.start(ControllerConfig(
        target: .host(host, port: port), token: token, side: side,
        switching: flag("--switch"), forwardKeys: flag("--keys"), debug: flag("--debug")
    ))
    print("Auto-exit in \(Int(seconds)) s.")
    waitUntilDone(done, seconds: seconds) { session.stop() }
    print("Stopped.")
}

final class Once {
    private let lock = NSLock()
    private var done = false
    /// True the first time only.
    func first() -> Bool { lock.lock(); defer { lock.unlock() }; let was = done; done = true; return !was }
}

/// Lists the computers announcing `_edgehop._tcp` (a check of discovery, also against the Rust core). With
/// `--connect NAME` it connects to that one (token from EDGEHOP_TOKEN), which proves its address resolves.
func browse() {
    let seconds = argument("--seconds").flatMap(Double.init) ?? 10
    let wanted = argument("--connect")
    let token = wanted == nil ? "" : requireToken()
    let done = DispatchSemaphore(value: 0)
    let session = ControllerSession()
    let started = Once()
    let peers = PeerBrowser(ownName: Device.name)
    peers.onChange = { found in
        print("found: " + (found.isEmpty ? "(nothing)" : found.map(\.name).joined(separator: ", ")))
        guard let wanted, let peer = found.first(where: { $0.name == wanted }) else { return }
        guard started.first() else { return }
        session.onLog = { print($0) }
        session.onState = { state in
            switch state {
            case let .connected(peer, _): print("connected to \(peer)"); done.signal()
            case .failed: done.signal()
            default: break
            }
        }
        session.start(ControllerConfig(target: .peer(peer), token: token, side: .right, switching: false))
    }
    peers.start()
    waitUntilDone(done, seconds: seconds) { session.stop(); peers.stop() }
}

switch CommandLine.arguments.dropFirst().first {
case "token": print(TokenStore.generate())
case "browse": browse()
case "serve": serve()
case "connect": connect()
default:
    fail("usage:\n  edgehop-link token\n  edgehop-link browse [--seconds N] [--connect NAME]\n  edgehop-link serve [--port N] [--seconds N] [--allow-keys] [--debug]\n  edgehop-link connect HOST [--port N] [--seconds N] [--keys] [--debug] [--switch [--side S]]", code: 2)
}
