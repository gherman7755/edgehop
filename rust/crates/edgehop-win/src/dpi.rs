use std::sync::Once;

use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

/// Makes the process DPI aware (once). Without it Windows hands out virtualised, scaled coordinates, and cursor
/// positions and display sizes would not match the physical pixels the other computer expects. Failure is
/// ignored: it means the awareness was already set (by a manifest or an earlier call).
pub fn ensure_dpi_aware() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: plain FFI call with a constant argument, no pointers involved.
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
    });
}
