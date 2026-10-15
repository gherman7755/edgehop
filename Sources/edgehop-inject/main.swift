import CoreGraphics
import EdgeHopCore
import Foundation

// Milestone 3: replay events on this Mac and prove the feedback guard works.
//
//   edgehop-inject selftest                      move the cursor in a small square, check the tap ignores it
//   edgehop-inject replay FILE [--speed 2.0] [--allow-input]
//
// `replay` refuses recordings that contain clicks, keys or scrolling unless --allow-input is given,
// because those act on whatever is under the cursor / focused right now.

final class Counter {
    private let lock = NSLock()
    private var value = 0
    func increment() { lock.lock(); value += 1; lock.unlock() }
    var current: Int { lock.lock(); defer { lock.unlock() }; return value }
}

func fail(_ message: String, code: Int32 = 1) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(code)
}

func startTap(onEvent: ((InputEvent) -> Void)?) -> EventTap {
    let tap = EventTap()
    tap.onEvent = onEvent
    do {
        try tap.start()
    } catch {
        fail("Could not create the event tap (\(error)). Grant Accessibility and Input Monitoring to the app running this command.")
    }
    return tap
}

func selftest() {
    guard CGPreflightPostEventAccess() else {
        _ = CGRequestPostEventAccess()
        fail("Posting events is not allowed for this app yet. Grant Accessibility (or the post-event permission), then run again.")
    }
    print("Self-test: the cursor will move in a small square for ~2 s. Please do NOT touch the mouse.")
    fflush(stdout)

    let realEvents = Counter() // events the tap delivered to onEvent (should only be real user input)
    let tap = startTap { _ in realEvents.increment() }
    let injector = EventInjector()

    let original = CGEvent(source: nil)?.location ?? .zero
    let bounds = CGDisplayBounds(CGMainDisplayID())
    let center = CGPoint(x: bounds.midX, y: bounds.midY)
    let side = 120.0
    let steps = 24

    var injected = 0
    func move(_ x: Double, _ y: Double) {
        injector.inject(.mouseMoveAbsolute(x: Int32(x.rounded()), y: Int32(y.rounded())))
        injected += 1
        Thread.sleep(forTimeInterval: 0.008)
    }

    // Clockwise square. The first point is far from the cursor (warp path), the rest are small steps (relative path).
    let corners = [
        CGPoint(x: center.x - side / 2, y: center.y - side / 2),
        CGPoint(x: center.x + side / 2, y: center.y - side / 2),
        CGPoint(x: center.x + side / 2, y: center.y + side / 2),
        CGPoint(x: center.x - side / 2, y: center.y + side / 2),
        CGPoint(x: center.x - side / 2, y: center.y - side / 2),
    ]
    move(corners[0].x, corners[0].y)
    for i in 1..<corners.count {
        for s in 1...steps {
            let t = Double(s) / Double(steps)
            move(corners[i - 1].x + (corners[i].x - corners[i - 1].x) * t,
                 corners[i - 1].y + (corners[i].y - corners[i - 1].y) * t)
        }
    }
    move(original.x, original.y) // put the cursor back
    Thread.sleep(forTimeInterval: 0.6)

    // Phase 2 (regression for the M4 bug): a FRESH injector that only ever receives deltas must move the
    // real cursor. Before the fix, relative moves were dropped until an absolute move had been seen.
    let fresh = EventInjector()
    let before = CGEvent(source: nil)?.location ?? .zero
    fresh.inject(.mouseMoveDelta(dx: 40, dy: 0))
    injected += 1
    Thread.sleep(forTimeInterval: 0.15)
    let after = CGEvent(source: nil)?.location ?? .zero
    fresh.inject(.mouseMoveDelta(dx: -40, dy: 0)) // back where it was
    injected += 1
    Thread.sleep(forTimeInterval: 0.5) // let the tap drain
    tap.stop()
    let moved = after.x - before.x
    let relativeOK = abs(moved - 40) <= 2
    print("relative-only move    : cursor moved \(moved) px (expected 40) -> \(relativeOK ? "ok" : "BROKEN")")

    let stats = tap.statistics
    print("injected moves        : \(injected)")
    print("ignored by tap (marker): \(stats.ownIgnored)")
    print("delivered to onEvent   : \(realEvents.current)")
    if !relativeOK {
        fail("FAIL: a relative move on a fresh injector did not move the cursor (the M4 bug).")
    } else if stats.ownIgnored == injected && realEvents.current == 0 {
        print("PASS: every injected event was recognised as ours and none reached the capture callback.")
    } else if stats.ownIgnored == 0 {
        fail("FAIL: the tap never saw our injected events (permission problem?) or the marker is not being set.")
    } else if realEvents.current > 0 {
        fail("INCONCLUSIVE/FAIL: \(realEvents.current) event(s) reached onEvent. Either you moved the mouse during the test or the marker guard leaks.")
    } else {
        fail("FAIL: \(stats.ownIgnored) of \(injected) injected events were recognised.")
    }
}

func replay(_ args: [String]) {
    guard args.count >= 2 else { fail("usage: edgehop-inject replay FILE [--speed 2.0] [--allow-input]", code: 2) }
    let path = args[1]
    var speed = 1.0
    if let i = args.firstIndex(of: "--speed"), i + 1 < args.count, let v = Double(args[i + 1]), v > 0 { speed = v }
    let allowInput = args.contains("--allow-input")

    let text: String
    do { text = try String(contentsOfFile: path, encoding: .utf8) } catch { fail("Cannot read \(path): \(error)") }
    let events: [RecordedEvent]
    do { events = try Recording.parse(text) } catch { fail("Bad recording: \(error)") }
    guard !events.isEmpty else { fail("Recording is empty.") }

    var buttons = 0, keys = 0, scrolls = 0
    for e in events {
        switch e.event {
        case .mouseButton: buttons += 1
        case .key: keys += 1
        case .scroll: scrolls += 1
        default: break
        }
    }
    print("Recording: \(events.count) events (\(buttons) button, \(keys) key, \(scrolls) scroll), \(events.last!.millis) ms, speed x\(speed)")
    if (buttons > 0 || keys > 0 || scrolls > 0) && !allowInput {
        fail("Refusing to replay clicks/keys/scrolling without --allow-input: they would act on whatever is under the cursor or focused now.", code: 2)
    }
    guard CGPreflightPostEventAccess() else {
        _ = CGRequestPostEventAccess()
        fail("Posting events is not allowed for this app yet. Grant Accessibility, then run again.")
    }

    let injector = EventInjector()
    let start = DispatchTime.now()
    for recorded in events {
        let due = Double(recorded.millis) / speed / 1000
        let elapsed = Double(DispatchTime.now().uptimeNanoseconds - start.uptimeNanoseconds) / 1e9
        if due > elapsed { Thread.sleep(forTimeInterval: due - elapsed) }
        injector.inject(recorded.event)
    }
    injector.releaseAll()
    print("Replay finished.")
}

let args = Array(CommandLine.arguments.dropFirst())
switch args.first {
case "selftest": selftest()
case "replay": replay(args)
default: fail("usage:\n  edgehop-inject selftest\n  edgehop-inject replay FILE [--speed 2.0] [--allow-input]", code: 2)
}
