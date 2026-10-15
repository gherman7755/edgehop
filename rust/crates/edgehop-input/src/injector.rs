use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use edgehop_proto::{InputEvent, Modifiers, MouseButton};

use crate::os::OsInput;
use crate::pointer::PointerState;

/// Absolute jumps up to this many pixels are replayed as ordinary moves; larger ones warp the cursor.
pub const DEFAULT_WARP_THRESHOLD: f64 = 150.0;
/// After this long without input the tracked position is forgotten and re-read from the real cursor, so a
/// locally moved mouse is respected.
pub const DEFAULT_IDLE_RESYNC: Duration = Duration::from_millis(500);

/// Replays [`InputEvent`]s on this computer through an [`OsInput`].
pub struct Injector<O: OsInput> {
    os: O,
    pointer: PointerState,
    last_injection: Option<Instant>,
    held_buttons: Vec<MouseButton>,
    held_keys: BTreeSet<u16>,
    pub warp_threshold: f64,
    pub idle_resync: Duration,
}

impl<O: OsInput> Injector<O> {
    pub fn new(os: O) -> Self {
        Self {
            os,
            pointer: PointerState::new(),
            last_injection: None,
            held_buttons: Vec::new(),
            held_keys: BTreeSet::new(),
            warp_threshold: DEFAULT_WARP_THRESHOLD,
            idle_resync: DEFAULT_IDLE_RESYNC,
        }
    }

    pub fn os(&self) -> &O {
        &self.os
    }

    pub fn os_mut(&mut self) -> &mut O {
        &mut self.os
    }

    pub fn inject(&mut self, event: &InputEvent) {
        self.inject_at(event, Instant::now());
    }

    /// Same as [`Self::inject`] with an explicit clock, for tests.
    pub fn inject_at(&mut self, event: &InputEvent, now: Instant) {
        self.resync_if_idle(now);
        match *event {
            InputEvent::MouseMoveAbsolute { x, y } => {
                self.move_absolute((f64::from(x), f64::from(y)))
            }
            InputEvent::MouseMoveDelta { dx, dy } => {
                self.move_relative(f64::from(dx), f64::from(dy))
            }
            InputEvent::MouseButton { button, is_down } => self.press_button(button, is_down),
            InputEvent::Scroll {
                horizontal,
                vertical,
            } => self.os.scroll(horizontal, vertical),
            InputEvent::Key {
                code,
                is_down,
                modifiers,
            } => self.press_key(code, is_down, modifiers),
        }
    }

    /// Releases every key and button pressed through this injector (sent when control returns to the other
    /// computer, and when the link drops).
    pub fn release_all(&mut self) {
        let buttons = std::mem::take(&mut self.held_buttons);
        for button in buttons {
            let (x, y) = self.position();
            self.os.button(button, false, x, y);
        }
        let keys = std::mem::take(&mut self.held_keys);
        for code in keys {
            self.os.key(code, false, Modifiers::NONE);
        }
    }

    fn resync_if_idle(&mut self, now: Instant) {
        if let Some(last) = self.last_injection {
            if now.saturating_duration_since(last) > self.idle_resync
                && self.held_buttons.is_empty()
            {
                self.pointer.reset();
            }
        }
        self.last_injection = Some(now);
    }

    fn position(&mut self) -> (f64, f64) {
        let os = &mut self.os;
        self.pointer.current(|| os.cursor_position())
    }

    fn move_absolute(&mut self, target: (f64, f64)) {
        if let Some((x, y)) = self.pointer.position() {
            let (dx, dy) = (target.0 - x, target.1 - y);
            if dx.hypot(dy) <= self.warp_threshold {
                self.os.move_cursor(target.0, target.1, dx, dy);
                self.pointer.set(target);
                return;
            }
        }
        self.os.warp_cursor(target.0, target.1);
        self.pointer.set(target);
    }

    fn move_relative(&mut self, dx: f64, dy: f64) {
        let bounds = self.os.desktop_bounds();
        let os = &mut self.os;
        let (x, y) = self.pointer.moved(dx, dy, || os.cursor_position(), &bounds);
        self.os.move_cursor(x, y, dx, dy);
    }

    fn press_button(&mut self, button: MouseButton, is_down: bool) {
        let (x, y) = self.position();
        if is_down {
            if !self.held_buttons.contains(&button) {
                self.held_buttons.push(button);
            }
        } else {
            self.held_buttons.retain(|held| *held != button);
        }
        self.os.button(button, is_down, x, y);
    }

