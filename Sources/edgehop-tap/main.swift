import EdgeHopCore
import Foundation

// Print (and optionally record) what the event tap sees. Log-only: it never swallows an event, so it
// cannot lock your input. It exits by itself after --seconds (default 15).
//
//   swift run edgehop-tap --seconds 20 [--out recording.txt]
//
// macOS asks for Accessibility / Input Monitoring for the app running this (Terminal, iTerm, Claude…).

func argument(_ name: String) -> String? {
    guard let i = CommandLine.arguments.firstIndex(of: name), i + 1 < CommandLine.arguments.count else { return nil }
    return CommandLine.arguments[i + 1]
}

let seconds = argument("--seconds").flatMap(Double.init) ?? 15
let outPath = argument("--out")

var outFile: FileHandle?
if let outPath {
    FileManager.default.createFile(atPath: outPath, contents: Data("# edgehop recording: <millis> <wire record hex>\n".utf8))
    outFile = FileHandle(forWritingAtPath: outPath)
    outFile?.seekToEndOfFile()
}
let writeQueue = DispatchQueue(label: "edgehop.recording")
let start = DispatchTime.now()

let tap = EventTap()
tap.onEvent = { event in
    let millis = Int((DispatchTime.now().uptimeNanoseconds - start.uptimeNanoseconds) / 1_000_000)
    let recorded = RecordedEvent(millis: millis, event: event)
    print("\(event)  \(Recording.line(recorded))")
    if let outFile {
        // Never block the tap thread on disk I/O.
        writeQueue.async { outFile.write(Data((Recording.line(recorded) + "\n").utf8)) }
    }
}

do {
    try tap.start()
} catch {
    FileHandle.standardError.write(Data("""
    Could not create the event tap (\(error)).
    Grant Accessibility and Input Monitoring to the app that runs this command, then run it again.

    """.utf8))
    exit(1)
}

print("Tapping events for \(Int(seconds)) s. Move the mouse, click, scroll, type.")
fflush(stdout)
Thread.sleep(forTimeInterval: seconds)
tap.stop()
writeQueue.sync { try? outFile?.close() }
let stats = tap.statistics
print("Done. \(stats.mapped) events seen" + (outPath.map { ", recorded to \($0)" } ?? "") + ".")
