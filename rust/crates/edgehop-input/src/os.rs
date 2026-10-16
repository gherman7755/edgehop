use edgehop_proto::{Modifiers, MouseButton};

use crate::pointer::Rect;

/// What the injector needs from the operating system. Positions are global desktop coordinates (the primary
/// display's top-left is the origin, y grows downwards).
///
/// Everything injected through this trait must be tagged so the platform's own capture can recognise and ignore
/// it (see the `MARKER` of each platform crate), otherwise injected input would be captured and forwarded again.
pub trait OsInput {
    /// The real cursor position, if it can be read.
    fn cursor_position(&mut self) -> Option<(f64, f64)>;

    /// The rectangle covering every display.
    fn desktop_bounds(&mut self) -> Rect;

    /// Jump the cursor to `(x, y)`. Applications see one move event.
    fn warp_cursor(&mut self, x: f64, y: f64);

    /// Move the cursor to `(x, y)`, which is `(dx, dy)` away from where it was, as an ordinary mouse move, so
    /// applications see hover and drag. Implementations must not apply pointer acceleration to it.
    fn move_cursor(&mut self, x: f64, y: f64, dx: f64, dy: f64);

    /// Press or release a button with the cursor at `(x, y)`.
    fn button(&mut self, button: MouseButton, is_down: bool, x: f64, y: f64);

    /// Scroll by `horizontal` / `vertical` in 1/120 of a wheel notch (positive vertical = up).
    fn scroll(&mut self, horizontal: i32, vertical: i32);

    /// Press or release a key. `code` is the USB HID usage of the physical key (`docs/keymap.csv`); a platform
    /// translates it to its own codes and ignores keys it has no equivalent for.
    fn key(&mut self, code: u16, is_down: bool, modifiers: Modifiers);
}
