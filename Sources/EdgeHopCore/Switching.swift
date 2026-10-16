import Foundation

/// Decides when this Mac hands control to the remote one and back (report 04 [D] design).
///
/// Doorways: the outer edge, on `side`, of EVERY local display that has no other display next to it
/// there (a setup with a monitor above the laptop still works from either screen). The remote screen is
/// treated as attached to that display with the facing tops/lefts aligned, so the cursor keeps its
/// offset along the edge (clamped to the remote screen).
///
/// Flow, driven by the event tap:
///   `handleMotion` (every mouse move, with the real absolute position and raw delta) -> may switch;
///   `handle`       (every mapped event) -> forwards buttons/scroll/keys while remote;
///   `shouldSwallow`(same event) -> true while remote so local apps do not see it.
///
/// While remote, motion moves a `VirtualCursor` on the remote screen (remote-local coordinates, origin
/// at its top-left) and only the *applied* (clamped) delta is sent. All state is behind a lock: the tap
/// thread and the link's queue both call in.
public final class SwitchController {
    public enum Mode: Equatable, Sendable { case local, remote }

    public struct Actions {
        /// Send an event to the remote Mac.
        public var send: (InputEvent) -> Void
        /// Control just moved to the remote Mac (decouple/hide the local cursor, notify the user).
        public var enterRemote: () -> Void
        /// Control returns here: put the local cursor at this global position and re-couple it.
        public var leaveRemote: (Point) -> Void
        /// Tell the remote Mac to release every key and button we pressed there.
        public var releaseRemote: () -> Void

        public init(
            send: @escaping (InputEvent) -> Void,
            enterRemote: @escaping () -> Void,
            leaveRemote: @escaping (Point) -> Void,
            releaseRemote: @escaping () -> Void
        ) {
            self.send = send
            self.enterRemote = enterRemote
            self.leaveRemote = leaveRemote
            self.releaseRemote = releaseRemote
        }
    }

    /// Trigger band in px: within this distance of the edge, moving outward, counts as pushing through.
    public static let band = 2.0
    /// After entering the other computer, pushing back out only counts once the cursor has moved this many
    /// pixels away from the edge it entered at (a jitter right after crossing must not bounce straight back).
    public static let armDistance = 10.0

    private static let remoteId = "remote"
    private static let localId = "local"

    private let lock = NSLock()
    private let displays: [Rect]
    private let side: Side
    private let remoteWidth: Double
    private let remoteHeight: Double
    private let detector: EdgeDetector
    private let actions: Actions
    private let forwardKeys: Bool

    private var cursor: VirtualCursor?
    private var origin: Rect?
    private var state: Mode = .local
    /// Buttons held, in either mode: while one is, neither computer is left (the release would go astray).
    private var buttonsDown = 0
    /// The remote cursor has come `armDistance` away from the edge it entered at, so a return is allowed.
    private var armed = false
    private var lastLocalPosition = Point(x: 0, y: 0)
    private var swallowTransitionMove = false

    /// - Parameters:
    ///   - displays: frames of the local displays in global coordinates.
    ///   - side: where the remote Mac sits relative to this one.
    public init(
        displays: [Rect], side: Side, remoteWidth: Double, remoteHeight: Double,
        forwardKeys: Bool, actions: Actions
    ) {
        self.displays = displays
        self.side = side
        self.remoteWidth = remoteWidth
        self.remoteHeight = remoteHeight
        self.forwardKeys = forwardKeys
        self.actions = actions

        // Remote screen at the origin; a placeholder "local" screen on the facing side gives the
        // return edge somewhere to lead to.
        let remote = Monitor(id: Self.remoteId, deviceId: "remote", frame: Rect(x: 0, y: 0, width: remoteWidth, height: remoteHeight))
        let facing = side.opposite
        let placeholderFrame: Rect
        switch facing {
        case .left: placeholderFrame = Rect(x: -1000, y: 0, width: 1000, height: remoteHeight)
        case .right: placeholderFrame = Rect(x: remoteWidth, y: 0, width: 1000, height: remoteHeight)
        case .top: placeholderFrame = Rect(x: 0, y: -1000, width: remoteWidth, height: 1000)
        case .bottom: placeholderFrame = Rect(x: 0, y: remoteHeight, width: remoteWidth, height: 1000)
        }
        let local = Monitor(id: Self.localId, deviceId: "local", frame: placeholderFrame)
        self.detector = EdgeDetector(
            layout: Layout(monitors: [remote, local], edges: [Edge(from: Self.remoteId, side: facing, to: Self.localId)]),
            band: Self.band
        )
    }

