# Windows platform layer

What exists for Windows, how it is tested, and what is still missing. The Windows core is Rust
(`rust/crates/*`); this page covers the input layer: `edgehop-input` (shared logic) and `edgehop-win`
(Windows only).

## Crates

| Crate | Role |
|---|---|
| `edgehop-input` | Platform-independent injection logic behind a small `OsInput` trait: pointer tracking, small absolute jumps are ordinary moves and large ones warp (threshold 150 px), relative moves start from the real cursor, held buttons block the idle resync so drags do not jump, `release_all`. Same rules as the Swift `EventInjector`. Tested with a fake on any machine. |
| `edgehop-win` | `list_monitors` (displays, primary first), `WinOutput` (`OsInput` over `SendInput`), `Capture` (low-level mouse hook plus raw input on its own thread). The crate is empty on other platforms. |

## Design decisions

* **DPI aware.** The process opts into per-monitor DPI awareness (`ensure_dpi_aware`), otherwise Windows hands
  out scaled coordinates that do not match the pixels the other computer expects.
* **Absolute moves, no acceleration.** Moves use `SendInput` with `MOUSEEVENTF_ABSOLUTE | VIRTUALDESK`;
  relative `SendInput` moves are subject to pointer acceleration. The pixel to 0..65535 conversion is chosen so
  that Windows' own conversion back lands exactly on the pixel (the naive formula is off by one for some
  pixels; the unit test checks every pixel of several desktop widths).
* **Feedback guard.** Every injected event carries `dwExtraInfo = 0x45484F50` ("EHOP"), the same value the macOS
  core uses as event source user data; `Capture` ignores events carrying it and counts them.
* **Wheel.** The wire unit (1/120 of a notch) is Windows' `WHEEL_DELTA`, so scroll values pass straight through.
* **Capture.** The low-level hook sees every mouse event and can swallow it (return non-zero). Raw input adds
  unaccelerated device deltas, which keep changing while the cursor is pinned at a screen edge: that is how an
  outward push is recognised. Callbacks run inside the hook, which Windows abandons after about 300 ms, so they
  must be fast; a panic in a callback is caught so it can never unwind through the hook.
* **Hard-won lesson (link crate):** `shutdown` from another thread does not reliably wake a blocked `read` on
  Windows. Any thread that blocks on a socket must wake periodically and look at a flag.

## Testing

* Host (macOS or Linux): `cd rust && cargo test`, `cargo clippy --all-targets -- -D warnings`.
* Type-check for Windows from anywhere: `cargo check --target x86_64-pc-windows-msvc --all-targets` and
  `cargo clippy -p edgehop-win --target x86_64-pc-windows-msvc --all-targets -- -D warnings`.
* CI runs the Windows unit tests on a real Windows runner (displays, the coordinate round trip, the cursor
  following an injected move) and the self-test below (non-blocking: it needs an interactive desktop).
* On your Windows PC:
  ```
  cd rust
  cargo run -p edgehop-win --bin win-selftest           # injection + feedback guard, about a second
  cargo run -p edgehop-win --bin win-selftest -- --live # also prints the mouse events you generate for 10 s
  ```
  A pass prints `PASS`: the displays are listed, the cursor followed every injected move, a relative move on a
  fresh injector moved it by the expected 40 px, and every injected event was recognised as ours and none reached
  the capture callbacks (do not touch the mouse during the test). It never presses keys or buttons.

## Not done yet

* **Keyboard.** Key codes on the wire are macOS virtual keycodes; there is no neutral key map yet (milestone
  M7), so `WinOutput` ignores key events and there is no keyboard capture. Windows is mouse and scroll only.
* **Horizontal scroll sign.** Windows `WM_MOUSEHWHEEL` is positive to the right; the sign macOS puts on the wire
  has not been checked against real devices.
* **Sessions.** Joining the link (`edgehop-link`), the injector and the capture into receiver and controller
  sessions behind the core's IPC, including the Rust port of the edge switching, is the next step.
* **Remote mode on Windows.** While another computer is controlled, the local cursor has to stay put: the plan is
  to swallow moves in the hook and take the deltas from raw input (`Motion::raw_dx/raw_dy`).
