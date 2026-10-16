use std::sync::{Mutex, MutexGuard, PoisonError};

use edgehop_proto::InputEvent;

use crate::geometry::{Point, Rect, Side};

/// Trigger band in pixels: within this distance of an edge, moving outward, counts as pushing through.
pub const BAND: f64 = 2.0;

/// After entering the other computer, pushing back out only counts once the cursor has moved this many pixels
/// away from the edge it entered at. The cursor starts exactly on that edge, so without this a one-pixel
/// jitter of the mouse right after crossing would bounce straight back.
pub const ARM_DISTANCE: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// This computer's keyboard and mouse drive this computer.
    Local,
    /// They drive the other computer.
    Remote,
}

/// What the platform must do when control changes hands. Called while the controller's lock is held: they must
/// not call back into the controller.
pub trait Actions: Send + Sync {
    /// Send an event to the other computer.
    fn send(&self, event: InputEvent);
    /// Control just moved to the other computer (pin or hide the local cursor, notify the user).
    fn enter_remote(&self);
    /// Control returns here: put the local cursor at this global position and release it.
    fn leave_remote(&self, to: Point);
    /// Tell the other computer to release every key and button we pressed there.
    fn release_remote(&self);
}

/// The cursor on the other computer's screen while it is controlled (its own coordinates, origin top-left).
#[derive(Debug, Clone, Copy)]
struct RemoteCursor {
    x: f64,
    y: f64,
}

struct Inner {
    mode: Mode,
    cursor: Option<RemoteCursor>,
    /// The local display we left from.
    origin: Option<Rect>,
    /// Buttons held, in either mode: while one is, neither computer is left (the release would go astray).
    buttons_down: usize,
    /// The remote cursor has moved [`ARM_DISTANCE`] away from the edge it entered at, so a return is allowed.
    armed: bool,
    last_local: Point,
    swallow_transition_move: bool,
}

/// Decides when this computer hands control to the other one and back.
///
/// Doorways: the outer edge, on `side`, of EVERY local display that has no other display next to it there (a
/// setup with a monitor above the laptop still works from either screen). The remote screen is treated as
/// attached to that display with the facing tops/lefts aligned, so the cursor keeps its offset along the edge
/// (clamped to the remote screen).
///
/// Flow, driven by the platform's capture: [`Self::handle_motion`] for every mouse move (real absolute position
/// and device delta), then [`Self::handle`] and [`Self::should_swallow`] for every mapped event. While remote,
/// motion moves a virtual cursor on the remote screen and only the *applied* (clamped) delta is sent. State is
/// behind a lock: the capture thread and the link's threads both call in.
pub struct SwitchController<A: Actions> {
    displays: Vec<Rect>,
    side: Side,
    remote_width: f64,
    remote_height: f64,
    forward_keys: bool,
    actions: A,
    inner: Mutex<Inner>,
}

fn clamp(value: f64, low: f64, high: f64) -> f64 {
    value.max(low).min(high.max(low))
}

fn is_move(event: &InputEvent) -> bool {
    matches!(
        event,
        InputEvent::MouseMoveAbsolute { .. } | InputEvent::MouseMoveDelta { .. }
    )
}

