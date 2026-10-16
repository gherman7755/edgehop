//! EdgeHop's Windows platform layer. The whole crate is empty on other platforms, so the workspace builds and
//! tests everywhere; on Windows it provides:
//!
//! * [`list_monitors`]: the displays, in the shape the protocol advertises them;
//! * [`WinOutput`]: [`edgehop_input::OsInput`] over `SendInput` (absolute moves without pointer acceleration,
//!   buttons, wheel), tagged with [`MARKER`];
//! * [`Capture`]: a low-level mouse hook plus raw input, on its own thread, that ignores events tagged with
//!   [`MARKER`] and can swallow the rest.
//!
//! * [`WinPlatform`]: all of the above as an [`edgehop_input::Platform`], which the core's sessions run on.
//!
//! Keyboard capture and injection wait for the neutral key map: key codes on the wire are macOS keycodes today.

#![cfg(windows)]

mod capture;
mod displays;
mod dpi;
mod keyboard;
mod output;
mod platform;

pub use capture::{Capture, CaptureCallbacks, CaptureError, Motion};
pub use displays::list_monitors;
pub use dpi::ensure_dpi_aware;
pub use keyboard::{KeyCallback, KeySubscription};
pub use output::WinOutput;
pub use platform::WinPlatform;

/// Written to `dwExtraInfo` of every event we inject ("EHOP"); our own capture ignores events carrying it.
pub const MARKER: usize = 0x4548_4F50;
