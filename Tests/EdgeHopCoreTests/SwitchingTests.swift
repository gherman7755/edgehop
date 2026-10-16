import Testing
@testable import EdgeHopCore

private final class Recorder {
    var sent: [InputEvent] = []
    var entered = 0
    var left: [Point] = []
    var released = 0
    var order: [String] = []

    lazy var actions = SwitchController.Actions(
        send: { [unowned self] in sent.append($0) },
        enterRemote: { [unowned self] in entered += 1; order.append("enter") },
        leaveRemote: { [unowned self] in left.append($0); order.append("leave") },
        releaseRemote: { [unowned self] in released += 1; order.append("release") }
    )
}

/// The real setup that exposed the bug: a 1512x982 laptop at the origin and a 1920x1080 monitor above it,
/// shifted left. The other Mac is 1440x900 and sits on the right.
private let laptop = Rect(x: 0, y: 0, width: 1512, height: 982)
private let monitorAbove = Rect(x: -223, y: -1080, width: 1920, height: 1080)

private func makeController(
    _ rec: Recorder, displays: [Rect] = [laptop, monitorAbove], side: Side = .right, forwardKeys: Bool = false
) -> SwitchController {
    SwitchController(
        displays: displays, side: side, remoteWidth: 1440, remoteHeight: 900,
        forwardKeys: forwardKeys, actions: rec.actions
    )
}

