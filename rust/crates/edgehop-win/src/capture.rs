//! Mouse capture: a low-level mouse hook (sees every event, can swallow it) and raw input (unaccelerated device
//! deltas that keep flowing while the cursor is pinned at a screen edge). Both live on one dedicated thread
//! with a message loop.
//!
//! Callbacks run inside the hook, which Windows abandons if it takes longer than about 300 ms: keep them quick.

use std::mem::{size_of, zeroed};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use edgehop_proto::{InputEvent, MouseButton};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEMOUSE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    PostThreadMessageW, RegisterClassW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    HHOOK, HWND_MESSAGE, MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_INPUT, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSW,
};

use crate::dpi::ensure_dpi_aware;
use crate::MARKER;

const MOUSE_MOVE_ABSOLUTE: u16 = 1;

/// Why capture could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureError(pub String);

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CaptureError {}

/// One mouse move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Motion {
    /// Where the cursor is (or, if the move is being swallowed, where it would have gone), in pixels.
    pub x: i32,
    pub y: i32,
    /// Unaccelerated device movement since the previous `Motion` (raw input). Unlike the position it keeps
    /// changing while the cursor is pinned at a screen edge, which is how an outward push is recognised.
    pub raw_dx: i32,
    pub raw_dy: i32,
}

/// What capture reports. All three run inside the hook: keep them fast and never block.
pub struct CaptureCallbacks {
    /// Every mouse move.
    pub on_motion: Box<dyn Fn(Motion) + Send + Sync>,
    /// Every mapped event (moves as absolute positions, buttons, wheel).
    pub on_event: Box<dyn Fn(InputEvent) + Send + Sync>,
    /// True if the event must not reach applications (it is being forwarded to another computer).
    pub should_swallow: Box<dyn Fn(&InputEvent) -> bool + Send + Sync>,
}

struct State {
    callbacks: CaptureCallbacks,
    raw_dx: AtomicI32,
    raw_dy: AtomicI32,
    mapped: AtomicUsize,
    own_ignored: AtomicUsize,
}

/// The hook procedure and the window procedure are plain functions: they find their state here.
static STATE: Mutex<Option<Arc<State>>> = Mutex::new(None);

fn state_lock() -> MutexGuard<'static, Option<Arc<State>>> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn current_state() -> Option<Arc<State>> {
    state_lock().clone()
}

pub struct Capture {
    thread: Option<JoinHandle<()>>,
    thread_id: u32,
    state: Arc<State>,
}

