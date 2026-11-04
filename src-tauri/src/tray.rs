//! Tray icon: show/hide the widget, start with Windows, quit. The menu follows the widget's language.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

/// Menu texts for one language. English is the default; the widget's settings switch to Russian.
struct Labels {
    show: &'static str,
    hide: &'static str,
    autostart: &'static str,
    quit: &'static str,
}

fn labels(lang: &str) -> Labels {
    match lang {
        "ru" => Labels {
            show: "Показать виджет",
            hide: "Спрятать виджет",
            autostart: "Запускать вместе с Windows",
            quit: "Выход",
        },
        _ => Labels {
            show: "Show widget",
            hide: "Hide widget",
            autostart: "Start with Windows",
            quit: "Quit",
        },
    }
}

/// Only "ru" and "en" exist; anything else (including a missing file) means English.
fn normalize(lang: &str) -> &'static str {
    if lang.trim() == "ru" { "ru" } else { "en" }
}

/// The language is kept next to the other app data, so a restart does not flash the other language.
/// Only the first start after the update shows English: the file appears once the widget reports its language.
fn lang_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join("lang"))
}

/// Menu items whose text depends on the language, and the language they currently show.
pub struct Tray {
    toggle: MenuItem<Wry>,
    autostart: CheckMenuItem<Wry>,
    quit: MenuItem<Wry>,
    lang: Mutex<&'static str>,
}

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

fn current_lang(tray: &Tray) -> &'static str {
    tray.lang.lock().map(|lang| *lang).unwrap_or("en")
}

fn toggle_widget(app: &AppHandle) {
    let Some(widget) = app.get_webview_window("widget") else {
        return;
    };
    let tray = app.state::<Tray>();
    let text = labels(current_lang(&tray));
    if widget.is_visible().unwrap_or(true) {
        let _ = widget.hide();
        let _ = tray.toggle.set_text(text.show);
    } else {
        let _ = widget.show();
        // Tauri rewrites the window styles on show: pin the widget to the desktop again.
        #[cfg(windows)]
        if let Ok(hwnd) = widget.hwnd() {
            crate::desktop::pin_again(hwnd);
        }
        let _ = tray.toggle.set_text(text.hide);
    }
}

/// The widget reports its language on start and on every change: the tray menu follows it,
/// and the choice is kept for the next start.
#[tauri::command]
pub fn set_tray_language(app: AppHandle, lang: String) {
    let lang = normalize(&lang);
    let Some(tray) = app.try_state::<Tray>() else {
        return;
    };
    if let Ok(mut current) = tray.lang.lock() {
        *current = lang;
    }
    let text = labels(lang);
    let visible = app
        .get_webview_window("widget")
        .and_then(|widget| widget.is_visible().ok())
        .unwrap_or(true);
    let _ = tray.toggle.set_text(if visible { text.hide } else { text.show });
    let _ = tray.autostart.set_text(text.autostart);
    let _ = tray.quit.set_text(text.quit);
    if let Some(path) = lang_file(&app) {
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(path, lang);
    }
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    first_run_autostart(app);
    // Re-register on every start so autostart always points at this copy of Carina
    // (after installing, updating or moving the program).
    if app.autolaunch().is_enabled().unwrap_or(false) {
        let _ = app.autolaunch().enable();
    }
    let lang = lang_file(app)
        .and_then(|path| fs::read_to_string(path).ok())
        .map_or("en", |saved| normalize(&saved));
    let text = labels(lang);
    let toggle = MenuItem::with_id(app, "toggle", text.hide, true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        text.autostart,
        true,
        app.autolaunch().is_enabled().unwrap_or(false),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", text.quit, true, None::<&str>)?;
    // The developer's mark, as on every product: shown, not clickable.
    let signature = MenuItem::with_id(app, "signature", "by Zhichkin", false, None::<&str>)?;
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
    // Clones share the menu items, so changing the text here changes the open menu too.
    app.manage(Tray {
        toggle: toggle.clone(),
        autostart: autostart.clone(),
        quit: quit.clone(),
        lang: Mutex::new(lang),
    });

    let mut tray = TrayIconBuilder::with_id("carina")
        .tooltip("Carina")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle" => toggle_widget(app),
            "autostart" => {
                let launcher = app.autolaunch();
                let on = launcher.is_enabled().unwrap_or(false);
                let _ = if on {
                    launcher.disable()
                } else {
                    launcher.enable()
                };
                let _ = app
                    .state::<Tray>()
                    .autostart
                    .set_checked(launcher.is_enabled().unwrap_or(!on));
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_widget(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_the_default_language() {
        assert_eq!(normalize(""), "en");
        assert_eq!(normalize("de"), "en");
        assert_eq!(normalize("ru\n"), "ru");
        assert_eq!(labels(normalize("garbage")).quit, "Quit");
    }
}