    fn press_key(&mut self, code: u16, is_down: bool, modifiers: Modifiers) {
        if is_down {
            self.held_keys.insert(code);
        } else {
            self.held_keys.remove(&code);
        }
        self.os.key(code, is_down, modifiers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pointer::Rect;

    #[derive(Debug, Clone, PartialEq)]
    enum Call {
        Warp(f64, f64),
        Move {
            x: f64,
            y: f64,
            dx: f64,
            dy: f64,
        },
        Button {
            button: MouseButton,
            down: bool,
            x: f64,
            y: f64,
        },
        Scroll(i32, i32),
        Key {
            code: u16,
            down: bool,
            modifiers: Modifiers,
        },
    }

    struct FakeOs {
        cursor: Option<(f64, f64)>,
        bounds: Rect,
        cursor_reads: usize,
        calls: Vec<Call>,
    }

    impl FakeOs {
        fn new(cursor: Option<(f64, f64)>) -> Self {
            Self {
                cursor,
                bounds: Rect::new(0.0, 0.0, 1000.0, 800.0),
                cursor_reads: 0,
                calls: Vec::new(),
            }
        }
    }

    impl OsInput for FakeOs {
        fn cursor_position(&mut self) -> Option<(f64, f64)> {
            self.cursor_reads += 1;
            self.cursor
        }
        fn desktop_bounds(&mut self) -> Rect {
            self.bounds
        }
        fn warp_cursor(&mut self, x: f64, y: f64) {
            self.calls.push(Call::Warp(x, y));
        }
        fn move_cursor(&mut self, x: f64, y: f64, dx: f64, dy: f64) {
            self.calls.push(Call::Move { x, y, dx, dy });
        }
        fn button(&mut self, button: MouseButton, is_down: bool, x: f64, y: f64) {
            self.calls.push(Call::Button {
                button,
                down: is_down,
                x,
                y,
            });
        }
        fn scroll(&mut self, horizontal: i32, vertical: i32) {
            self.calls.push(Call::Scroll(horizontal, vertical));
        }
        fn key(&mut self, code: u16, is_down: bool, modifiers: Modifiers) {
            self.calls.push(Call::Key {
                code,
                down: is_down,
                modifiers,
            });
        }
    }

    fn injector(cursor: Option<(f64, f64)>) -> Injector<FakeOs> {
        Injector::new(FakeOs::new(cursor))
    }

    fn abs(x: i32, y: i32) -> InputEvent {
        InputEvent::MouseMoveAbsolute { x, y }
    }

    fn delta(dx: i32, dy: i32) -> InputEvent {
        InputEvent::MouseMoveDelta { dx, dy }
    }

    fn click(button: MouseButton, is_down: bool) -> InputEvent {
        InputEvent::MouseButton { button, is_down }
    }

    #[test]
    fn a_relative_move_works_before_any_absolute_move() {
        let mut injector = injector(Some((500.0, 400.0)));
        injector.inject(&delta(30, -10));
        assert_eq!(
            injector.os().calls,
            vec![Call::Move {
                x: 530.0,
                y: 390.0,
                dx: 30.0,
                dy: -10.0
            }]
        );
    }

    #[test]
    fn the_real_cursor_is_read_only_once_while_active() {
        let mut injector = injector(Some((100.0, 100.0)));
        let now = Instant::now();
        for i in 0..5u32 {
            injector.inject_at(&delta(1, 0), now + Duration::from_millis(u64::from(i)));
        }
        assert_eq!(injector.os().cursor_reads, 1);
        assert_eq!(
            injector.os().calls.last(),
            Some(&Call::Move {
                x: 105.0,
                y: 100.0,
                dx: 1.0,
                dy: 0.0
            })
        );
    }

    #[test]
    fn relative_moves_stop_at_the_last_pixel_of_the_desktop() {
        let mut injector = injector(Some((990.0, 790.0)));
        injector.inject(&delta(500, 500));
        assert_eq!(
            injector.os().calls,
            vec![Call::Move {
                x: 999.0,
                y: 799.0,
                dx: 500.0,
                dy: 500.0
            }]
        );
    }

    #[test]
    fn the_first_absolute_move_warps() {
        let mut injector = injector(Some((10.0, 10.0)));
        injector.inject(&abs(400, 300));
        assert_eq!(injector.os().calls, vec![Call::Warp(400.0, 300.0)]);
    }

    #[test]
    fn small_absolute_jumps_are_moves_and_large_ones_warp() {
        let mut injector = injector(None);
        injector.inject(&abs(100, 100)); // warp, now tracked
        injector.inject(&abs(250, 100)); // exactly 150 px: still a move
        injector.inject(&abs(250, 251)); // 151 px: warp
        assert_eq!(
            injector.os().calls,
            vec![
                Call::Warp(100.0, 100.0),
                Call::Move {
                    x: 250.0,
                    y: 100.0,
                    dx: 150.0,
                    dy: 0.0
                },
                Call::Warp(250.0, 251.0),
            ]
        );
    }

    #[test]
    fn clicks_use_the_tracked_position_or_the_real_cursor_never_the_origin() {
        let mut injector = injector(Some((640.0, 360.0)));
        injector.inject(&click(MouseButton::Right, true));
        injector.inject(&abs(700, 360));
        injector.inject(&click(MouseButton::Right, false));
        assert_eq!(
            injector.os().calls,
            vec![
                Call::Button {
                    button: MouseButton::Right,
                    down: true,
                    x: 640.0,
                    y: 360.0
                },
                Call::Move {
                    x: 700.0,
                    y: 360.0,
                    dx: 60.0,
                    dy: 0.0
                },
                Call::Button {
                    button: MouseButton::Right,
                    down: false,
                    x: 700.0,
                    y: 360.0
                },
            ]
        );
    }

    #[test]
    fn after_being_idle_the_position_is_re_read_from_the_real_cursor() {
        let mut injector = injector(Some((100.0, 100.0)));
        let start = Instant::now();
        injector.inject_at(&delta(10, 0), start);
        injector.os_mut().cursor = Some((800.0, 600.0)); // the user moved the mouse locally
        injector.inject_at(&delta(10, 0), start + Duration::from_millis(100)); // still active: tracked position
        injector.inject_at(&delta(10, 0), start + Duration::from_secs(2)); // idle: re-read
        let calls = &injector.os().calls;
        assert_eq!(
            calls[1],
            Call::Move {
                x: 120.0,
                y: 100.0,
                dx: 10.0,
                dy: 0.0
            }
        );
        assert_eq!(
            calls[2],
            Call::Move {
                x: 810.0,
                y: 600.0,
                dx: 10.0,
                dy: 0.0
            }
        );
    }

    #[test]
    fn a_held_button_prevents_the_idle_resync() {
        let mut injector = injector(Some((100.0, 100.0)));
        let start = Instant::now();
        injector.inject_at(&click(MouseButton::Left, true), start);
        injector.os_mut().cursor = Some((800.0, 600.0));
        injector.inject_at(&delta(10, 0), start + Duration::from_secs(5)); // a drag must not jump
        assert_eq!(
            injector.os().calls[1],
            Call::Move {
                x: 110.0,
                y: 100.0,
                dx: 10.0,
                dy: 0.0
            }
        );
    }

    #[test]
    fn release_all_lets_go_of_every_button_and_key_exactly_once() {
        let mut injector = injector(Some((50.0, 60.0)));
        injector.inject(&click(MouseButton::Left, true));
        injector.inject(&click(MouseButton::Left, true)); // a repeated down is not held twice
        injector.inject(&click(MouseButton::Middle, true));
        injector.inject(&InputEvent::Key {
            code: 56,
            is_down: true,
            modifiers: Modifiers::SHIFT,
        });
        injector.inject(&InputEvent::Key {
            code: 0,
            is_down: true,
            modifiers: Modifiers::NONE,
        });
        injector.os_mut().calls.clear();

        injector.release_all();
        assert_eq!(
            injector.os().calls,
            vec![
                Call::Button {
                    button: MouseButton::Left,
                    down: false,
                    x: 50.0,
                    y: 60.0
                },
                Call::Button {
                    button: MouseButton::Middle,
                    down: false,
                    x: 50.0,
                    y: 60.0
                },
                Call::Key {
                    code: 0,
                    down: false,
                    modifiers: Modifiers::NONE
                },
                Call::Key {
                    code: 56,
                    down: false,
                    modifiers: Modifiers::NONE
                },
            ]
        );
        injector.os_mut().calls.clear();
        injector.release_all();
        assert!(injector.os().calls.is_empty(), "nothing left to release");
    }

    #[test]
    fn released_buttons_and_keys_are_no_longer_held() {
        let mut injector = injector(Some((1.0, 1.0)));
        injector.inject(&click(MouseButton::Left, true));
        injector.inject(&click(MouseButton::Left, false));
        injector.inject(&InputEvent::Key {
            code: 7,
            is_down: true,
            modifiers: Modifiers::NONE,
        });
        injector.inject(&InputEvent::Key {
            code: 7,
            is_down: false,
            modifiers: Modifiers::NONE,
        });
        injector.os_mut().calls.clear();
        injector.release_all();
        assert!(injector.os().calls.is_empty());
    }

    #[test]
    fn scroll_and_keys_pass_straight_through() {
        let mut injector = injector(None);
        injector.inject(&InputEvent::Scroll {
            horizontal: -120,
            vertical: 360,
        });
        injector.inject(&InputEvent::Key {
            code: 53,
            is_down: true,
            modifiers: Modifiers::COMMAND | Modifiers::SHIFT,
        });
        assert_eq!(
            injector.os().calls,
            vec![
                Call::Scroll(-120, 360),
                Call::Key {
                    code: 53,
                    down: true,
                    modifiers: Modifiers::COMMAND | Modifiers::SHIFT
                },
            ]
        );
    }
}
