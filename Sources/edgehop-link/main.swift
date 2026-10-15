import CoreGraphics
import EdgeHopCore
import Foundation

// Command-line front end (the desktop app uses the same sessions).
//
//   edgehop-link token                                                    print a fresh random token
//   edgehop-link serve   [--port 47474] [--seconds 300] [--allow-keys] [--debug]
//   edgehop-link connect HOST [--port 47474] [--seconds 120] [--keys] [--debug]
//                        [--switch [--side right|left|top|bottom]]
//
// Token: pass it through the EDGEHOP_TOKEN environment variable (preferred; --token shows up in `ps`).
// Without --switch, `connect` mirrors the mouse (both cursors move). Keys are OFF unless you opt in on BOTH
// sides (--keys / --allow-keys): the link is not encrypted yet. Panic chord: Ctrl + Option + Cmd + Esc.

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

switch CommandLine.arguments.dropFirst().first {
case "token": print(TokenStore.generate())
case "serve": serve()
case "connect": connect()
default:
    fail("usage:\n  edgehop-link token\n  edgehop-link serve [--port N] [--seconds N] [--allow-keys] [--debug]\n  edgehop-link connect HOST [--port N] [--seconds N] [--keys] [--debug] [--switch [--side S]]", code: 2)
}