impl<A: Actions> SwitchController<A> {
    /// `displays`: frames of the local displays in global coordinates. `side`: where the other computer sits.
    pub fn new(
        displays: Vec<Rect>,
        side: Side,
        remote_width: f64,
        remote_height: f64,
        forward_keys: bool,
        actions: A,
    ) -> Self {
        Self {
            displays,
            side,
            remote_width,
            remote_height,
            forward_keys,
            actions,
            inner: Mutex::new(Inner {
                mode: Mode::Local,
                cursor: None,
                origin: None,
                buttons_down: 0,
                armed: false,
                last_local: Point::default(),
                swallow_transition_move: false,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn mode(&self) -> Mode {
        self.lock().mode
    }

    /// A mouse move: `position` is where the real cursor is, `delta` the device movement.
    pub fn handle_motion(&self, position: Point, delta: Point) {
        let mut inner = self.lock();
        match inner.mode {
            Mode::Local => {
                inner.last_local = position;
                // Never start a switch in the middle of a drag: the button-up would go to the wrong computer.
                if inner.buttons_down != 0 {
                    return;
                }
                let Some(display) = self.doorway(position, delta) else {
                    return;
                };
                let entry = self.remote_entry(position, &display);
                inner.mode = Mode::Remote;
                inner.armed = false;
                inner.origin = Some(display);
                inner.cursor = Some(RemoteCursor {
                    x: entry.x,
                    y: entry.y,
                });
                self.actions.enter_remote();
                self.actions.send(InputEvent::MouseMoveAbsolute {
                    x: entry.x.round() as i32,
                    y: entry.y.round() as i32,
                });
            }
            Mode::Remote => {
                let Some(before) = inner.cursor else { return };
                let moved = RemoteCursor {
                    x: clamp(before.x + delta.x, 0.0, self.remote_width - 1.0),
                    y: clamp(before.y + delta.y, 0.0, self.remote_height - 1.0),
                };
                if let Some(along) = self.return_crossing(moved, delta) {
                    // Not while a button is held (a drag stays on this computer), and not before the cursor
                    // has come away from the edge: then the push only holds the cursor at the edge.
                    if inner.armed && inner.buttons_down == 0 {
                        let to = self.local_return(&inner, along);
                        self.leave(&mut inner, to, true);
                        return;
                    }
                }
                if self.distance_from_facing_edge(moved) >= ARM_DISTANCE {
                    inner.armed = true;
                }
                inner.cursor = Some(moved);
                let dx = (moved.x - before.x).round() as i32;
                let dy = (moved.y - before.y).round() as i32;
                if dx != 0 || dy != 0 {
                    self.actions.send(InputEvent::MouseMoveDelta { dx, dy });
                }
            }
        }
    }

    /// Every mapped event, after [`Self::handle_motion`] for moves.
    pub fn handle(&self, event: &InputEvent) {
        let mut inner = self.lock();
        match (inner.mode, event) {
            (Mode::Local, InputEvent::MouseButton { is_down, .. }) => {
                inner.buttons_down = if *is_down {
                    inner.buttons_down + 1
                } else {
                    inner.buttons_down.saturating_sub(1)
                };
            }
            (Mode::Local, _) => {}
            // Motion goes through `handle_motion`.
            (
                Mode::Remote,
                InputEvent::MouseMoveAbsolute { .. } | InputEvent::MouseMoveDelta { .. },
            ) => {}
            (Mode::Remote, InputEvent::Key { .. }) => {
                if self.forward_keys {
                    self.actions.send(*event);
                }
            }
            (Mode::Remote, InputEvent::MouseButton { is_down, .. }) => {
                inner.buttons_down = if *is_down {
                    inner.buttons_down + 1
                } else {
                    inner.buttons_down.saturating_sub(1)
                };
                self.actions.send(*event);
            }
            (Mode::Remote, _) => self.actions.send(*event),
        }
    }

    /// True if local applications must not see `event`.
    pub fn should_swallow(&self, event: &InputEvent) -> bool {
        let mut inner = self.lock();
        match inner.mode {
            // Keys keep working locally unless they are forwarded.
            Mode::Remote => !matches!(event, InputEvent::Key { .. }) || self.forward_keys,
            Mode::Local => {
                // The move that brought us back must not reach applications: its location predates the warp.
                if inner.swallow_transition_move && is_move(event) {
                    inner.swallow_transition_move = false;
                    return true;
                }
                false
            }
        }
    }

    /// Panic chord, lost link, shutdown: always safe to call, idempotent.
    pub fn return_to_local(&self) {
        let mut inner = self.lock();
        if inner.mode == Mode::Remote {
            let to = inner.last_local;
            self.leave(&mut inner, to, false);
        }
    }

    fn leave(&self, inner: &mut Inner, to: Point, swallow_current_move: bool) {
        inner.mode = Mode::Local;
        inner.cursor = None;
        inner.origin = None;
        inner.buttons_down = 0;
        inner.swallow_transition_move = swallow_current_move;
        self.actions.release_remote();
        self.actions.leave_remote(to);
    }

    /// The display to leave from if `p` is pushing through that display's outer `side` edge.
    fn doorway(&self, p: Point, d: Point) -> Option<Rect> {
        let display = self.display_at(p)?;
        let (at_edge, beyond) = match self.side {
            Side::Right => (
                p.x >= display.max_x() - BAND && d.x > 0.0,
                Point::new(display.max_x() + 0.5, p.y),
            ),
            Side::Left => (
                p.x <= display.min_x() + BAND - 1.0 && d.x < 0.0,
                Point::new(display.min_x() - 0.5, p.y),
            ),
            Side::Bottom => (
                p.y >= display.max_y() - BAND && d.y > 0.0,
                Point::new(p.x, display.max_y() + 0.5),
            ),
            Side::Top => (
                p.y <= display.min_y() + BAND - 1.0 && d.y < 0.0,
                Point::new(p.x, display.min_y() - 0.5),
            ),
        };
        (at_edge && !self.displays.iter().any(|other| other.contains(beyond))).then_some(display)
    }

    fn display_at(&self, p: Point) -> Option<Rect> {
        if let Some(hit) = self.displays.iter().find(|display| display.contains(p)) {
            return Some(*hit);
        }
        // Positions exactly on a far edge fall just outside `contains`: take the nearest display.
        self.displays
            .iter()
            .copied()
            .min_by(|a, b| a.distance_to(p).total_cmp(&b.distance_to(p)))
    }

    /// Remote-local entry point: the offset along the edge is preserved relative to the display, clamped.
    fn remote_entry(&self, p: Point, display: &Rect) -> Point {
        let (w, h) = (self.remote_width, self.remote_height);
        match self.side {
            Side::Right => Point::new(0.0, clamp(p.y - display.min_y(), 0.0, h - 1.0)),
            Side::Left => Point::new(w - 1.0, clamp(p.y - display.min_y(), 0.0, h - 1.0)),
            Side::Bottom => Point::new(clamp(p.x - display.min_x(), 0.0, w - 1.0), 0.0),
            Side::Top => Point::new(clamp(p.x - display.min_x(), 0.0, w - 1.0), h - 1.0),
        }
    }

    /// How far `p` is from the remote edge that faces this computer (the one the cursor entered at).
    fn distance_from_facing_edge(&self, p: RemoteCursor) -> f64 {
        match self.side {
            Side::Right => p.x,
            Side::Left => self.remote_width - 1.0 - p.x,
            Side::Bottom => p.y,
            Side::Top => self.remote_height - 1.0 - p.y,
        }
    }

    /// If the remote cursor, now at `moved`, is pushing out through the edge that faces this computer, the
    /// coordinate along that edge (the other coordinate is the edge itself).
    fn return_crossing(&self, moved: RemoteCursor, d: Point) -> Option<f64> {
        let (w, h) = (self.remote_width, self.remote_height);
        match self.side.opposite() {
            Side::Left => (moved.x < BAND && d.x < 0.0).then_some(moved.y),
            Side::Right => (moved.x >= w - BAND && d.x > 0.0).then_some(moved.y),
            Side::Top => (moved.y < BAND && d.y < 0.0).then_some(moved.x),
            Side::Bottom => (moved.y >= h - BAND && d.y > 0.0).then_some(moved.x),
        }
    }

    /// Global position on the display we left from, for a return crossing at `along` on the remote's edge.
    fn local_return(&self, inner: &Inner, along: f64) -> Point {
        let Some(d) = inner.origin else {
            return inner.last_local;
        };
        match self.side {
            Side::Right => Point::new(
                d.max_x() - 1.0,
                clamp(d.min_y() + along, d.min_y(), d.max_y() - 1.0),
            ),
            Side::Left => Point::new(
                d.min_x(),
                clamp(d.min_y() + along, d.min_y(), d.max_y() - 1.0),
            ),
            Side::Bottom => Point::new(
                clamp(d.min_x() + along, d.min_x(), d.max_x() - 1.0),
                d.max_y() - 1.0,
            ),
            Side::Top => Point::new(
                clamp(d.min_x() + along, d.min_x(), d.max_x() - 1.0),
                d.min_y(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use edgehop_proto::{Modifiers, MouseButton};

    use super::*;

    #[derive(Default)]
    struct Recorder {
        sent: Mutex<Vec<InputEvent>>,
        entered: AtomicUsize,
        released: AtomicUsize,
        left: Mutex<Vec<Point>>,
        order: Mutex<Vec<&'static str>>,
    }

    impl Recorder {
        fn sent(&self) -> Vec<InputEvent> {
            self.sent.lock().unwrap().clone()
        }
        fn clear_sent(&self) {
            self.sent.lock().unwrap().clear();
        }
        fn left(&self) -> Vec<Point> {
            self.left.lock().unwrap().clone()
        }
        fn order(&self) -> Vec<&'static str> {
            self.order.lock().unwrap().clone()
        }
    }

    impl Actions for Arc<Recorder> {
        fn send(&self, event: InputEvent) {
            self.sent.lock().unwrap().push(event);
        }
        fn enter_remote(&self) {
            self.entered.fetch_add(1, Ordering::SeqCst);
            self.order.lock().unwrap().push("enter");
        }
        fn leave_remote(&self, to: Point) {
            self.left.lock().unwrap().push(to);
            self.order.lock().unwrap().push("leave");
        }
        fn release_remote(&self) {
            self.released.fetch_add(1, Ordering::SeqCst);
            self.order.lock().unwrap().push("release");
        }
    }

    // The real setup that exposed a bug in the Swift core: a 1512x982 laptop at the origin and a 1920x1080
    // monitor above it, shifted left. The other computer is 1440x900.
    const LAPTOP: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1512.0,
        height: 982.0,
    };
    const MONITOR_ABOVE: Rect = Rect {
        x: -223.0,
        y: -1080.0,
        width: 1920.0,
        height: 1080.0,
    };

    fn controller(
        displays: Vec<Rect>,
        side: Side,
        forward_keys: bool,
    ) -> (SwitchController<Arc<Recorder>>, Arc<Recorder>) {
        let recorder = Arc::new(Recorder::default());
        (
            SwitchController::new(
                displays,
                side,
                1440.0,
                900.0,
                forward_keys,
                recorder.clone(),
            ),
            recorder,
        )
    }

    fn standard() -> (SwitchController<Arc<Recorder>>, Arc<Recorder>) {
        controller(vec![LAPTOP, MONITOR_ABOVE], Side::Right, false)
    }

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn abs(x: i32, y: i32) -> InputEvent {
        InputEvent::MouseMoveAbsolute { x, y }
    }

    fn delta(dx: i32, dy: i32) -> InputEvent {
        InputEvent::MouseMoveDelta { dx, dy }
    }

    fn button(button: MouseButton, is_down: bool) -> InputEvent {
        InputEvent::MouseButton { button, is_down }
    }

    #[test]
    fn enters_from_the_laptop_edge() {
        let (c, rec) = standard();
        c.handle_motion(p(1511.0, 500.0), p(6.0, 0.0));
        assert_eq!(c.mode(), Mode::Remote);
        assert_eq!(rec.entered.load(Ordering::SeqCst), 1);
        assert_eq!(rec.sent(), vec![abs(0, 500)]); // remote-local coordinates
    }

    /// The Swift bug: only the main display's edge was a doorway, so the other monitor did nothing.
    #[test]
    fn enters_from_the_other_monitors_outer_edge_too() {
        let (c, rec) = standard();
        c.handle_motion(p(1696.0, -500.0), p(6.0, 0.0));
        assert_eq!(c.mode(), Mode::Remote);
        assert_eq!(rec.sent(), vec![abs(0, 580)]); // offset from that display's top: -500 + 1080
    }

    #[test]
    fn a_display_with_a_neighbour_on_that_side_is_not_a_doorway() {
        let a = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let b = Rect::new(1000.0, 0.0, 1000.0, 800.0);
        let (c, _) = controller(vec![a, b], Side::Right, false);
        c.handle_motion(p(999.0, 400.0), p(6.0, 0.0)); // a's right edge touches b
        assert_eq!(c.mode(), Mode::Local);
        c.handle_motion(p(1999.0, 400.0), p(6.0, 0.0)); // b's right edge is outer
        assert_eq!(c.mode(), Mode::Remote);
    }

    #[test]
    fn stays_local_away_from_edges_or_moving_away() {
        let (c, rec) = standard();
        c.handle_motion(p(800.0, 500.0), p(6.0, 0.0));
        c.handle_motion(p(0.0, 500.0), p(-6.0, 0.0)); // left edge, but the other computer is on the right
        c.handle_motion(p(1511.0, 500.0), p(-6.0, 0.0)); // at the edge, moving away
        assert_eq!(c.mode(), Mode::Local);
        assert_eq!(rec.entered.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn other_sides_work() {
        let (left, rec) = controller(vec![LAPTOP, MONITOR_ABOVE], Side::Left, false);
        left.handle_motion(p(0.0, 300.0), p(-6.0, 0.0)); // y = 300 has no display to the left
        assert_eq!(left.mode(), Mode::Remote);
        assert_eq!(rec.sent(), vec![abs(1439, 300)]);

        let (top, rec) = controller(vec![LAPTOP], Side::Top, false);
        top.handle_motion(p(700.0, 0.0), p(0.0, -6.0));
        assert_eq!(top.mode(), Mode::Remote);
        assert_eq!(rec.sent(), vec![abs(700, 899)]);

        let (bottom, rec) = controller(vec![LAPTOP], Side::Bottom, false);
        bottom.handle_motion(p(700.0, 981.0), p(0.0, 6.0));
        assert_eq!(rec.sent(), vec![abs(700, 0)]);
    }

    #[test]
    fn clamps_the_entry_along_the_edge_to_the_remote_screen() {
        let (c, rec) = controller(
            vec![Rect::new(0.0, 0.0, 1512.0, 1200.0)],
            Side::Right,
            false,
        );
        c.handle_motion(p(1511.0, 1100.0), p(6.0, 0.0)); // the other computer is only 900 tall
        assert_eq!(rec.sent(), vec![abs(0, 899)]);
    }

    #[test]
    fn sends_only_the_applied_delta_when_the_remote_cursor_hits_its_bounds() {
        let (c, rec) = standard();
        c.handle_motion(p(1511.0, 500.0), p(6.0, 0.0));
        rec.clear_sent();
        c.handle_motion(p(1511.0, 500.0), p(10.0, 5.0));
        assert_eq!(rec.sent(), vec![delta(10, 5)]);
        rec.clear_sent();
        c.handle_motion(p(1511.0, 500.0), p(0.0, -1000.0)); // remote y: 505 -> 0
        assert_eq!(rec.sent(), vec![delta(0, -505)]);
    }

    #[test]
    fn returns_through_the_facing_edge_to_the_display_we_left() {
        let (c, rec) = standard();
        c.handle_motion(p(1696.0, -500.0), p(6.0, 0.0)); // left from the monitor above
        c.handle_motion(p(0.0, 0.0), p(20.0, 0.0)); // come away from the entry edge
        c.handle_motion(p(0.0, 0.0), p(-25.0, 0.0)); // remote x reaches 0: facing edge
        assert_eq!(c.mode(), Mode::Local);
        assert_eq!(rec.order(), vec!["enter", "release", "leave"]);
        assert_eq!(rec.left(), vec![p(1696.0, -500.0)]); // back on the monitor above, same height
        assert!(c.should_swallow(&abs(1, 1)));
        assert!(!c.should_swallow(&abs(1, 1)));
    }

    #[test]
    fn does_not_start_a_switch_in_the_middle_of_a_drag() {
        let (c, _) = standard();
        c.handle(&button(MouseButton::Left, true));
        c.handle_motion(p(1511.0, 500.0), p(6.0, 0.0));
        assert_eq!(c.mode(), Mode::Local);
        c.handle(&button(MouseButton::Left, false));
        c.handle_motion(p(1511.0, 500.0), p(6.0, 0.0));
        assert_eq!(c.mode(), Mode::Remote);
    }

    #[test]
    fn forwards_buttons_and_scroll_only_while_remote() {
        let (c, rec) = standard();
        c.handle(&InputEvent::Scroll {
            horizontal: 0,
            vertical: 120,
        });
        assert!(rec.sent().is_empty());
        c.handle_motion(p(1511.0, 500.0), p(6.0, 0.0));
        rec.clear_sent();
        c.handle(&button(MouseButton::Right, true));
        c.handle(&InputEvent::Scroll {
            horizontal: 0,
            vertical: -120,
        });
        c.handle(&delta(4, 4)); // raw move events go through handle_motion, not forwarded twice
        assert_eq!(
            rec.sent(),
            vec![
                button(MouseButton::Right, true),
                InputEvent::Scroll {
                    horizontal: 0,
                    vertical: -120
                }
            ]
        );
    }

    #[test]
    fn keys_are_forwarded_and_swallowed_only_when_enabled() {
        for enabled in [false, true] {
            let (c, rec) = controller(vec![LAPTOP, MONITOR_ABOVE], Side::Right, enabled);
            let key = InputEvent::Key {
                code: 0,
                is_down: true,
                modifiers: Modifiers::NONE,
            };
            assert!(!c.should_swallow(&key)); // local: never swallowed
            c.handle_motion(p(1511.0, 500.0), p(6.0, 0.0));
            rec.clear_sent();
            c.handle(&key);
            assert_eq!(rec.sent(), if enabled { vec![key] } else { vec![] });
            assert_eq!(c.should_swallow(&key), enabled); // keys keep working locally when they are not forwarded
            assert!(c.should_swallow(&button(MouseButton::Left, true)));
            assert!(c.should_swallow(&InputEvent::Scroll {
                horizontal: 0,
                vertical: 1
            }));
            assert!(c.should_swallow(&delta(1, 1)));
        }
    }

    #[test]
    fn return_to_local_is_idempotent_and_uses_the_last_local_position() {
        let (c, rec) = standard();
        c.return_to_local(); // while local: no-op
        assert_eq!(rec.released.load(Ordering::SeqCst), 0);
        c.handle_motion(p(1511.0, 321.0), p(6.0, 0.0));
        c.return_to_local();
        c.return_to_local();
        assert_eq!(c.mode(), Mode::Local);
        assert_eq!(rec.released.load(Ordering::SeqCst), 1);
        assert_eq!(rec.left(), vec![p(1511.0, 321.0)]);
        assert!(!c.should_swallow(&abs(5, 5))); // a panic exit must not swallow a stray move
    }

    #[test]
    fn does_not_bounce_straight_back_after_entering() {
        let (c, rec) = standard();
        c.handle_motion(p(1511.0, 500.0), p(6.0, 0.0));
        // A jitter to the left right after crossing only holds the cursor at the edge.
        c.handle_motion(p(1511.0, 500.0), p(-1.0, 0.0));
        c.handle_motion(p(0.0, 0.0), p(9.0, 0.0)); // 9 px in: not far enough yet
        c.handle_motion(p(0.0, 0.0), p(-20.0, 0.0));
        assert_eq!(c.mode(), Mode::Remote);
        assert!(rec.left().is_empty());
        c.handle_motion(p(0.0, 0.0), p(10.0, 0.0)); // 10 px in: armed
        c.handle_motion(p(0.0, 0.0), p(-20.0, 0.0));
        assert_eq!(c.mode(), Mode::Local);
    }

    #[test]
    fn a_drag_started_on_the_other_computer_stays_there() {
        let (c, rec) = standard();
        c.handle_motion(p(1511.0, 500.0), p(6.0, 0.0));
        c.handle_motion(p(0.0, 0.0), p(30.0, 0.0));
        c.handle(&button(MouseButton::Left, true));
        c.handle_motion(p(0.0, 0.0), p(-60.0, 0.0)); // pushes through the facing edge, button held
        assert_eq!(c.mode(), Mode::Remote);
        assert!(rec.left().is_empty());
        assert!(rec.sent().contains(&button(MouseButton::Left, true)));
        // The cursor was held at the edge: only the 30 px that were left were applied.
        assert_eq!(rec.sent().last(), Some(&delta(-30, 0)));
        c.handle(&button(MouseButton::Left, false));
        c.handle_motion(p(0.0, 0.0), p(-5.0, 0.0));
        assert_eq!(c.mode(), Mode::Local);
    }

    #[test]
    fn crosses_again_after_returning() {
        let (c, rec) = standard();
        c.handle_motion(p(1511.0, 500.0), p(6.0, 0.0));
        c.return_to_local();
        c.handle_motion(p(1511.0, 700.0), p(6.0, 0.0));
        assert_eq!(c.mode(), Mode::Remote);
        assert_eq!(rec.entered.load(Ordering::SeqCst), 2);
        assert_eq!(rec.sent().last(), Some(&abs(0, 700)));
    }

    #[test]
    fn a_degenerate_remote_size_never_panics() {
        let recorder = Arc::new(Recorder::default());
        let c = SwitchController::new(vec![LAPTOP], Side::Right, 0.0, 0.0, false, recorder);
        c.handle_motion(p(1511.0, 500.0), p(6.0, 0.0));
        c.handle_motion(p(0.0, 0.0), p(3.0, 3.0));
        c.return_to_local();
    }

    #[test]
    fn no_displays_means_no_doorway() {
        let (c, _) = controller(vec![], Side::Right, false);
        c.handle_motion(p(10.0, 10.0), p(5.0, 0.0));
        assert_eq!(c.mode(), Mode::Local);
    }
}
