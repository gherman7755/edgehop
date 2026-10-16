import CoreGraphics
import Foundation

public enum EventTapError: Error {
    /// Usually missing Accessibility / Input Monitoring permission for the host process.
    case createFailed
}

/// Session-level, head-inserted, *active* event tap on its own high-priority thread
/// (the same shape as CursorHop's capture, see report 01).
///
/// - Re-enables itself when macOS disables it (timeout / user input).
/// - Ignores events carrying `EventTap.marker` (our own injected events) to avoid feedback loops.
/// - Swallows (drops locally) mapped events while `shouldSwallow()` is true.
public final class EventTap {
    /// Written to `kCGEventSourceUserData` on every injected event ("EHOP").
    public static let marker: Int64 = 0x4548_4F50

    /// Called on the tap thread for each mapped event. Keep it fast: the system disables slow taps.
    public var onEvent: ((InputEvent) -> Void)?
    /// Called for every mouse move/drag with the real absolute position and the raw hardware delta
    /// (valid even when the cursor is pinned at a screen edge). Edge detection needs both.
    public var onMotion: ((_ position: Point, _ delta: Point) -> Void)?
    /// True if local apps must not see this event (it is being forwarded to a peer).
    public var shouldSwallow: (InputEvent) -> Bool = { _ in false }
    /// True while forwarding: moves are reported as deltas, otherwise as absolute positions.
    public var reportsDeltas: () -> Bool = { false }

    private var port: CFMachPort?
    private var runLoop: CFRunLoop?
    private var startError: Error?
    private let started = DispatchSemaphore(value: 0)
    private let statsLock = NSLock()
    private var mappedCount = 0
    private var ownIgnoredCount = 0

    public init() {}

    /// `mapped`: events delivered to `onEvent`. `ownIgnored`: events dropped because they carry
    /// `EventTap.marker`, i.e. our own injected events (proof that the feedback guard works).
    public var statistics: (mapped: Int, ownIgnored: Int) {
        statsLock.lock()
        defer { statsLock.unlock() }
        return (mappedCount, ownIgnoredCount)
    }

    public func start() throws {
        let thread = Thread { [weak self] in self?.run() }
        thread.name = "edgehop.eventtap"
        thread.qualityOfService = .userInteractive
        thread.start()
        started.wait()
        if let startError { throw startError }
    }

    public func stop() {
        if let runLoop { CFRunLoopStop(runLoop) }
    }

    private func run() {
        let callback: CGEventTapCallBack = { _, type, event, userInfo in
            guard let userInfo else { return Unmanaged.passUnretained(event) }
            let tap = Unmanaged<EventTap>.fromOpaque(userInfo).takeUnretainedValue()
            return tap.handle(type: type, event: event)
        }
        guard let port = CGEvent.tapCreate(
            tap: .cgSessionEventTap,
            place: .headInsertEventTap,
            options: .defaultTap,
            eventsOfInterest: ~CGEventMask(0),
            callback: callback,
            userInfo: Unmanaged.passUnretained(self).toOpaque()
        ) else {
            startError = EventTapError.createFailed
            started.signal()
            return
        }
        self.port = port
        let source = CFMachPortCreateRunLoopSource(nil, port, 0)
        let loop = CFRunLoopGetCurrent()
        runLoop = loop
        CFRunLoopAddSource(loop, source, .commonModes)
        CGEvent.tapEnable(tap: port, enable: true)
        started.signal()
        CFRunLoopRun()
        CGEvent.tapEnable(tap: port, enable: false)
    }

