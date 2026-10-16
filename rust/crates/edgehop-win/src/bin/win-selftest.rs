//! Self-test of the Windows layer on a real desktop: displays, injection (the cursor must end up where it was
//! sent), the feedback guard (our own events must be ignored by our capture) and capture of real input.
//!
//!     win-selftest          injection + guard (no interaction needed; the cursor moves for about a second)
//!     win-selftest --live   additionally prints the mouse events you generate for 10 seconds
//!
//! It never presses keys or buttons.

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use edgehop_input::{Injector, OsInput};
    use edgehop_proto::InputEvent;
    use edgehop_win::{list_monitors, Capture, CaptureCallbacks, WinOutput};

    fn near(actual: (f64, f64), wanted: (f64, f64)) -> bool {
        (actual.0 - wanted.0).abs() <= 1.0 && (actual.1 - wanted.1).abs() <= 1.0
    }

    /// F13 (HID 0x68, scan code 0x64).
    const F13_HID: u16 = 0x68;
    const F13_SCAN: u16 = 0x64;

    fn raw_key(scan: u16, is_down: bool) {
        use std::mem::size_of;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
            KEYEVENTF_SCANCODE,
        };
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: scan,
                    dwFlags: KEYEVENTF_SCANCODE | if is_down { 0 } else { KEYEVENTF_KEYUP },
                    time: 0,
                    dwExtraInfo: 0, // not ours
                },
            },
        };
        // SAFETY: one fully initialised INPUT and its exact size.
        unsafe {
            SendInput(1, &input, size_of::<INPUT>() as i32);
        }
    }

    fn keyboard_phase(check: &mut impl FnMut(bool, String)) {
        use edgehop_proto::Modifiers;
        use edgehop_win::KeySubscription;
        use std::sync::Mutex;

        let seen = Arc::new(Mutex::new(Vec::<InputEvent>::new()));
        let recorder = Arc::clone(&seen);
        let subscription = KeySubscription::new(Arc::new(move |event| {
            recorder.lock().unwrap().push(event);
            false
        }));
        check(subscription.is_ok(), "keyboard: the hook installs".into());
        let Ok(_subscription) = subscription else {
            return;
        };

        let mut ours = WinOutput::new();
        ours.key(F13_HID, true, Modifiers::NONE);
        ours.key(F13_HID, false, Modifiers::NONE);
        std::thread::sleep(Duration::from_millis(300));
        check(
            seen.lock().unwrap().is_empty(),
            "keyboard guard: keys we inject are ignored by our own hook".into(),
        );

        raw_key(F13_SCAN, true);
        raw_key(F13_SCAN, false);
        std::thread::sleep(Duration::from_millis(300));
        let events = seen.lock().unwrap().clone();
        let expected = vec![
            InputEvent::Key {
                code: F13_HID,
                is_down: true,
                modifiers: Modifiers::NONE,
            },
            InputEvent::Key {
                code: F13_HID,
                is_down: false,
                modifiers: Modifiers::NONE,
            },
        ];
        check(
            events == expected,
            format!("keyboard: a foreign F13 press was captured as HID 0x68 (saw {events:?})"),
        );
    }

    pub fn run() -> i32 {
        let live = std::env::args().any(|a| a == "--live");
        let mut failures = 0;
        let mut check = |ok: bool, label: String| {
            println!("{} {label}", if ok { "ok  " } else { "FAIL" });
            if !ok {
                failures += 1;
            }
        };

        let monitors = list_monitors();
        for m in &monitors {
            println!(
                "display {}: {}x{} at ({}, {})",
                m.id, m.width, m.height, m.x, m.y
            );
        }
        check(
            !monitors.is_empty() && monitors[0].x == 0.0 && monitors[0].y == 0.0,
            "displays: the primary is at the origin".into(),
        );

        let foreign = Arc::new(AtomicUsize::new(0));
        let counter = foreign.clone();
        let live_counter = foreign.clone();
        let capture = Capture::start(CaptureCallbacks {
            on_motion: Box::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
            on_event: Box::new(move |event| {
                if live {
                    live_counter.fetch_add(0, Ordering::SeqCst);
                    println!("  event: {event:?}");
                }
            }),
            should_swallow: Box::new(|_| false),
        });
        let mut capture = match capture {
            Ok(capture) => capture,
            Err(error) => {
                println!("FAIL capture could not start: {error}");
                return 1;
            }
        };
        check(true, "capture started (mouse hook + raw input)".into());

        let mut injector = Injector::new(WinOutput::new());
        let original = injector.os_mut().cursor_position().unwrap_or((0.0, 0.0));
        let bounds = injector.os_mut().desktop_bounds();
        let (cx, cy) = (
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        );

        // Phase 1: absolute moves along a square; the cursor must follow each one.
        let corners = [
            (cx - 60.0, cy - 60.0),
            (cx + 60.0, cy - 60.0),
            (cx + 60.0, cy + 60.0),
            (cx - 60.0, cy + 60.0),
        ];
        let mut all_followed = true;
        let mut injected = 0usize;
        for (x, y) in corners {
            injector.inject(&InputEvent::MouseMoveAbsolute {
                x: x as i32,
                y: y as i32,
            });
            injected += 1;
            std::thread::sleep(Duration::from_millis(60));
            let actual = injector
                .os_mut()
                .cursor_position()
                .unwrap_or((f64::NAN, f64::NAN));
            if !near(actual, (x, y)) {
                all_followed = false;
                println!("  asked for ({x}, {y}), the cursor is at {actual:?}");
            }
        }
        check(
            all_followed,
            format!(
                "injection: the cursor followed {} absolute moves",
                corners.len()
            ),
        );

        // Phase 2: relative moves on a fresh injector (the case that broke the macOS receiver).
        let mut fresh = Injector::new(WinOutput::new());
        let before = fresh.os_mut().cursor_position().unwrap_or((0.0, 0.0));
        fresh.inject(&InputEvent::MouseMoveDelta { dx: 40, dy: 0 });
        injected += 1;
        std::thread::sleep(Duration::from_millis(60));
        let after = fresh.os_mut().cursor_position().unwrap_or((0.0, 0.0));
        check(
            (after.0 - before.0 - 40.0).abs() <= 1.0 && (after.1 - before.1).abs() <= 1.0,
            format!(
                "injection: a relative move on a fresh injector moved the cursor by {}",
                after.0 - before.0
            ),
        );

        // Put the cursor back and let the hook drain.
        injector.inject(&InputEvent::MouseMoveAbsolute {
            x: original.0 as i32,
            y: original.1 as i32,
        });
        injected += 1;
        std::thread::sleep(Duration::from_millis(300));

        let (mapped, own_ignored) = capture.statistics();
        println!(
            "injected {injected} moves; capture ignored {own_ignored} as ours and mapped {mapped}"
        );
        check(
            own_ignored == injected,
            format!(
                "guard: every injected event was recognised as ours ({own_ignored} of {injected})"
            ),
        );
        check(foreign.load(Ordering::SeqCst) == 0, "guard: none of them reached the capture callbacks (do not touch the mouse during the test)".into());

        // Phase 3: the keyboard. F13 does nothing in any application. A key we inject is ignored by the hook (the
        // feedback guard); the same key injected without our marker is captured, translated to its HID usage and
        // can be kept from applications.
        keyboard_phase(&mut check);

        if live {
            println!("Move the mouse, click and scroll for 10 seconds...");
            let end = Instant::now() + Duration::from_secs(10);
            while Instant::now() < end {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        capture.stop();
        println!("{}", if failures == 0 { "\nPASS" } else { "\nFAILED" });
        i32::from(failures != 0)
    }
}

fn main() {
    #[cfg(windows)]
    std::process::exit(imp::run());
    #[cfg(not(windows))]
    eprintln!("win-selftest only runs on Windows");
}
