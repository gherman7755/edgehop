//! EdgeHop's Windows platform layer. The whole crate is empty on other platforms, so the workspace builds and
//! tests everywhere; on Windows it provides:
//!
//! * [`list_monitors`]: the displays, in the shape the protocol advertises them;
//! * [`WinOutput`]: [`edgehop_input::OsInput`] over `SendInput` (absolute moves without pointer acceleration,
//!   buttons, wheel), tagged with [`MARKER`];
//! * [`Capture`]: a low-level mouse hook plus raw input, on its own thread, that ignores events tagged with
//!   [`MARKER`] and can swallow the rest.
//!
//! Keyboard capture and injection wait for the neutral key map: key codes on the wire are macOS keycodes today.

#![cfg(windows)]

mod displays;
mod dpi;

pub use displays::list_monitors;
pub use dpi::ensure_dpi_aware;
