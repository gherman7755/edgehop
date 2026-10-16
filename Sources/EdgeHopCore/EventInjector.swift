import CoreGraphics
import Foundation

/// Replays `InputEvent`s on this Mac (the receiving side, see report 02).
///
/// - Every event is tagged with `EventTap.marker` so a local tap ignores it.
/// - Absolute moves: small jumps (<= `warpThreshold` px) become relative moves; large jumps warp the
///   cursor and re-associate mouse and cursor.
/// - Relative moves work from the first event: the starting point is the real cursor (`PointerState`),
///   re-seeded after `idleResync` seconds without injection so a locally moved mouse is respected.
/// - Scroll: multiples of 120 become line scrolls, anything else pixel scrolls with the fractional
///   remainder carried over.
/// - Modifier keys are posted as `flagsChanged`.
public final class EventInjector {
    public var warpThreshold: Double = 150
    public var idleResync: TimeInterval = 0.5

    private let source = CGEventSource(stateID: .hidSystemState)
    private var pointer = PointerState()
    private var lastInjection: DispatchTime?
    private var leftDown = false
    private var rightDown = false
    private var otherDown = false
    private var scrollRemainderH = 0.0
    private var scrollRemainderV = 0.0
    private var flags: CGEventFlags = []
    private var heldKeys = Set<UInt16>()
    private var heldButtons = Set<UInt8>()

    public init() {}

    public func inject(_ event: InputEvent) {
        resyncIfIdle()
        switch event {
        case let .mouseMoveAbsolute(x, y): moveAbsolute(CGPoint(x: Double(x), y: Double(y)))
        case let .mouseMoveDelta(dx, dy): moveRelative(dx: Double(dx), dy: Double(dy))
        case let .mouseButton(button, isDown): postButton(button, isDown: isDown)
        case let .scroll(h, v): postScroll(horizontal: h, vertical: v)
        case let .key(code, isDown, modifiers):
            // The wire carries HID usages; macOS wants its own virtual keycodes.
            if let mac = Keymap.mac(forHID: code) { postKey(code: mac, isDown: isDown, modifiers: modifiers) }
        }
    }

    /// Release every key and button we pressed. Send this when control returns to the local machine
    /// (CursorHop logs "[RETURN] sent full input state release").
    public func releaseAll() {
        for button in heldButtons.compactMap(MouseButton.init(rawValue:)) { postButton(button, isDown: false) }
        for code in heldKeys { postKey(code: code, isDown: false, modifiers: []) }
        heldKeys.removeAll()
        heldButtons.removeAll()
    }

    // MARK: Position

    private static func actualCursor() -> Point {
        let p = CGEvent(source: nil)?.location ?? .zero
        return Point(x: p.x, y: p.y)
    }

    /// Union of all displays (a rectangle: good enough to stop deltas from running off-screen).
    private static func desktopBounds() -> Rect {
        let displays = LocalDisplays.list()
        guard let first = displays.first else { return Rect(x: -100_000, y: -100_000, width: 200_000, height: 200_000) }
        var minX = first.x, minY = first.y, maxX = first.x + first.width, maxY = first.y + first.height
        for d in displays {
            minX = min(minX, d.x); minY = min(minY, d.y)
            maxX = max(maxX, d.x + d.width); maxY = max(maxY, d.y + d.height)
        }
        return Rect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
    }

    private func resyncIfIdle() {
        let now = DispatchTime.now()
        if let last = lastInjection,
           Double(now.uptimeNanoseconds - last.uptimeNanoseconds) / 1e9 > idleResync,
           !leftDown, !rightDown, !otherDown {
            pointer.reset()
        }
        lastInjection = now
    }

    private func currentPosition() -> CGPoint {
        let p = pointer.current(actual: Self.actualCursor)
        return CGPoint(x: p.x, y: p.y)
    }

    // MARK: Mouse