    public var mode: Mode {
        lock.lock(); defer { lock.unlock() }
        return state
    }

    // MARK: Tap callbacks

    public func handleMotion(position: Point, delta: Point) {
        lock.lock(); defer { lock.unlock() }
        switch state {
        case .local:
            lastLocalPosition = position
            // Never start a switch in the middle of a drag: the button-up would go to the wrong machine.
            guard buttonsDown == 0, let display = doorway(at: position, delta: delta) else { return }
            let entry = remoteEntry(from: position, display: display)
            state = .remote
            armed = false
            origin = display
            cursor = VirtualCursor(detector: detector, monitorId: Self.remoteId, position: entry)
            actions.enterRemote()
            actions.send(.mouseMoveAbsolute(x: Int32(entry.x.rounded()), y: Int32(entry.y.rounded())))
        case .remote:
            guard var vc = cursor else { return }
            let before = vc.position
            let crossing = vc.apply(dx: delta.x, dy: delta.y)
            if let crossing, crossing.to == Self.localId {
                // Not while a button is held (a drag stays on this computer), and not before the cursor has come
                // away from the edge: then the push only holds the cursor at the edge.
                if armed && buttonsDown == 0 {
                    cursor = vc
                    leave(at: localReturn(from: crossing.entry), swallowCurrentMove: true)
                    return
                }
                vc = VirtualCursor(detector: detector, monitorId: Self.remoteId, position: Point(
                    x: clamp(before.x + delta.x, 0, remoteWidth - 1), y: clamp(before.y + delta.y, 0, remoteHeight - 1)))
            }
            cursor = vc
            if distanceFromFacingEdge(vc.position) >= Self.armDistance { armed = true }
            let dx = Int32((vc.position.x - before.x).rounded())
            let dy = Int32((vc.position.y - before.y).rounded())
            if dx != 0 || dy != 0 { actions.send(.mouseMoveDelta(dx: dx, dy: dy)) }
        }
    }

    public func handle(_ event: InputEvent) {
        lock.lock(); defer { lock.unlock() }
        switch (state, event) {
        case let (.local, .mouseButton(_, isDown)):
            buttonsDown = max(0, buttonsDown + (isDown ? 1 : -1))
        case (.local, _):
            break
        case (.remote, .mouseMoveAbsolute), (.remote, .mouseMoveDelta):
            break // motion goes through handleMotion
        case (.remote, .key):
            if forwardKeys { actions.send(event) }
        case let (.remote, .mouseButton(_, isDown)):
            buttonsDown = max(0, buttonsDown + (isDown ? 1 : -1))
            actions.send(event)
        case (.remote, _):
            actions.send(event)
        }
    }

    public func shouldSwallow(_ event: InputEvent) -> Bool {
        lock.lock(); defer { lock.unlock() }
        switch state {
        case .remote:
            if case .key = event { return forwardKeys } // keys keep working locally unless they are forwarded
            return true
        case .local:
            // The move that brought us back must not reach apps: its location predates the warp.
            if swallowTransitionMove, isMove(event) {
                swallowTransitionMove = false
                return true
            }
            return false
        }
    }

    /// Panic chord, lost link, Ctrl+C: always safe to call, idempotent.
    public func returnToLocal() {
        lock.lock(); defer { lock.unlock() }
        guard state == .remote else { return }
        leave(at: lastLocalPosition, swallowCurrentMove: false)
    }

