use std::mem::size_of;

use edgehop_proto::MonitorInfo;
use windows_sys::Win32::Foundation::{LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};

use crate::dpi::ensure_dpi_aware;

const MONITORINFOF_PRIMARY: u32 = 1;

unsafe extern "system" fn collect(
    monitor: HMONITOR,
    _dc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> i32 {
    // SAFETY: `data` is the address of the `Vec` that `list_monitors` passed to `EnumDisplayMonitors`, which
    // only calls back while that call is running.
    let found = unsafe { &mut *(data as *mut Vec<(bool, RECT)>) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        rcMonitor: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        rcWork: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        dwFlags: 0,
    };
    // SAFETY: `info` is a valid MONITORINFO with `cbSize` set, as the API requires.
    if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
        found.push((info.dwFlags & MONITORINFOF_PRIMARY != 0, info.rcMonitor));
    }
    1 // keep enumerating
}

/// The displays in global desktop coordinates (the primary display's top-left is the origin), primary first,
/// as the protocol advertises them. Ids are `m0`, `m1`, ...
pub fn list_monitors() -> Vec<MonitorInfo> {
    ensure_dpi_aware();
    let mut found: Vec<(bool, RECT)> = Vec::new();
    // SAFETY: the callback only runs during this call and `found` outlives it; null device context and
    // clipping rectangle mean "all monitors".
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect),
            &mut found as *mut _ as LPARAM,
        );
    }
    found.sort_by_key(|(primary, _)| !*primary); // primary first (stable for the rest)
    found
        .into_iter()
        .enumerate()
        .map(|(index, (_, rect))| MonitorInfo {
            id: format!("m{index}"),
            x: f64::from(rect.left),
            y: f64::from(rect.top),
            width: f64::from(rect.right - rect.left),
            height: f64::from(rect.bottom - rect.top),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_at_least_one_display_and_the_primary_is_at_the_origin() {
        let monitors = list_monitors();
        assert!(!monitors.is_empty(), "no displays found");
        let primary = &monitors[0];
        assert_eq!(
            (primary.x, primary.y),
            (0.0, 0.0),
            "the primary display's top-left is the origin"
        );
        assert!(primary.width > 0.0 && primary.height > 0.0);
    }
}
