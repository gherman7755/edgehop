import Foundation

/// Errors a handler can throw; they become `{"error": {"code", "message"}}` responses.
public enum IPCError: Error, Equatable {
    case invalidParams(String)
    case unknownMethod(String)
    case busy(String)
    case `internal`(String)

    var code: String {
        switch self {
        case .invalidParams: return "invalid_params"
        case .unknownMethod: return "unknown_method"
        case .busy: return "busy"
        case .internal: return "internal"
        }
    }

    var message: String {
        switch self {
        case let .invalidParams(text), let .unknownMethod(text), let .busy(text), let .internal(text): return text
        }
    }
}

public typealias IPCEmit = (_ event: String, _ data: [String: Any]) -> Void

/// What the server dispatches requests to. `CoreService` is the real one; tests use a fake.
public protocol IPCHandler: AnyObject {
    func handle(method: String, params: [String: Any]) throws -> [String: Any]
    /// Stop everything and give the cursor back. Called on `core.shutdown` and on end of input.
    func shutdown()
}

/// One line in, one line out. Blocking read; `nil` means end of input.
public protocol LineTransport: AnyObject {
    func readLine() -> String?
    func write(_ line: String)
}

public final class StdioTransport: LineTransport {
    public init() {}

    public func readLine() -> String? { Swift.readLine(strippingNewline: true) }

    public func write(_ line: String) {
        // `try?`: a closed pipe (the UI is gone) must not crash us before we can restore the cursor.
        try? FileHandle.standardOutput.write(contentsOf: Data((line + "\n").utf8))
    }
}

/// Newline-delimited JSON server (see docs/IPC.md). Requests are handled one at a time in arrival order;
/// events can be emitted from any thread.
public final class IPCServer {
    private let transport: LineTransport
    private let writeLock = NSLock()
    private var handler: IPCHandler!
    private var stopping = false

    public init(transport: LineTransport, makeHandler: (@escaping IPCEmit) -> IPCHandler) {
        self.transport = transport
        self.handler = makeHandler { [unowned self] event, data in self.send(event: event, data: data) }
    }

    /// Blocks until the input ends or `core.shutdown` is received, then shuts the handler down.
    public func run() {
        while !stopping, let line = transport.readLine() {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.isEmpty { continue }
            process(trimmed)
        }
        handler.shutdown()
    }

    // MARK: Requests

    private func process(_ line: String) {
        guard let data = line.data(using: .utf8),
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        else {
            return respond(id: NSNull(), error: ("parse_error", "line is not a JSON object"))
        }
        let id = object["id"] ?? NSNull()
        guard Self.isValidId(id) else {
            return respond(id: NSNull(), error: ("invalid_request", "\"id\" must be a number or a string"))
        }
        guard let method = object["method"] as? String, !method.isEmpty else {
            return respond(id: id, error: ("invalid_request", "\"method\" must be a non-empty string"))
        }
        let params: [String: Any]
        if let given = object["params"] {
            guard let dictionary = given as? [String: Any] else {
                return respond(id: id, error: ("invalid_params", "\"params\" must be an object"))
            }
            params = dictionary
        } else {
            params = [:]
        }

        if method == "core.shutdown" {
            respond(id: id, result: [:])
            stopping = true
            return
        }
        do {
            respond(id: id, result: try handler.handle(method: method, params: params))
        } catch let error as IPCError {
            respond(id: id, error: (error.code, error.message))
        } catch {
            respond(id: id, error: ("internal", "\(error)"))
        }
    }

    /// A number or a string. JSON booleans also arrive as `NSNumber`, so exclude them explicitly.
    private static func isValidId(_ id: Any) -> Bool {
        if id is String { return true }
        guard let number = id as? NSNumber else { return false }
        return CFGetTypeID(number) != CFBooleanGetTypeID()
    }

    // MARK: Output

    private func respond(id: Any, result: [String: Any]) {
        write(["id": id, "result": result])
    }

    private func respond(id: Any, error: (code: String, message: String)) {
        write(["id": id, "error": ["code": error.code, "message": error.message]])
    }

    public func send(event: String, data: [String: Any]) {
        write(["event": event, "data": data])
    }

    private func write(_ object: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.withoutEscapingSlashes, .sortedKeys]),
              let line = String(data: data, encoding: .utf8)
        else { return }
        writeLock.lock()
        transport.write(line)
        writeLock.unlock()
    }
}
