//! Windows: "lives on the desktop" behaviour for the widget window. The glass itself is just
//! the transparent webview with a tint the page paints (the user chose no blur).
//!
//! The widget is owned by the desktop window (Progman), so it always stays right above the
//! wallpaper, below normal windows, and is not minimised by Win+D. It never takes focus.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZESTART, FindWindowW, GWL_EXSTYLE, GWLP_HWNDPARENT,
    GetWindowLongPtrW, HWND_BOTTOM, IsIconic, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SetWindowLongPtrW, SetWindowPos, ShowWindow, WINEVENT_OUTOFCONTEXT,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};
use windows::core::w;

static WIDGET: AtomicIsize = AtomicIsize::new(0);

/// No taskbar button and no Alt+Tab; takes focus only while `focusable` (the open panel, so the
/// search field can be typed into). Tauri rewrites these styles when it changes focusability,
/// so call this again after that.
pub fn styles(hwnd: HWND, focusable: bool) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let mut ex = (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize);
        if focusable {
            ex &= !(WS_EX_NOACTIVATE.0 as isize);
        } else {
            ex |= WS_EX_NOACTIVATE.0 as isize;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
    }
}

/// After Tauri shows a hidden widget again: restore our styles, owner and z-order.
pub fn pin_again(hwnd: HWND) {
    styles(hwnd, false);
    unsafe {
        if let Ok(progman) = FindWindowW(w!("Progman"), None) {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman.0 as isize);
        }
    }
    send_to_bottom(hwnd);
}

/// Pin the widget to the desktop: owned by Progman, no focus, survives Win+D.
pub fn pin(hwnd: HWND) {
    WIDGET.store(hwnd.0 as isize, Ordering::Relaxed);
    styles(hwnd, false);
    unsafe {
        if let Ok(progman) = FindWindowW(w!("Progman"), None) {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman.0 as isize);
        }
        send_to_bottom(hwnd);

        // Win+D / "Show desktop" and minimise attempts: put the widget straight back.
        for event in [EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZESTART] {
            SetWinEventHook(
                event,
                event,
                None,
                Some(on_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
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
