//! Tray icon: show/hide the widget, start with Windows, quit.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

const SHOW: &str = "Показать виджет";
const HIDE: &str = "Спрятать виджет";

/// A desktop widget is meant to be there after a reboot: autostart is switched on once, on the
/// first run; after that the user's choice in the tray menu stands.
fn first_run_autostart(app: &AppHandle) {
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    let flag = dir.join("first-run-done");
    if flag.exists() {
        return;
    }
    let _ = app.autolaunch().enable();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(flag, b"");
}

fn toggle_widget(app: &AppHandle, item: &MenuItem<tauri::Wry>) {
    let Some(widget) = app.get_webview_window("widget") else {
        return;
    };
    if widget.is_visible().unwrap_or(true) {
        let _ = widget.hide();
        let _ = item.set_text(SHOW);
    } else {
        let _ = widget.show();
        // Tauri rewrites the window styles on show: pin the widget to the desktop again.
        #[cfg(windows)]
        if let Ok(hwnd) = widget.hwnd() {
            crate::desktop::pin_again(hwnd);
        }
        let _ = item.set_text(HIDE);
    }
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    first_run_autostart(app);
    // Re-register on every start so autostart always points at this copy of Carina
    // (after installing, updating or moving the program).
    if app.autolaunch().is_enabled().unwrap_or(false) {
        let _ = app.autolaunch().enable();
    }
    let toggle = MenuItem::with_id(app, "toggle", HIDE, true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Запускать вместе с Windows",
        true,
        app.autolaunch().is_enabled().unwrap_or(false),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    // The developer's mark, as on every product: shown, not clickable.
    let signature = MenuItem::with_id(app, "signature", "by zhichkin44-del", false, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &signature,
            &PredefinedMenuItem::separator(app)?,
            &toggle,
            &autostart,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let on_menu = toggle.clone();
    let on_click = toggle.clone();
    let mut tray = TrayIconBuilder::with_id("carina")
        .tooltip("Carina")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "toggle" => toggle_widget(app, &on_menu),
            "autostart" => {
                let launcher = app.autolaunch();
                let on = launcher.is_enabled().unwrap_or(false);
                let _ = if on {
                    launcher.disable()
                } else {
                    launcher.enable()
                };
                let _ = autostart.set_checked(launcher.is_enabled().unwrap_or(!on));
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_widget(tray.app_handle(), &on_click);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}