@Suite struct SwitchControllerTests {
    @Test func entersFromTheLaptopEdge() {
        let rec = Recorder()
        let c = makeController(rec)
        c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: 6, y: 0))
        #expect(c.mode == .remote)
        #expect(rec.entered == 1)
        #expect(rec.sent == [.mouseMoveAbsolute(x: 0, y: 500)]) // remote-local coordinates
    }

    /// The bug: only the main display's edge was a doorway, so pushing through the other monitor did nothing.
    @Test func entersFromTheOtherMonitorsOuterEdgeToo() {
        let rec = Recorder()
        let c = makeController(rec)
        // Monitor above spans x -223..1697, y -1080..0; its right edge has nothing next to it.
        c.handleMotion(position: Point(x: 1696, y: -500), delta: Point(x: 6, y: 0))
        #expect(c.mode == .remote)
        #expect(rec.sent == [.mouseMoveAbsolute(x: 0, y: 580)]) // offset from that display's top: -500 + 1080
    }

    @Test func aDisplayWithANeighbourOnThatSideIsNotADoorway() {
        let rec = Recorder()
        let a = Rect(x: 0, y: 0, width: 1000, height: 800)
        let b = Rect(x: 1000, y: 0, width: 1000, height: 800)
        let c = makeController(rec, displays: [a, b])
        c.handleMotion(position: Point(x: 999, y: 400), delta: Point(x: 6, y: 0)) // a's right edge touches b
        #expect(c.mode == .local)
        c.handleMotion(position: Point(x: 1999, y: 400), delta: Point(x: 6, y: 0)) // b's right edge is outer
        #expect(c.mode == .remote)
    }

    @Test func staysLocalAwayFromEdgesOrMovingAway() {
        let rec = Recorder()
        let c = makeController(rec)
        c.handleMotion(position: Point(x: 800, y: 500), delta: Point(x: 6, y: 0))
        c.handleMotion(position: Point(x: 0, y: 500), delta: Point(x: -6, y: 0)) // left edge, but the remote is on the right
        c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: -6, y: 0)) // at the edge, moving away
        #expect(c.mode == .local)
        #expect(rec.entered == 0)
    }

    @Test func otherSidesWork() {
        let left = Recorder()
        let cl = makeController(left, side: .left)
        cl.handleMotion(position: Point(x: 0, y: 300), delta: Point(x: -6, y: 0)) // y=300 has no display to the left
        #expect(cl.mode == .remote)
        #expect(left.sent == [.mouseMoveAbsolute(x: 1439, y: 300)])

        let top = Recorder()
        let ct = makeController(top, displays: [laptop], side: .top)
        ct.handleMotion(position: Point(x: 700, y: 0), delta: Point(x: 0, y: -6))
        #expect(ct.mode == .remote)
        #expect(top.sent == [.mouseMoveAbsolute(x: 700, y: 899)])
    }

    @Test func clampsTheEntryAlongTheEdgeToTheRemoteScreen() {
        let rec = Recorder()
        let c = makeController(rec, displays: [Rect(x: 0, y: 0, width: 1512, height: 1200)])
        c.handleMotion(position: Point(x: 1511, y: 1100), delta: Point(x: 6, y: 0)) // remote is only 900 tall
        #expect(rec.sent == [.mouseMoveAbsolute(x: 0, y: 899)])
    }

    @Test func sendsOnlyTheAppliedDeltaWhenTheRemoteCursorHitsItsBounds() {
        let rec = Recorder()
        let c = makeController(rec)
        c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: 6, y: 0))
        rec.sent.removeAll()
        c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: 10, y: 5))
        #expect(rec.sent == [.mouseMoveDelta(dx: 10, dy: 5)])
        rec.sent.removeAll()
        c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: 0, y: -1000)) // remote y: 505 -> 0
        #expect(rec.sent == [.mouseMoveDelta(dx: 0, dy: -505)])
    }

    @Test func returnsThroughTheFacingEdgeToTheDisplayWeLeft() {
        let rec = Recorder()
        let c = makeController(rec)
        c.handleMotion(position: Point(x: 1696, y: -500), delta: Point(x: 6, y: 0)) // left from the monitor above
        c.handleMotion(position: Point(x: 0, y: 0), delta: Point(x: 20, y: 0)) // come away from the entry edge
        c.handleMotion(position: Point(x: 0, y: 0), delta: Point(x: -25, y: 0)) // remote x reaches 0: facing edge
        #expect(c.mode == .local)
        #expect(rec.order == ["enter", "release", "leave"])
        #expect(rec.left == [Point(x: 1696, y: -500)]) // back on the monitor above, same height
        #expect(c.shouldSwallow(.mouseMoveAbsolute(x: 1, y: 1)))
        #expect(!c.shouldSwallow(.mouseMoveAbsolute(x: 1, y: 1)))
    }

    @Test func doesNotStartASwitchInTheMiddleOfADrag() {
        let rec = Recorder()
        let c = makeController(rec)
        c.handle(.mouseButton(.left, isDown: true))
        c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: 6, y: 0))
        #expect(c.mode == .local)
        c.handle(.mouseButton(.left, isDown: false))
        c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: 6, y: 0))
        #expect(c.mode == .remote)
    }

    @Test func forwardsButtonsAndScrollOnlyWhileRemote() {
        let rec = Recorder()
        let c = makeController(rec)
        c.handle(.scroll(horizontal: 0, vertical: 120))
        #expect(rec.sent.isEmpty)
        c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: 6, y: 0))
        rec.sent.removeAll()
        c.handle(.mouseButton(.right, isDown: true))
        c.handle(.scroll(horizontal: 0, vertical: -120))
        c.handle(.mouseMoveDelta(dx: 4, dy: 4)) // raw move events are handled via handleMotion, not forwarded twice
        #expect(rec.sent == [.mouseButton(.right, isDown: true), .scroll(horizontal: 0, vertical: -120)])
    }

    @Test func keysAreForwardedAndSwallowedOnlyWhenEnabled() {
        for enabled in [false, true] {
            let rec = Recorder()
            let c = makeController(rec, forwardKeys: enabled)
            let key = InputEvent.key(code: 0, isDown: true, modifiers: [])
            #expect(!c.shouldSwallow(key)) // local: never swallowed
            c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: 6, y: 0))
            rec.sent.removeAll()
            c.handle(key)
            #expect(rec.sent == (enabled ? [key] : []))
            #expect(c.shouldSwallow(key) == enabled) // keys keep working locally when they are not forwarded
            #expect(c.shouldSwallow(.mouseButton(.left, isDown: true)))
            #expect(c.shouldSwallow(.scroll(horizontal: 0, vertical: 1)))
            #expect(c.shouldSwallow(.mouseMoveDelta(dx: 1, dy: 1)))
        }
    }

    @Test func returnToLocalIsIdempotentAndUsesTheLastLocalPosition() {
        let rec = Recorder()
        let c = makeController(rec)
        c.returnToLocal() // while local: no-op
        #expect(rec.released == 0)
        c.handleMotion(position: Point(x: 1511, y: 321), delta: Point(x: 6, y: 0))
        c.returnToLocal()
        c.returnToLocal()
        #expect(c.mode == .local)
        #expect(rec.released == 1)
        #expect(rec.left == [Point(x: 1511, y: 321)])
        #expect(!c.shouldSwallow(.mouseMoveAbsolute(x: 5, y: 5))) // panic exit must not swallow a stray move
    }

    @Test func doesNotBounceStraightBackAfterEntering() {
        let rec = Recorder()
        let c = makeController(rec)
        c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: 6, y: 0))
        // A jitter to the left right after crossing only holds the cursor at the edge.
        c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: -1, y: 0))
        c.handleMotion(position: Point(x: 0, y: 0), delta: Point(x: 9, y: 0)) // 9 px in: not far enough yet
        c.handleMotion(position: Point(x: 0, y: 0), delta: Point(x: -20, y: 0))
        #expect(c.mode == .remote)
        #expect(rec.left.isEmpty)
        c.handleMotion(position: Point(x: 0, y: 0), delta: Point(x: 10, y: 0)) // 10 px in: armed
        c.handleMotion(position: Point(x: 0, y: 0), delta: Point(x: -20, y: 0))
        #expect(c.mode == .local)
    }

    @Test func aDragStartedOnTheOtherComputerStaysThere() {
        let rec = Recorder()
        let c = makeController(rec)
        c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: 6, y: 0))
        c.handleMotion(position: Point(x: 0, y: 0), delta: Point(x: 30, y: 0))
        c.handle(.mouseButton(.left, isDown: true))
        c.handleMotion(position: Point(x: 0, y: 0), delta: Point(x: -60, y: 0)) // through the facing edge, button held
        #expect(c.mode == .remote)
        #expect(rec.left.isEmpty)
        #expect(rec.sent.contains(.mouseButton(.left, isDown: true)))
        // The cursor was held at the edge: only the 30 px that were left were applied.
        #expect(rec.sent.last == .mouseMoveDelta(dx: -30, dy: 0))
        c.handle(.mouseButton(.left, isDown: false))
        c.handleMotion(position: Point(x: 0, y: 0), delta: Point(x: -5, y: 0))
        #expect(c.mode == .local)
    }

    private func key(_ code: UInt16, _ isDown: Bool) -> InputEvent { .key(code: code, isDown: isDown, modifiers: []) }

    @Test func aKeyHeldBeforeCrossingIsReleasedHereAndNeverForwarded() {
        let rec = Recorder()
        let c = makeController(rec, forwardKeys: true)
        let shift: UInt16 = 0xE1
        c.handle(key(shift, true))
        #expect(!c.shouldSwallow(key(shift, true)))
        c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: 6, y: 0))
        rec.sent.removeAll()

        c.handle(key(shift, true)) // a repeat
        #expect(c.shouldSwallow(key(shift, true)))
        c.handle(key(shift, false)) // the release must reach this computer's applications
        #expect(!c.shouldSwallow(key(shift, false)))
        #expect(rec.sent.isEmpty)

        c.handle(key(0x04, true))
        #expect(c.shouldSwallow(key(0x04, true)))
        c.handle(key(0x04, false))
        #expect(c.shouldSwallow(key(0x04, false)))
        #expect(rec.sent == [key(0x04, true), key(0x04, false)])
        c.handle(key(shift, true)) // released, so an ordinary key again
        #expect(c.shouldSwallow(key(shift, true)))
        #expect(rec.sent.last == key(shift, true))
    }

    @Test func aKeyStillDownWhenControlComesBackStaysHiddenUntilReleased() {
        let rec = Recorder()
        let c = makeController(rec, forwardKeys: true)
        c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: 6, y: 0))
        c.handle(key(0x04, true))
        #expect(c.shouldSwallow(key(0x04, true)))
        c.returnToLocal()

        c.handle(key(0x04, true))
        #expect(c.shouldSwallow(key(0x04, true)))
        c.handle(key(0x04, false))
        #expect(c.shouldSwallow(key(0x04, false)))
        c.handle(key(0x04, true))
        #expect(!c.shouldSwallow(key(0x04, true)))
    }

    @Test func keysPressedDuringASessionWithoutKeySharingAreTrackedAsLocal() {
        let rec = Recorder()
        let c = makeController(rec, forwardKeys: false)
        c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: 6, y: 0))
        rec.sent.removeAll()
        c.handle(key(0x04, true))
        #expect(!c.shouldSwallow(key(0x04, true)))
        c.returnToLocal()
        c.handle(key(0x04, false))
        #expect(!c.shouldSwallow(key(0x04, false)))
        #expect(rec.sent.isEmpty)
    }

    @Test func crossesAgainAfterReturning() {
        let rec = Recorder()
        let c = makeController(rec)
        c.handleMotion(position: Point(x: 1511, y: 500), delta: Point(x: 6, y: 0))
        c.returnToLocal()
        c.handleMotion(position: Point(x: 1511, y: 700), delta: Point(x: 6, y: 0))
        #expect(c.mode == .remote)
        #expect(rec.entered == 2)
        #expect(rec.sent.last == .mouseMoveAbsolute(x: 0, y: 700))
    }
}
