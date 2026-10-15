use std::mem::size_of;

use edgehop_input::{OsInput, Rect};
use edgehop_proto::{Modifiers, MouseButton};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK,
    MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN,
};

use crate::dpi::ensure_dpi_aware;
use crate::MARKER;

const XBUTTON1: u32 = 1;
const XBUTTON2: u32 = 2;

/// `SendInput` based [`OsInput`]. Every event carries [`MARKER`] in `dwExtraInfo`.
///
/// Moves are *absolute* (`MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK`): relative `SendInput` moves go
/// through pointer acceleration, which would make injected motion differ from what the other computer sent.
#[derive(Debug, Default)]
pub struct WinOutput;

impl WinOutput {
    pub fn new() -> Self {
        ensure_dpi_aware();
        Self
    }
}

/// The virtual desktop: `(x, y, width, height)` in pixels.
fn virtual_desktop() -> (i32, i32, i32, i32) {
    // SAFETY: GetSystemMetrics takes a constant index and has no pointer arguments.
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}

/// Converts a pixel coordinate to the 0..=65535 range `SendInput` wants for absolute moves, chosen so that
/// Windows' own conversion back (`pixel = normalized * span >> 16`) lands exactly on `pixel`.
pub(crate) fn to_absolute(pixel: i32, origin: i32, span: i32) -> i32 {
    let span = i64::from(span.max(1));
    let offset = (i64::from(pixel) - i64::from(origin)).clamp(0, span - 1);
    ((offset * 65536 + span - 1) / span) as i32
}

fn send(flags: u32, dx: i32, dy: i32, mouse_data: u32) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: mouse_data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: MARKER,
            },
        },
    };
    // SAFETY: one fully initialised INPUT and its exact size, as SendInput requires.
    unsafe {
        SendInput(1, &input, size_of::<INPUT>() as i32);
    }
}

fn send_absolute_move(x: f64, y: f64) {
    let (vx, vy, vw, vh) = virtual_desktop();
    let nx = to_absolute(x.round() as i32, vx, vw);
    let ny = to_absolute(y.round() as i32, vy, vh);
    send(
        MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        nx,
        ny,
        0,
    );
}

impl OsInput for WinOutput {
    fn cursor_position(&mut self) -> Option<(f64, f64)> {
        let mut point = POINT { x: 0, y: 0 };
        // SAFETY: `point` is a valid, writable POINT.
        (unsafe { GetCursorPos(&mut point) } != 0).then(|| (f64::from(point.x), f64::from(point.y)))
    }

    fn desktop_bounds(&mut self) -> Rect {
        let (x, y, width, height) = virtual_desktop();
        Rect::new(
            f64::from(x),
            f64::from(y),
            f64::from(width),
            f64::from(height),
        )
    }

    fn warp_cursor(&mut self, x: f64, y: f64) {
        send_absolute_move(x, y);
    }

    fn move_cursor(&mut self, x: f64, y: f64, _dx: f64, _dy: f64) {
        send_absolute_move(x, y);
    }

    fn button(&mut self, button: MouseButton, is_down: bool, _x: f64, _y: f64) {
        let (flag, data) = match (button, is_down) {
            (MouseButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
            (MouseButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
            (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
            (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
            (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
            (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
            (MouseButton::Back, true) => (MOUSEEVENTF_XDOWN, XBUTTON1),
            (MouseButton::Back, false) => (MOUSEEVENTF_XUP, XBUTTON1),
            (MouseButton::Forward, true) => (MOUSEEVENTF_XDOWN, XBUTTON2),
            (MouseButton::Forward, false) => (MOUSEEVENTF_XUP, XBUTTON2),
        };
        send(flag, 0, 0, data);
    }

    fn scroll(&mut self, horizontal: i32, vertical: i32) {
        // The wire unit (1/120 of a notch) is Windows' own WHEEL_DELTA, so the values pass straight through.
        // The raw i32 bit pattern goes into the unsigned `mouseData`, as the API expects for signed deltas.
        if vertical != 0 {
            send(MOUSEEVENTF_WHEEL, 0, 0, vertical as u32);
        }
        if horizontal != 0 {
            send(MOUSEEVENTF_HWHEEL, 0, 0, horizontal as u32);
        }
    }

    fn key(&mut self, _code: u16, _is_down: bool, _modifiers: Modifiers) {
        // Key codes on the wire are macOS keycodes until the neutral key map exists: not translated yet.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows' conversion from the normalised value back to a pixel.
    fn back_to_pixel(normalized: i32, span: i32) -> i32 {
        ((i64::from(normalized) * i64::from(span)) >> 16) as i32
    }

    #[test]
    fn absolute_coordinates_round_trip_exactly_for_every_pixel() {
        for span in [800, 1366, 1512, 1920, 2560, 3840, 7680] {
            for pixel in 0..span {
                let normalized = to_absolute(pixel, 0, span);
                assert!(
                    (0..=65535).contains(&normalized),
                    "{normalized} out of range for pixel {pixel} of {span}"
                );
                assert_eq!(
                    back_to_pixel(normalized, span),
                    pixel,
                    "pixel {pixel} of {span}"
                );
            }
        }
    }

    #[test]
    fn a_negative_origin_is_handled() {
        // A monitor to the left of the primary: the virtual desktop starts at -1920.
        assert_eq!(back_to_pixel(to_absolute(-1920, -1920, 3840), 3840), 0);
        assert_eq!(back_to_pixel(to_absolute(0, -1920, 3840), 3840), 1920);
        assert_eq!(back_to_pixel(to_absolute(1919, -1920, 3840), 3840), 3839);
    }

    #[test]
    fn out_of_range_pixels_are_clamped_to_the_desktop() {
        assert_eq!(back_to_pixel(to_absolute(-50, 0, 1920), 1920), 0);
        assert_eq!(back_to_pixel(to_absolute(99999, 0, 1920), 1920), 1919);
    }

    #[test]
    fn the_cursor_follows_an_injected_absolute_move() {
        let mut output = WinOutput::new();
        let bounds = output.desktop_bounds();
        let target = (bounds.x + 120.0, bounds.y + 90.0);
        output.warp_cursor(target.0, target.1);
        std::thread::sleep(std::time::Duration::from_millis(100));
        let actual = output.cursor_position().expect("cursor position");
        assert!(
            (actual.0 - target.0).abs() <= 1.0 && (actual.1 - target.1).abs() <= 1.0,
            "asked for {target:?}, the cursor is at {actual:?}"
        );
    }
}