impl Capture {
    /// Installs the hooks on a new thread. Only one capture can run at a time.
    pub fn start(callbacks: CaptureCallbacks) -> Result<Self, CaptureError> {
        let state = Arc::new(State {
            callbacks,
            raw_dx: AtomicI32::new(0),
            raw_dy: AtomicI32::new(0),
            mapped: AtomicUsize::new(0),
            own_ignored: AtomicUsize::new(0),
        });
        {
            let mut slot = state_lock();
            if slot.is_some() {
                return Err(CaptureError("capture is already running".into()));
            }
            *slot = Some(Arc::clone(&state));
        }
        let (ready_tx, ready_rx) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("edgehop-win-capture".into())
            .spawn(move || run(&ready_tx));
        let thread = match spawned {
            Ok(thread) => thread,
            Err(error) => {
                *state_lock() = None;
                return Err(CaptureError(format!(
                    "cannot start the capture thread: {error}"
                )));
            }
        };
        match ready_rx.recv() {
            Ok(Ok(thread_id)) => Ok(Self {
                thread: Some(thread),
                thread_id,
                state,
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                *state_lock() = None;
                Err(error)
            }
            Err(_) => {
                *state_lock() = None;
                Err(CaptureError(
                    "the capture thread ended before it was ready".into(),
                ))
            }
        }
    }

    /// `(mapped, own_ignored)`: events delivered to the callbacks, and events dropped because they carry
    /// [`MARKER`] (our own injected input, which must never be captured again).
    pub fn statistics(&self) -> (usize, usize) {
        (
            self.state.mapped.load(Ordering::SeqCst),
            self.state.own_ignored.load(Ordering::SeqCst),
        )
    }

    /// Removes the hooks and ends the thread. Idempotent.
    pub fn stop(&mut self) {
        if let Some(thread) = self.thread.take() {
            // SAFETY: plain FFI call; the thread id belongs to the capture thread.
            unsafe {
                PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
            }
            let _ = thread.join();
            *state_lock() = None;
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop();
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error(what: &str) -> CaptureError {
    // SAFETY: GetLastError has no arguments.
    CaptureError(format!("{what} (Windows error {})", unsafe {
        GetLastError()
    }))
}

/// The capture thread: hooks, the raw input window, then the message loop.
fn run(ready: &mpsc::Sender<Result<u32, CaptureError>>) {
    ensure_dpi_aware();
    // SAFETY: every call below is plain FFI with valid arguments; handles are checked before use and released
    // before the thread ends.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let hook: HHOOK = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), instance, 0);
        if hook.is_null() {
            let _ = ready.send(Err(last_error("cannot install the mouse hook")));
            return;
        }
        let window = create_raw_input_window();
        if window.is_null() {
            UnhookWindowsHookEx(hook);
            let _ = ready.send(Err(last_error("cannot create the raw input window")));
            return;
        }
        let device = RAWINPUTDEVICE {
            usUsagePage: 1,
            usUsage: 2,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: window,
        };
        // Raw input is a refinement (edge pushes): if it is unavailable the hook still works.
        RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32);

        let _ = ready.send(Ok(GetCurrentThreadId()));
        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        UnhookWindowsHookEx(hook);
        DestroyWindow(window);
    }
}

unsafe fn create_raw_input_window() -> HWND {
    let class_name = wide("EdgeHopRawInput");
    // SAFETY: `class_name` outlives both calls; the class and window procedure are valid for the process.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(raw_input_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&class); // fails harmlessly if a previous capture registered it
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    }
}

/// Window procedure of the message-only window: collects raw mouse deltas.
unsafe extern "system" fn raw_input_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_INPUT {
        if let Some(state) = current_state() {
            // SAFETY: `raw` is a writable RAWINPUT of exactly the size passed; lparam is the handle Windows
            // gave us for this WM_INPUT.
            unsafe {
                let mut raw: RAWINPUT = zeroed();
                let mut size = size_of::<RAWINPUT>() as u32;
                let copied = GetRawInputData(
                    lparam as HRAWINPUT,
                    RID_INPUT,
                    (&mut raw as *mut RAWINPUT).cast(),
                    &mut size,
                    size_of::<RAWINPUTHEADER>() as u32,
                );
                if copied != u32::MAX && raw.header.dwType == RIM_TYPEMOUSE {
                    let mouse = raw.data.mouse;
                    if mouse.usFlags & MOUSE_MOVE_ABSOLUTE == 0 {
                        state.raw_dx.fetch_add(mouse.lLastX, Ordering::SeqCst);
                        state.raw_dy.fetch_add(mouse.lLastY, Ordering::SeqCst);
                    }
                }
            }
        }
    }
    // SAFETY: forwarding the arguments we were given.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Maps a hook message to a wire event. `None` for messages we do not forward.
fn map_event(message: u32, info: &MSLLHOOKSTRUCT) -> Option<InputEvent> {
    let wheel_delta = i32::from((info.mouseData >> 16) as u16 as i16);
    let x_button = if (info.mouseData >> 16) as u16 == 1 {
        MouseButton::Back
    } else {
        MouseButton::Forward
    };
    match message {
        WM_MOUSEMOVE => Some(InputEvent::MouseMoveAbsolute {
            x: info.pt.x,
            y: info.pt.y,
        }),
        WM_LBUTTONDOWN => Some(InputEvent::MouseButton {
            button: MouseButton::Left,
            is_down: true,
        }),
        WM_LBUTTONUP => Some(InputEvent::MouseButton {
            button: MouseButton::Left,
            is_down: false,
        }),
        WM_RBUTTONDOWN => Some(InputEvent::MouseButton {
            button: MouseButton::Right,
            is_down: true,
        }),
        WM_RBUTTONUP => Some(InputEvent::MouseButton {
            button: MouseButton::Right,
            is_down: false,
        }),
        WM_MBUTTONDOWN => Some(InputEvent::MouseButton {
            button: MouseButton::Middle,
            is_down: true,
        }),
        WM_MBUTTONUP => Some(InputEvent::MouseButton {
            button: MouseButton::Middle,
            is_down: false,
        }),
        WM_XBUTTONDOWN => Some(InputEvent::MouseButton {
            button: x_button,
            is_down: true,
        }),
        WM_XBUTTONUP => Some(InputEvent::MouseButton {
            button: x_button,
            is_down: false,
        }),
        WM_MOUSEWHEEL => Some(InputEvent::Scroll {
            horizontal: 0,
            vertical: wheel_delta,
        }),
        // The sign of horizontal scrolling between Windows and macOS still has to be checked on real devices.
        WM_MOUSEHWHEEL => Some(InputEvent::Scroll {
            horizontal: wheel_delta,
            vertical: 0,
        }),
        _ => None,
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY (all FFI below): `code >= 0` means lparam points to a valid MSLLHOOKSTRUCT for this call.
    if code < 0 {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
    let swallow = match current_state() {
        Some(state) => handle(&state, wparam as u32, info),
        None => false,
    };
    if swallow {
        1
    } else {
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }
}

/// Returns true if the event must be swallowed. A panic in a callback must not unwind through the hook (that
/// would be undefined behaviour), so it is caught and the event passes through.
fn handle(state: &State, message: u32, info: &MSLLHOOKSTRUCT) -> bool {
    if info.dwExtraInfo == MARKER {
        state.own_ignored.fetch_add(1, Ordering::SeqCst);
        return false;
    }
    let Some(event) = map_event(message, info) else {
        return false;
    };
    state.mapped.fetch_add(1, Ordering::SeqCst);
    catch_unwind(AssertUnwindSafe(|| {
        if message == WM_MOUSEMOVE {
            (state.callbacks.on_motion)(Motion {
                x: info.pt.x,
                y: info.pt.y,
                raw_dx: state.raw_dx.swap(0, Ordering::SeqCst),
                raw_dy: state.raw_dy.swap(0, Ordering::SeqCst),
            });
        }
        (state.callbacks.on_event)(event);
        (state.callbacks.should_swallow)(&event)
    }))
    .unwrap_or(false)
}
