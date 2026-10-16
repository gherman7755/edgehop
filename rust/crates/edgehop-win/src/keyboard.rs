//! Keyboard capture: one low-level keyboard hook shared by everything in the process that wants to see keys (the
//! controller's key forwarding and the receiver's panic-chord watch). Keys are reported as HID usages
//! (`docs/keymap.csv`), by scan code, so the layout does not matter. The first subscriber installs the hook on its
//! own thread, the last one removes it.
//!
//! Callbacks run inside the hook, which Windows abandons if it takes longer than about 300 ms: keep them quick.

use std::mem::zeroed;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use edgehop_input::ModifierTracker;
use edgehop_proto::{windows_key_to_hid, InputEvent};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::capture::CaptureError;
use crate::MARKER;

const LLKHF_EXTENDED: u32 = 0x01;

/// Decides whether a key must not reach applications (`true`: it is being forwarded to another computer).
pub type KeyCallback = Arc<dyn Fn(InputEvent) -> bool + Send + Sync>;

struct Hook {
    thread: JoinHandle<()>,
    thread_id: u32,
}

#[derive(Default)]
struct Registry {
    next_id: u64,
    subscribers: Vec<(u64, KeyCallback)>,
    tracker: ModifierTracker,
    hook: Option<Hook>,
}

/// The hook procedure is a plain function: it finds its state here.
static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

fn registry() -> MutexGuard<'static, Option<Registry>> {
    REGISTRY.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A live subscription to keyboard events; dropping it unsubscribes (and removes the hook if it was the last).
pub struct KeySubscription {
    id: u64,
}

impl KeySubscription {
    /// Starts receiving key events. `callback` runs in the hook: keep it fast and never block.
    pub fn new(callback: KeyCallback) -> Result<Self, CaptureError> {
        let mut guard = registry();
        let state = guard.get_or_insert_with(Registry::default);
        let id = state.next_id;
        state.next_id += 1;
        state.subscribers.push((id, callback));
        if state.hook.is_none() {
            match start_hook() {
                Ok(hook) => state.hook = Some(hook),
                Err(error) => {
                    state.subscribers.pop();
                    return Err(error);
                }
            }
        }
        Ok(Self { id })
    }
}

impl Drop for KeySubscription {
    fn drop(&mut self) {
        let hook = {
            let mut guard = registry();
            let Some(state) = guard.as_mut() else { return };
            state.subscribers.retain(|(id, _)| *id != self.id);
            if state.subscribers.is_empty() {
                state.tracker.reset();
                state.hook.take()
            } else {
                None
            }
        };
        // Joined outside the lock: the hook thread takes it while it handles its last events.
        if let Some(hook) = hook {
            // SAFETY: plain FFI call; the thread id belongs to the hook thread.
            unsafe {
                PostThreadMessageW(hook.thread_id, WM_QUIT, 0, 0);
            }
            let _ = hook.thread.join();
        }
    }
}

fn start_hook() -> Result<Hook, CaptureError> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("edgehop-win-keyboard".into())
        .spawn(move || run(&ready_tx))
        .map_err(|e| CaptureError(format!("cannot start the keyboard thread: {e}")))?;
    match ready_rx.recv() {
        Ok(Ok(thread_id)) => Ok(Hook { thread, thread_id }),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(_) => Err(CaptureError(
            "the keyboard thread ended before it was ready".into(),
        )),
    }
}

fn run(ready: &mpsc::Sender<Result<u32, CaptureError>>) {
    // SAFETY: plain FFI with valid arguments; the hook is removed before the thread ends.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let hook: HHOOK = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), instance, 0);
        if hook.is_null() {
            let _ = ready.send(Err(CaptureError(
                "cannot install the keyboard hook".to_string(),
            )));
            return;
        }
        let _ = ready.send(Ok(GetCurrentThreadId()));
        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        UnhookWindowsHookEx(hook);
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY (all FFI below): `code >= 0` means lparam points to a valid KBDLLHOOKSTRUCT for this call.
    if code < 0 {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    let swallow = catch_unwind(AssertUnwindSafe(|| handle(wparam as u32, info))).unwrap_or(false);
    if swallow {
        1
    } else {
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }
}

fn handle(message: u32, info: &KBDLLHOOKSTRUCT) -> bool {
    if info.dwExtraInfo == MARKER {
        return false; // our own injected key: never captured again
    }
    let is_down = match message {
        WM_KEYDOWN | WM_SYSKEYDOWN => true,
        WM_KEYUP | WM_SYSKEYUP => false,
        _ => return false,
    };
    let extended = info.flags & LLKHF_EXTENDED != 0;
    let Some(code) = windows_key_to_hid(info.vkCode, info.scanCode as u16, extended) else {
        return false;
    };
    let (event, callbacks) = {
        let mut guard = registry();
        let Some(state) = guard.as_mut() else {
            return false;
        };
        let modifiers = state.tracker.apply(code, is_down);
        let callbacks: Vec<KeyCallback> = state
            .subscribers
            .iter()
            .map(|(_, c)| Arc::clone(c))
            .collect();
        (
            InputEvent::Key {
                code,
                is_down,
                modifiers,
            },
            callbacks,
        )
    };
    // Every subscriber sees every key; any of them can keep it from applications.
    callbacks
        .iter()
        .fold(false, |swallow, callback| callback(event) | swallow)
}
