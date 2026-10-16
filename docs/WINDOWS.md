# Windows platform layer

What exists for Windows, how it is tested, and what is still missing. The Windows core is Rust
(`rust/crates/*`); this page covers the input layer: `edgehop-input` (shared logic) and `edgehop-win`
(Windows only).

## Crates

| Crate | Role |
|---|---|
| `edgehop-input` | Also the `Platform` trait the core's sessions run on (displays, an injector, capture) and `MotionMapper`. Platform-independent injection logic behind a small `OsInput` trait: pointer tracking, small absolute jumps are ordinary moves and large ones warp (threshold 150 px), relative moves start from the real cursor, held buttons block the idle resync so drags do not jump, `release_all`. Same rules as the Swift `EventInjector`. Tested with a fake on any machine. |
| `edgehop-win` | `list_monitors` (displays, primary first), `WinOutput` (`OsInput` over `SendInput`), `Capture` (low-level mouse hook plus raw input on its own thread), `WinPlatform` (all of it as a `Platform`). The crate is empty on other platforms. |

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
* **Discovery.** The Rust core has no mDNS yet: a controller connects by address (`host`), not by computer
  name.
* **Panic chord.** The Swift core stops a session when Ctrl+Option+Cmd+Esc is typed locally. Windows has no
  keyboard capture yet, so the way out is the UI's stop button (or closing the UI, which stops everything).
* **Not yet verified on real hardware:** the pinned cursor in remote mode (below). Unit tests and the loopback
  CI run cover the logic, not the feel.

## Sessions and the pinned cursor

`SessionBackend<WinPlatform>` (in `edgehop-core`) runs the receiver (`LinkServer` + `Injector`) and the controller
(`LinkClient` + `SwitchController` + `Capture`); `docs/IPC.md` describes what the UI sees.

While another computer is controlled, the local cursor must not move, but the hook still has to measure the mouse.
When control leaves, the cursor is parked at the centre of its display, the hook swallows every move, and the
movement is `pt - centre`: the hook reports where a swallowed move would have gone, and from the centre nothing is
clamped at a screen edge. Locally the movement is the raw (unaccelerated) device delta, which still changes when the
cursor is stopped at an edge, so an outward push is recognised. This is `MotionMapper`. On return the cursor is
warped to the crossing point. The first move or two after parking may be measured against the old position; if that
shows as a jump on real hardware, it needs a guard.

## Edge switching rules (Swift and Rust, identical)

* Enter through the outer edge of a display on the chosen side, with a 2 px band; the entry keeps its offset along
  the edge. Return through the facing edge of the other computer.
* **No switching while a button is held**, in either direction: a drag stays on the computer it started on (the
  cursor is held at the edge until the button is released).
* **No bounce:** after entering, a push back out only counts once the cursor has moved 10 px away from the
  entry edge, so a one-pixel jitter right after crossing does not send you straight back.
* Stopping, a lost link or shutdown always return control, whatever is held.
