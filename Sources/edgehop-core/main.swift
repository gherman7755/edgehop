import EdgeHopIPC
import Foundation

// The macOS core as a helper process (docs/IPC.md): newline-delimited JSON on stdin/stdout.
// stdout belongs to the protocol; anything for humans goes to stderr.
//
//   edgehop-core            run until stdin closes or `core.shutdown` arrives
//
// If the parent dies, stdin closes, so every session is stopped and the cursor is given back.

signal(SIGPIPE, SIG_IGN) // a vanished parent shows up as a failed write, not a fatal signal

var service: CoreService?
let server = IPCServer(transport: StdioTransport()) { emit in
    let created = CoreService(emit: emit)
    service = created
    return created
}

// SIGTERM / SIGINT (the UI killing us): same clean-up as end of input.
var signalSources: [DispatchSourceSignal] = []
for sig in [SIGTERM, SIGINT] {
    signal(sig, SIG_IGN)
    let source = DispatchSource.makeSignalSource(signal: sig, queue: .global())
    source.setEventHandler {
        service?.shutdown()
        exit(0)
    }
    source.resume()
    signalSources.append(source)
}

FileHandle.standardError.write(Data("edgehop-core \(CoreService.version) ready\n".utf8))
server.run()