    private func moveAbsolute(_ target: CGPoint) {
        if let start = pointer.position {
            let dx = target.x - start.x
            let dy = target.y - start.y
            if hypot(dx, dy) <= warpThreshold {
                postMove(to: target, dx: dx, dy: dy)
                return
            }
        }
        CGWarpMouseCursorPosition(target)
        CGAssociateMouseAndMouseCursorPosition(1)
        postMove(to: target, dx: 0, dy: 0)
    }

    private func moveRelative(dx: Double, dy: Double) {
        let next = pointer.moved(dx: dx, dy: dy, actual: Self.actualCursor, bounds: Self.desktopBounds())
        postMove(to: CGPoint(x: next.x, y: next.y), dx: dx, dy: dy)
    }

    private func postMove(to target: CGPoint, dx: Double, dy: Double) {
        let type: CGEventType
        let button: CGMouseButton
        if leftDown { type = .leftMouseDragged; button = .left }
        else if rightDown { type = .rightMouseDragged; button = .right }
        else if otherDown { type = .otherMouseDragged; button = .center }
        else { type = .mouseMoved; button = .left }
        guard let ev = CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: target, mouseButton: button) else { return }
        ev.setIntegerValueField(.mouseEventDeltaX, value: Int64(dx))
        ev.setIntegerValueField(.mouseEventDeltaY, value: Int64(dy))
        post(ev)
        pointer.set(Point(x: target.x, y: target.y))
    }

    private func postButton(_ button: MouseButton, isDown: Bool) {
        let type: CGEventType
        let cgButton: CGMouseButton
        switch button {
        case .left: type = isDown ? .leftMouseDown : .leftMouseUp; cgButton = .left; leftDown = isDown
        case .right: type = isDown ? .rightMouseDown : .rightMouseUp; cgButton = .right; rightDown = isDown
        default: type = isDown ? .otherMouseDown : .otherMouseUp; cgButton = .center; otherDown = isDown
        }
        if isDown { heldButtons.insert(button.rawValue) } else { heldButtons.remove(button.rawValue) }
        guard let ev = CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: currentPosition(), mouseButton: cgButton) else { return }
        ev.setIntegerValueField(.mouseEventButtonNumber, value: Int64(button.rawValue))
        ev.setIntegerValueField(.mouseEventClickState, value: 1)
        post(ev)
    }

    private func postScroll(horizontal: Int32, vertical: Int32) {
        guard horizontal != 0 || vertical != 0 else { return }
        if horizontal % 120 == 0, vertical % 120 == 0 {
            emitScroll(units: .line, h: horizontal / 120, v: vertical / 120)
            return
        }
        // Pixel scrolling: 120 units == 10 px; keep the fraction for the next event.
        scrollRemainderH += Double(horizontal) / 12
        scrollRemainderV += Double(vertical) / 12
        let h = Int32(scrollRemainderH)
        let v = Int32(scrollRemainderV)
        scrollRemainderH -= Double(h)
        scrollRemainderV -= Double(v)
        if h != 0 || v != 0 { emitScroll(units: .pixel, h: h, v: v) }
    }

    private func emitScroll(units: CGScrollEventUnit, h: Int32, v: Int32) {
        guard let ev = CGEvent(scrollWheelEvent2Source: source, units: units, wheelCount: 2, wheel1: v, wheel2: h, wheel3: 0) else { return }
        post(ev)
    }

    // MARK: Keyboard

    private func postKey(code: UInt16, isDown: Bool, modifiers: Modifiers) {
        if isDown { heldKeys.insert(code) } else { heldKeys.remove(code) }
        guard let ev = CGEvent(keyboardEventSource: source, virtualKey: CGKeyCode(code), keyDown: isDown) else { return }
        if let flag = Modifiers.flag(forKeyCode: code) {
            if isDown { flags.insert(flag) } else { flags.remove(flag) }
            ev.type = .flagsChanged
            ev.flags = flags
        } else {
            ev.flags = modifiers.cgFlags
        }
        post(ev)
    }

    private func post(_ ev: CGEvent) {
        ev.setIntegerValueField(.eventSourceUserData, value: EventTap.marker)
        ev.post(tap: .cghidEventTap)
    }
}
