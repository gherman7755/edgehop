import Foundation

// Cross-language wire check between the Swift core and the Rust crate (rust/crates/edgehop-proto).
//
//   golden gen              print golden vectors produced by the Swift implementation
//   golden verify FILE      check that vectors produced by Rust (examples/emit_vectors.rs) are accepted by Swift
//
// Compiled together with Sources/EdgeHopCore by scripts/golden-vectors.sh and scripts/check-rust-compat.sh.

func hex(_ data: Data) -> String { data.map { String(format: "%02x", $0) }.joined() }

func unhex(_ text: String) -> Data? { Recording.data(fromHex: text) }

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data(("FAIL: " + message + "\n").utf8))
    exit(1)
}

// MARK: Event descriptors (JSON <-> InputEvent)

func buttonName(_ button: MouseButton) -> String {
    switch button {
    case .left: return "left"
    case .right: return "right"
    case .middle: return "middle"
    case .back: return "back"
    case .forward: return "forward"
    }
}

func describe(_ event: InputEvent) -> [String: Any] {
    switch event {
    case let .mouseMoveAbsolute(x, y): return ["kind": "mouseMoveAbsolute", "x": Int(x), "y": Int(y)]
    case let .mouseMoveDelta(dx, dy): return ["kind": "mouseMoveDelta", "dx": Int(dx), "dy": Int(dy)]
    case let .mouseButton(button, isDown): return ["kind": "mouseButton", "button": buttonName(button), "isDown": isDown]
    case let .scroll(h, v): return ["kind": "scroll", "horizontal": Int(h), "vertical": Int(v)]
    case let .key(code, isDown, modifiers): return ["kind": "key", "code": Int(code), "isDown": isDown, "modifiers": Int(modifiers.rawValue)]
    }
}

func parseEvent(_ d: [String: Any]) -> InputEvent? {
    func int32(_ key: String) -> Int32? { (d[key] as? NSNumber).map { Int32(truncating: $0) } }
    switch d["kind"] as? String {
    case "mouseMoveAbsolute": if let x = int32("x"), let y = int32("y") { return .mouseMoveAbsolute(x: x, y: y) }
    case "mouseMoveDelta": if let dx = int32("dx"), let dy = int32("dy") { return .mouseMoveDelta(dx: dx, dy: dy) }
    case "mouseButton":
        let buttons: [String: MouseButton] = ["left": .left, "right": .right, "middle": .middle, "back": .back, "forward": .forward]
        if let name = d["button"] as? String, let button = buttons[name], let down = d["isDown"] as? Bool { return .mouseButton(button, isDown: down) }
    case "scroll": if let h = int32("horizontal"), let v = int32("vertical") { return .scroll(horizontal: h, vertical: v) }
    case "key":
        if let code = int32("code"), let down = d["isDown"] as? Bool, let mods = int32("modifiers") {
            return .key(code: UInt16(truncatingIfNeeded: code), isDown: down, modifiers: Modifiers(rawValue: UInt8(truncatingIfNeeded: mods)))
        }
    default: break
    }
    return nil
}

// MARK: Samples

let sampleEvents: [InputEvent] = [
    .mouseMoveAbsolute(x: 1919, y: -3),
    .mouseMoveDelta(dx: -7, dy: 12),
    .mouseButton(.left, isDown: true),
    .mouseButton(.forward, isDown: false),
    .scroll(horizontal: -120, vertical: 360),
    .key(code: 53, isDown: true, modifiers: [.command, .shift]),
    .mouseMoveAbsolute(x: Int32.min, y: Int32.max),
]

let sampleFrames: [Frame] = [
    Frame(type: .input, payload: InputEvent.mouseMoveDelta(dx: 1, dy: 2).encoded()),
    Frame(type: .ping),
    Frame(type: .control, flags: 0x80, payload: Data("{}".utf8)),
]

let authToken = "correct-horse-battery"
let authNonce = Data((0..<32).map { UInt8($0) })

func jsonString(_ value: some Encodable) -> String {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
    return String(data: try! encoder.encode(value), encoding: .utf8)!
}

let sampleMonitors = [MonitorInfo(id: "d1", x: 0, y: 0, width: 1512, height: 982), MonitorInfo(id: "d2", x: -223, y: -1080, width: 1920, height: 1080)]
let sampleHellos: [HelloMessage] = [
    HelloMessage(name: "Mac", nonce: authNonce.base64EncodedString()),
    HelloMessage(name: "PC", proof: LinkAuth.proof(token: authToken, nonce: authNonce).base64EncodedString(), monitors: sampleMonitors),
    HelloMessage(name: "Mac", accepted: true, monitors: sampleMonitors),
    HelloMessage(name: "Mac", accepted: false),
]