    // MARK: Geometry (lock held)

    /// The display to leave from if `position` is pushing through that display's outer `side` edge.
    private func doorway(at p: Point, delta d: Point) -> Rect? {
        guard let display = display(containing: p) else { return nil }
        let atEdge: Bool
        let beyond: Point
        switch side {
        case .right: atEdge = p.x >= display.maxX - Self.band && d.x > 0; beyond = Point(x: display.maxX + 0.5, y: p.y)
        case .left: atEdge = p.x <= display.minX + Self.band - 1 && d.x < 0; beyond = Point(x: display.minX - 0.5, y: p.y)
        case .bottom: atEdge = p.y >= display.maxY - Self.band && d.y > 0; beyond = Point(x: p.x, y: display.maxY + 0.5)
        case .top: atEdge = p.y <= display.minY + Self.band - 1 && d.y < 0; beyond = Point(x: p.x, y: display.minY - 0.5)
        }
        guard atEdge, !displays.contains(where: { $0.contains(beyond) }) else { return nil }
        return display
    }

    private func display(containing p: Point) -> Rect? {
        if let hit = displays.first(where: { $0.contains(p) }) { return hit }
        // Positions exactly on a far edge fall just outside `contains`: take the nearest display.
        return displays.min { distance(from: p, to: $0) < distance(from: p, to: $1) }
    }

    private func distance(from p: Point, to r: Rect) -> Double {
        let dx = max(r.minX - p.x, 0, p.x - r.maxX)
        let dy = max(r.minY - p.y, 0, p.y - r.maxY)
        return (dx * dx + dy * dy).squareRoot()
    }

    /// Remote-local entry point: the offset along the edge is preserved relative to the display, clamped.
    private func remoteEntry(from p: Point, display: Rect) -> Point {
        switch side {
        case .right: return Point(x: 0, y: clamp(p.y - display.minY, 0, remoteHeight - 1))
        case .left: return Point(x: remoteWidth - 1, y: clamp(p.y - display.minY, 0, remoteHeight - 1))
        case .bottom: return Point(x: clamp(p.x - display.minX, 0, remoteWidth - 1), y: 0)
        case .top: return Point(x: clamp(p.x - display.minX, 0, remoteWidth - 1), y: remoteHeight - 1)
        }
    }

    /// Global position on the origin display for a return crossing (`exit` is in the placeholder's space,
    /// whose along-edge coordinate equals the remote-local one).
    /// How far `p` is from the remote edge that faces this computer (the one the cursor entered at).
    private func distanceFromFacingEdge(_ p: Point) -> Double {
        switch side {
        case .right: return p.x
        case .left: return remoteWidth - 1 - p.x
        case .bottom: return p.y
        case .top: return remoteHeight - 1 - p.y
        }
    }

    private func localReturn(from exit: Point) -> Point {
        guard let d = origin else { return lastLocalPosition }
        switch side {
        case .right: return Point(x: d.maxX - 1, y: clamp(d.minY + exit.y, d.minY, d.maxY - 1))
        case .left: return Point(x: d.minX, y: clamp(d.minY + exit.y, d.minY, d.maxY - 1))
        case .bottom: return Point(x: clamp(d.minX + exit.x, d.minX, d.maxX - 1), y: d.maxY - 1)
        case .top: return Point(x: clamp(d.minX + exit.x, d.minX, d.maxX - 1), y: d.minY)
        }
    }

    private func leave(at position: Point, swallowCurrentMove: Bool) {
        state = .local
        cursor = nil
        origin = nil
        buttonsDown = 0
        swallowTransitionMove = swallowCurrentMove
        actions.releaseRemote()
        actions.leaveRemote(position)
    }

    private func isMove(_ event: InputEvent) -> Bool {
        switch event {
        case .mouseMoveAbsolute, .mouseMoveDelta: return true
        default: return false
        }
    }
}
