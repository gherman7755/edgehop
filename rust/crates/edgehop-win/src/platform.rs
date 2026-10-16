//! [`Platform`] for Windows: what the core's sessions run on.
//!
//! Capture pins the cursor while control is on the other computer. The low-level hook can swallow a move, but
//! then the cursor stays where it is and the hook sees where each move *would* have gone, so the cursor is parked
//! at the centre of its display (far from any edge, where Windows would clamp it) and the movement is that
//! position minus the centre; see [`edgehop_input::MotionMapper`].

use std::sync::{Arc, Mutex, PoisonError};

use edgehop_input::{
    CaptureHandle, CaptureSink, MotionMapper, PanicGuard, Platform, PlatformError,
};
use edgehop_proto::{is_panic_chord, MonitorInfo};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};

use crate::capture::{Capture, CaptureCallbacks};
use crate::displays::list_monitors;
use crate::dpi::ensure_dpi_aware;
use crate::keyboard::KeySubscription;
use crate::output::WinOutput;

#[derive(Debug, Default)]
pub struct WinPlatform;

impl WinPlatform {
    pub fn new() -> Self {
        ensure_dpi_aware();
        Self
    }
}

impl Platform for WinPlatform {
    type Output = WinOutput;

    fn os_name(&self) -> &'static str {
        "windows"
    }

    fn device_name(&self) -> String {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "this computer".to_string())
    }

    fn input_permitted(&self) -> bool {
        true // Windows has no per-application input permissions
    }

    fn displays(&self) -> Vec<MonitorInfo> {
        list_monitors()
    }

    fn output(&self) -> WinOutput {
        WinOutput::new()
    }

    fn start_capture(
        &self,
        sink: Arc<dyn CaptureSink>,
    ) -> Result<Arc<dyn CaptureHandle>, PlatformError> {
        let mapper = Arc::new(MotionMapper::new());
        let motion_mapper = Arc::clone(&mapper);
        let motion_sink = Arc::clone(&sink);
        let event_sink = Arc::clone(&sink);
        let swallow_sink = Arc::clone(&sink);
        let key_sink = sink;
        let capture = Capture::start(CaptureCallbacks {
            on_motion: Box::new(move |motion| {
                let (position, delta) =
                    motion_mapper.map(motion.x, motion.y, motion.raw_dx, motion.raw_dy);
                motion_sink.motion(position, delta);
            }),
            on_event: Box::new(move |event| event_sink.event(event)),
            should_swallow: Box::new(move |event| swallow_sink.should_swallow(event)),
        })
        .map_err(|error| PlatformError(error.to_string()))?;
        // Keys are always watched while controlling, even when they are not shared: the panic chord must work.
        let keys = KeySubscription::new(Arc::new(move |event| {
            key_sink.event(event);
            key_sink.should_swallow(&event)
        }))
        .map_err(|error| PlatformError(error.to_string()))?;
        Ok(Arc::new(WinCapture {
            capture: Mutex::new(Some(capture)),
            keys: Mutex::new(Some(keys)),
            mapper,
        }))
    }

    fn watch_panic(
        &self,
        on_panic: Box<dyn Fn() + Send + Sync>,
    ) -> Result<Box<dyn PanicGuard>, PlatformError> {
        let subscription = KeySubscription::new(Arc::new(move |event| {
            if is_panic_chord(&event) {
                on_panic();
            }
            false // the receiver never keeps keys from the person sitting at it
        }))
        .map_err(|error| PlatformError(error.to_string()))?;
        Ok(Box::new(WinPanicGuard(subscription)))
    }
}

struct WinPanicGuard(#[allow(dead_code)] KeySubscription);

impl PanicGuard for WinPanicGuard {}

struct WinCapture {
    capture: Mutex<Option<Capture>>,
    keys: Mutex<Option<KeySubscription>>,
    mapper: Arc<MotionMapper>,
}

/// The centre of the display the cursor is on, falling back to the primary one.
fn pin_point() -> (f64, f64) {
    let mut cursor = POINT { x: 0, y: 0 };
    // SAFETY: `cursor` is a valid POINT for the call.
    let known = unsafe { GetCursorPos(&mut cursor) } != 0;
    let monitors = list_monitors();
    let on = monitors.iter().find(|m| {
        known
            && f64::from(cursor.x) >= m.x
            && f64::from(cursor.x) < m.x + m.width
            && f64::from(cursor.y) >= m.y
            && f64::from(cursor.y) < m.y + m.height
    });
    match on.or(monitors.first()) {
        Some(m) => (
            (m.x + m.width / 2.0).floor(),
            (m.y + m.height / 2.0).floor(),
        ),
        None => (0.0, 0.0),
    }
}

fn set_cursor(x: f64, y: f64) {
    // SAFETY: plain FFI call with integer coordinates.
    unsafe {
        SetCursorPos(x.round() as i32, y.round() as i32);
    }
}

impl CaptureHandle for WinCapture {
    fn enter_remote(&self) {
        let pin = pin_point();
        // The pin first: every move that arrives after the warp is measured from it.
        self.mapper.enter_remote(pin);
        set_cursor(pin.0, pin.1);
    }

    fn leave_remote(&self, to: (f64, f64)) {
        self.mapper.leave_remote();
        set_cursor(to.0, to.1);
    }

    fn cursor_position(&self) -> Option<(f64, f64)> {
        let mut cursor = POINT { x: 0, y: 0 };
        // SAFETY: `cursor` is a valid POINT for the call.
        let known = unsafe { GetCursorPos(&mut cursor) } != 0;
        known.then(|| (f64::from(cursor.x), f64::from(cursor.y)))
    }

    fn stop(&self) {
        let capture = self
            .capture
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let keys = self
            .keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        self.mapper.leave_remote();
        // Both join their hook thread: never hold a lock the hook callbacks need.
        drop(capture);
        drop(keys);
    }
}