// MARK: gen

func generate() {
    let events = sampleEvents.map { ["event": describe($0), "hex": hex($0.encoded())] as [String: Any] }
    let frames = sampleFrames.map {
        ["type": Int($0.type.rawValue), "flags": Int($0.flags), "payloadHex": hex($0.payload), "hex": hex(FrameCodec.encode($0))] as [String: Any]
    }
    let auth: [[String: Any]] = [["token": authToken, "nonceHex": hex(authNonce), "proofHex": hex(LinkAuth.proof(token: authToken, nonce: authNonce))]]
    let hellos = sampleHellos.map { ["json": jsonString($0)] }
    let control: [[String: Any]] = [["json": #"{"t":"release_all"}"#]]
    // Same expression as LinkClient.sendPing().
    var millis = UInt64(1_234_567_890_123).bigEndian
    let ping: [[String: Any]] = [["nowMillis": 1_234_567_890_123, "payloadHex": hex(Data(bytes: &millis, count: 8))]]
    let root: [String: Any] = ["events": events, "frames": frames, "auth": auth, "hello": hellos, "control": control, "ping": ping]
    let data = try! JSONSerialization.data(withJSONObject: root, options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes])
    print(String(data: data, encoding: .utf8)!)
}

// MARK: verify (vectors produced by Rust)

func verify(path: String) {
    guard let data = FileManager.default.contents(atPath: path),
          let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    else { fail("cannot read \(path)") }
    var checked = 0
    func check(_ condition: Bool, _ label: @autoclosure () -> String) {
        if !condition { fail(label()) }
        checked += 1
    }

    for entry in root["events"] as? [[String: Any]] ?? [] {
        guard let bytes = unhex(entry["hex"] as? String ?? ""), let expected = parseEvent(entry["event"] as? [String: Any] ?? [:])
        else { fail("bad event entry \(entry)") }
        let decoded = try? InputEvent.decode(bytes)
        check(decoded == expected, "Swift decodes Rust record \(hex(bytes)) as \(String(describing: decoded)), expected \(expected)")
        check(expected.encoded() == bytes, "Swift re-encodes \(expected) differently from Rust: \(hex(expected.encoded())) vs \(hex(bytes))")
    }
    for entry in root["frames"] as? [[String: Any]] ?? [] {
        guard let bytes = unhex(entry["hex"] as? String ?? ""), let payload = unhex(entry["payloadHex"] as? String ?? "")
        else { fail("bad frame entry") }
        var decoder = FrameDecoder()
        let frames = try? decoder.feed(bytes)
        check(frames?.count == 1 && frames?[0].payload == payload && Int(frames?[0].type.rawValue ?? 0) == entry["type"] as? Int
            && Int(frames?[0].flags ?? 0) == entry["flags"] as? Int, "Swift rejects or misreads Rust frame \(hex(bytes))")
    }
    for entry in root["auth"] as? [[String: Any]] ?? [] {
        guard let token = entry["token"] as? String, let nonce = unhex(entry["nonceHex"] as? String ?? ""), let proof = unhex(entry["proofHex"] as? String ?? "")
        else { fail("bad auth entry") }
        check(LinkAuth.verify(token: token, nonce: nonce, proof: proof), "Swift rejects the Rust proof")
        check(LinkAuth.proof(token: token, nonce: nonce) == proof, "proofs differ between Swift and Rust")
    }
    for entry in root["hello"] as? [[String: Any]] ?? [] {
        guard let json = entry["json"] as? String else { fail("bad hello entry") }
        check((try? JSONDecoder().decode(HelloMessage.self, from: Data(json.utf8))) != nil, "Swift cannot decode Rust hello JSON \(json)")
    }
    for entry in root["control"] as? [[String: Any]] ?? [] {
        let object = (try? JSONSerialization.jsonObject(with: Data((entry["json"] as? String ?? "").utf8))) as? [String: Any]
        check(object?["t"] as? String == ControlMessage.releaseAll.rawValue, "Rust control JSON is not release_all")
    }
    for entry in root["ping"] as? [[String: Any]] ?? [] {
        guard let now = (entry["nowMillis"] as? NSNumber)?.uint64Value, let payload = unhex(entry["payloadHex"] as? String ?? "")
        else { fail("bad ping entry") }
        check(payload.reduce(UInt64(0)) { $0 << 8 | UInt64($1) } == now && payload.count == 8, "ping payload is not 8 bytes big-endian")
    }
    print("Swift accepted \(checked) Rust vectors")
}

// MARK: scenarios (differential test of the edge-switching controller)

/// Deterministic pseudo-random numbers so the scenarios are reproducible.
struct Lcg {
    var state: UInt64
    mutating func next() -> UInt64 {
        state = state &* 6364136223846793005 &+ 1442695040888963407
        return state >> 11
    }
    mutating func int(_ range: ClosedRange<Int>) -> Int { range.lowerBound + Int(next() % UInt64(range.count)) }
    mutating func pick<T>(_ items: [T]) -> T { items[int(0...(items.count - 1))] }
}

final class Trace {
    var entries: [[String: Any]] = []
}

func sideName(_ side: Side) -> String { side.rawValue }

func rectJSON(_ r: Rect) -> [String: Any] { ["x": r.x, "y": r.y, "width": r.width, "height": r.height] }

/// Runs `steps` through the real Swift controller and records everything it does.
func runScenario(name: String, displays: [Rect], side: Side, remote: (Double, Double), forwardKeys: Bool, steps: [[String: Any]]) -> [String: Any] {
    let trace = Trace()
    let actions = SwitchController.Actions(
        send: { trace.entries.append(["action": "send", "event": describe($0)]) },
        enterRemote: { trace.entries.append(["action": "enter"]) },
        leaveRemote: { trace.entries.append(["action": "leave", "x": $0.x, "y": $0.y]) },
        releaseRemote: { trace.entries.append(["action": "release"]) }
    )
    let controller = SwitchController(displays: displays, side: side, remoteWidth: remote.0, remoteHeight: remote.1, forwardKeys: forwardKeys, actions: actions)
    for (index, step) in steps.enumerated() {
        trace.entries.append(["step": index])
        switch step["op"] as? String {
        case "motion":
            controller.handleMotion(position: Point(x: (step["x"] as! NSNumber).doubleValue, y: (step["y"] as! NSNumber).doubleValue),
                                    delta: Point(x: (step["dx"] as! NSNumber).doubleValue, y: (step["dy"] as! NSNumber).doubleValue))
        case "handle":
            controller.handle(parseEvent(step["event"] as! [String: Any])!)
        case "swallow":
            trace.entries.append(["swallow": controller.shouldSwallow(parseEvent(step["event"] as! [String: Any])!)])
        case "return":
            controller.returnToLocal()
        default: fatalError("unknown step")
        }
        trace.entries.append(["mode": controller.mode == .remote ? "remote" : "local"])
    }
    return [
        "name": name, "displays": displays.map(rectJSON), "side": sideName(side), "remoteWidth": remote.0, "remoteHeight": remote.1,
        "forwardKeys": forwardKeys, "steps": steps, "trace": trace.entries,
    ]
}

func motion(_ x: Double, _ y: Double, _ dx: Double, _ dy: Double) -> [String: Any] { ["op": "motion", "x": x, "y": y, "dx": dx, "dy": dy] }
func handleStep(_ e: InputEvent) -> [String: Any] { ["op": "handle", "event": describe(e)] }
func swallowStep(_ e: InputEvent) -> [String: Any] { ["op": "swallow", "event": describe(e)] }

/// Random but edge-biased steps: positions near the borders of the given displays are where the logic lives.
func randomSteps(count: Int, displays: [Rect], seed: UInt64) -> [[String: Any]] {
    var rng = Lcg(state: seed)
    let minX = displays.map { $0.minX }.min()!, maxX = displays.map { $0.maxX }.max()!
    let minY = displays.map { $0.minY }.min()!, maxY = displays.map { $0.maxY }.max()!
    func position() -> (Double, Double) {
        let d = rng.pick(displays)
        let xs = [d.minX, d.minX + 1, d.minX + 2, d.maxX - 3, d.maxX - 2, d.maxX - 1, d.maxX - 0.5, Double(rng.int(Int(minX)...Int(maxX - 1)))]
        let ys = [d.minY, d.minY + 1, d.maxY - 2, d.maxY - 1, Double(rng.int(Int(minY)...Int(maxY - 1)))]
        return (rng.pick(xs), rng.pick(ys))
    }
    let buttons: [MouseButton] = [.left, .right, .middle, .back, .forward]
    var steps: [[String: Any]] = []
    for _ in 0..<count {
        switch rng.int(0...99) {
        case 0..<55:
            let (x, y) = position()
            let big = [-1500, -300, -40, -12, -5, -2, -1, 0, 1, 2, 5, 12, 40, 300, 1500]
            steps.append(motion(x, y, Double(rng.pick(big)), Double(rng.pick(big))))
        case 55..<68: steps.append(handleStep(.mouseButton(rng.pick(buttons), isDown: rng.int(0...1) == 1)))
        case 68..<74: steps.append(handleStep(.scroll(horizontal: Int32(rng.int(-240...240)), vertical: Int32(rng.int(-240...240)))))
        case 74..<80: steps.append(handleStep(.key(code: UInt16(rng.int(0...60)), isDown: rng.int(0...1) == 1, modifiers: Modifiers(rawValue: UInt8(rng.int(0...15))))))
        case 80..<92:
            let events: [InputEvent] = [.mouseMoveAbsolute(x: 3, y: 4), .mouseMoveDelta(dx: 1, dy: 1), .mouseButton(.left, isDown: true), .scroll(horizontal: 0, vertical: 120), .key(code: 5, isDown: true, modifiers: [])]
            steps.append(swallowStep(rng.pick(events)))
        default: steps.append(["op": "return"])
        }
    }
    return steps
}

func generateScenarios() {
    let laptop = Rect(x: 0, y: 0, width: 1512, height: 982)
    let above = Rect(x: -223, y: -1080, width: 1920, height: 1080)
    let left = Rect(x: 0, y: 0, width: 1000, height: 800)
    let right = Rect(x: 1000, y: 0, width: 1000, height: 800)
    var out: [[String: Any]] = []

    // Hand-written walk through the whole life cycle (the laptop + monitor-above setup).
    out.append(runScenario(name: "lifecycle laptop+monitor right", displays: [laptop, above], side: .right, remote: (1440, 900), forwardKeys: false, steps: [
        motion(800, 500, 6, 0), motion(1511, 500, 6, 0), swallowStep(.mouseMoveDelta(dx: 1, dy: 1)), motion(1511, 500, 10, 5),
        handleStep(.mouseButton(.right, isDown: true)), handleStep(.scroll(horizontal: 0, vertical: -120)), motion(1511, 500, 0, -1000),
        motion(0, 0, -5, 0), motion(0, 0, -5, 0), swallowStep(.mouseMoveAbsolute(x: 1, y: 1)), swallowStep(.mouseMoveAbsolute(x: 1, y: 1)),
        motion(1696, -500, 6, 0), motion(0, 0, 40, 0), ["op": "return"], ["op": "return"],
    ]))
    out.append(runScenario(name: "guards: jitter after entering, drag on the other computer", displays: [laptop], side: .right, remote: (1440, 900), forwardKeys: false, steps: [
        motion(1511, 500, 6, 0), motion(1511, 500, -1, 0), motion(0, 0, 9, 0), motion(0, 0, -20, 0), motion(0, 0, 10, 0),
        handleStep(.mouseButton(.left, isDown: true)), motion(0, 0, -30, 0), motion(0, 0, -30, 0),
        handleStep(.mouseButton(.left, isDown: false)), motion(0, 0, -5, 0), swallowStep(.mouseMoveAbsolute(x: 1, y: 1)),
    ]))
    out.append(runScenario(name: "keys forwarded", displays: [laptop], side: .left, remote: (1440, 900), forwardKeys: true, steps: [
        swallowStep(.key(code: 0, isDown: true, modifiers: [])), motion(0, 300, -6, 0), handleStep(.key(code: 0, isDown: true, modifiers: [.shift])),
        swallowStep(.key(code: 0, isDown: true, modifiers: [])), motion(1439, 300, 3, 0),
    ]))

    // Randomised, replayed identically by the Rust tests.
    let layouts: [(String, [Rect], Side, (Double, Double), Bool)] = [
        ("random laptop+monitor right", [laptop, above], .right, (1440, 900), false),
        ("random laptop+monitor left keys", [laptop, above], .left, (1728, 1117), true),
        ("random side by side top", [left, right], .top, (1280, 800), false),
        ("random single bottom keys", [laptop], .bottom, (1920, 1080), true),
        ("random tiny remote right", [laptop, above], .right, (30, 20), false),
    ]
    for (index, layout) in layouts.enumerated() {
        for round in 0..<4 {
            let steps = randomSteps(count: 120, displays: layout.1, seed: UInt64(1000 * (index + 1) + round))
            out.append(runScenario(name: "\(layout.0) #\(round)", displays: layout.1, side: layout.2, remote: layout.3, forwardKeys: layout.4, steps: steps))
        }
    }
    let data = try! JSONSerialization.data(withJSONObject: ["scenarios": out], options: [.sortedKeys, .withoutEscapingSlashes])
    print(String(data: data, encoding: .utf8)!)
}

let arguments = CommandLine.arguments
switch arguments.dropFirst().first {
case "gen": generate()
case "scenarios": generateScenarios()
case "verify":
    guard arguments.count >= 3 else { fail("usage: golden verify FILE") }
    verify(path: arguments[2])
default: fail("usage: golden gen | golden scenarios | golden verify FILE")
}