    private func handle(type: CGEventType, event: CGEvent) -> Unmanaged<CGEvent>? {
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            if let port { CGEvent.tapEnable(tap: port, enable: true) }
            return Unmanaged.passUnretained(event)
        }
        if event.getIntegerValueField(.eventSourceUserData) == Self.marker {
            statsLock.lock()
            ownIgnoredCount += 1
            statsLock.unlock()
            return Unmanaged.passUnretained(event)
        }
        if let onMotion, Self.isMove(type) {
            let p = event.location
            onMotion(
                Point(x: p.x, y: p.y),
                Point(
                    x: Double(event.getIntegerValueField(.mouseEventDeltaX)),
                    y: Double(event.getIntegerValueField(.mouseEventDeltaY))
                )
            )
        }
        guard let mapped = map(type: type, event: event) else {
            return Unmanaged.passUnretained(event)
        }
        statsLock.lock()
        mappedCount += 1
        statsLock.unlock()
        onEvent?(mapped)
        return shouldSwallow(mapped) ? nil : Unmanaged.passUnretained(event)
    }

    private static func isMove(_ type: CGEventType) -> Bool {
        switch type {
        case .mouseMoved, .leftMouseDragged, .rightMouseDragged, .otherMouseDragged: return true
        default: return false
        }
    }

    private func map(type: CGEventType, event: CGEvent) -> InputEvent? {
        switch type {
        case .mouseMoved, .leftMouseDragged, .rightMouseDragged, .otherMouseDragged:
            if reportsDeltas() {
                return .mouseMoveDelta(
                    dx: Int32(truncatingIfNeeded: event.getIntegerValueField(.mouseEventDeltaX)),
                    dy: Int32(truncatingIfNeeded: event.getIntegerValueField(.mouseEventDeltaY))
                )
            }
            let p = event.location
            return .mouseMoveAbsolute(x: Int32(p.x.rounded()), y: Int32(p.y.rounded()))
        case .leftMouseDown: return .mouseButton(.left, isDown: true)
        case .leftMouseUp: return .mouseButton(.left, isDown: false)
        case .rightMouseDown: return .mouseButton(.right, isDown: true)
        case .rightMouseUp: return .mouseButton(.right, isDown: false)
        case .otherMouseDown, .otherMouseUp:
            let n = event.getIntegerValueField(.mouseEventButtonNumber)
            let button: MouseButton = n == 3 ? .back : n == 4 ? .forward : .middle
            return .mouseButton(button, isDown: type == .otherMouseDown)
        case .scrollWheel:
            // Line deltas scaled to 1/120 notch units (wire convention).
            let v = event.getIntegerValueField(.scrollWheelEventDeltaAxis1)
            let h = event.getIntegerValueField(.scrollWheelEventDeltaAxis2)
            return .scroll(horizontal: Int32(truncatingIfNeeded: h * 120), vertical: Int32(truncatingIfNeeded: v * 120))
        case .keyDown, .keyUp:
            let mac = UInt16(truncatingIfNeeded: event.getIntegerValueField(.keyboardEventKeycode))
            guard let code = Keymap.hid(forMac: mac) else { return nil } // a key the other side has no name for (Fn)
            return .key(code: code, isDown: type == .keyDown, modifiers: Modifiers(event.flags))
        case .flagsChanged:
            // Modifiers are tracked from flagsChanged: the keycode says which key, the flag says up/down.
            let mac = UInt16(truncatingIfNeeded: event.getIntegerValueField(.keyboardEventKeycode))
            guard let flag = Modifiers.flag(forKeyCode: mac), let code = Keymap.hid(forMac: mac) else { return nil }
            return .key(code: code, isDown: event.flags.contains(flag), modifiers: Modifiers(event.flags))
        default:
            return nil
        }
    }
}

extension Modifiers {
    public init(_ flags: CGEventFlags) {
        var m: Modifiers = []
        if flags.contains(.maskShift) { m.insert(.shift) }
        if flags.contains(.maskControl) { m.insert(.control) }
        if flags.contains(.maskAlternate) { m.insert(.option) }
        if flags.contains(.maskCommand) { m.insert(.command) }
        self = m
    }

    public var cgFlags: CGEventFlags {
        var f: CGEventFlags = []
        if contains(.shift) { f.insert(.maskShift) }
        if contains(.control) { f.insert(.maskControl) }
        if contains(.option) { f.insert(.maskAlternate) }
        if contains(.command) { f.insert(.maskCommand) }
        return f
    }

    /// macOS keycodes 54...62 are the modifier keys (CursorHop's tap tracks the same range).
    static func flag(forKeyCode code: UInt16) -> CGEventFlags? {
        switch code {
        case 54, 55: return .maskCommand
        case 56, 60: return .maskShift
        case 57: return .maskAlphaShift
        case 58, 61: return .maskAlternate
        case 59, 62: return .maskControl
        default: return nil
        }
    }
}
