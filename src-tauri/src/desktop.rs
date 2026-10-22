//! Windows: glass backdrop and "lives on the desktop" behaviour for the widget window.
//!
//! The widget is owned by the desktop window (Progman), so it always stays right above the
//! wallpaper, below normal windows, and is not minimised by Win+D. It never takes focus.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZESTART, FindWindowW, GWL_EXSTYLE, GWLP_HWNDPARENT,
    GetWindowLongPtrW, HWND_BOTTOM, IsIconic, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SetWindowLongPtrW, SetWindowPos, ShowWindow, WINEVENT_OUTOFCONTEXT,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, w};

static WIDGET: AtomicIsize = AtomicIsize::new(0);

/// Frosted glass behind the transparent webview + rounded Windows 11 corners.
///
/// Uses the accent policy (SetWindowCompositionAttribute), not DWM system backdrops: system
/// backdrops turn into a flat colour when the window is inactive, and the widget is never active.
pub fn glass(hwnd: HWND, tint_abgr: u32) {
    #[repr(C)]
    struct AccentPolicy {
        state: u32,
        flags: u32,
        gradient: u32,
        animation: u32,
    }
    #[repr(C)]
    struct CompositionData {
        attrib: u32,
        data: *mut AccentPolicy,
        size: usize,
    }
    type Swca = unsafe extern "system" fn(HWND, *mut CompositionData) -> BOOL;

    const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
    const WCA_ACCENT_POLICY: u32 = 19;

    unsafe {
        let user32 = windows::Win32::System::LibraryLoader::GetModuleHandleW(w!("user32.dll"));
        if let Ok(user32) = user32
            && let Some(f) = windows::Win32::System::LibraryLoader::GetProcAddress(
                user32,
                windows::core::s!("SetWindowCompositionAttribute"),
            )
        {
            let swca: Swca = std::mem::transmute(f);
            let mut policy = AccentPolicy {
                state: ACCENT_ENABLE_ACRYLICBLURBEHIND,
                flags: 0,
                gradient: tint_abgr,
                animation: 0,
            };
            let mut data = CompositionData {
                attrib: WCA_ACCENT_POLICY,
                data: &mut policy,
                size: std::mem::size_of::<AccentPolicy>(),
            };
            let _ = swca(hwnd, &mut data);
        }
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &round as *const _ as _,
            std::mem::size_of_val(&round) as u32,
        );
    }
}

/// Pin the widget to the desktop: no taskbar button, no Alt+Tab, no focus, owned by Progman.
pub fn pin(hwnd: HWND) {
    WIDGET.store(hwnd.0 as isize, Ordering::Relaxed);
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let ex = (ex | (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) as isize)
            & !(WS_EX_APPWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);

        if let Ok(progman) = FindWindowW(w!("Progman"), None) {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman.0 as isize);
        }
        send_to_bottom(hwnd);

        // Win+D / "Show desktop" and minimise attempts: put the widget straight back.
        for event in [EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZESTART] {
            SetWinEventHook(event, event, None, Some(on_event), 0, 0, WINEVENT_OUTOFCONTEXT);
        }
    }
}

fn send_to_bottom(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_BOTTOM),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    let widget = HWND(WIDGET.load(Ordering::Relaxed) as *mut _);
    if widget.0.is_null() {
        return;
    }
    unsafe {
        if IsIconic(widget).as_bool() {
            let _ = ShowWindow(widget, SW_SHOWNOACTIVATE);
        }
    }
    send_to_bottom(widget);
}
