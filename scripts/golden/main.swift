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

let arguments = CommandLine.arguments
switch arguments.dropFirst().first {
case "gen": generate()
case "verify":
    guard arguments.count >= 3 else { fail("usage: golden verify FILE") }
    verify(path: arguments[2])
default: fail("usage: golden gen | golden verify FILE")
}
