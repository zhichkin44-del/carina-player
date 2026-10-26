#[doc(hidden)]
pub mod check;
#[cfg(windows)]
mod desktop;
mod engine;
mod layout;
mod library;
mod media;
mod playlists;
mod position;
#[cfg(windows)]
mod smtc;
mod tray;

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use tauri::Manager;

/// Widget size in logical pixels (must match `tauri.conf.json`).
const WIDGET_SIZE: (f64, f64) = (400.0, 132.0);
const WIDGET_MARGIN: f64 = 24.0;
/// Height of the window with the library panel open, logical pixels: room for three rows of folders
/// plus the "+" card, so the last row is never cut off. Less than this only when the screen is shorter.
const PANEL_HEIGHT: f64 = 800.0;

static PANEL_OPEN: AtomicBool = AtomicBool::new(false);
/// The panel grew upwards (the widget sits in the lower half of the screen).
static PANEL_UP: AtomicBool = AtomicBool::new(false);
/// Height the panel was opened with, physical pixels; closing restores the widget from it.
static PANEL_TALL: AtomicI32 = AtomicI32::new(0);

/// Open or close the library panel: the window grows towards the middle of the screen.
/// Returns "up" or "down" so the page puts the panel on the right side of the widget card.
#[tauri::command]
fn set_panel(window: tauri::WebviewWindow, open: bool) -> tauri::Result<&'static str> {
    let scale = window.scale_factor()?;
    let pos = window.outer_position()?;
    let width = window.outer_size()?.width;
    let compact = (WIDGET_SIZE.1 * scale).round() as i32;
    let dir = |up: bool| if up { "up" } else { "down" };

    if open {
        if PANEL_OPEN.swap(true, Ordering::Relaxed) {
            return Ok(dir(PANEL_UP.load(Ordering::Relaxed)));
        }
        let wanted = (PANEL_HEIGHT * scale).round() as i32;
        let margin = (WIDGET_MARGIN * scale).round() as i32;
        let (up, tall) = match window.current_monitor()? {
            Some(m) => {
                let area = m.work_area();
                let middle = area.position.y + area.size.height as i32 / 2;
                let up = pos.y + compact / 2 > middle;
                // Never taller than the room on the widget's side of the screen (the taskbar is excluded),
                // and never shorter than the widget itself, even when the widget sits at the very edge.
                let room = if up {
                    pos.y + compact - area.position.y
                } else {
                    area.position.y + area.size.height as i32 - pos.y
                };
                (up, wanted.min(room - margin).max(compact))
            }
            None => (false, wanted),
        };
        PANEL_UP.store(up, Ordering::Relaxed);
        PANEL_TALL.store(tall, Ordering::Relaxed);
        position::suspend(true);
        if up {
            window.set_position(tauri::PhysicalPosition::new(
                pos.x,
                pos.y - (tall - compact),
            ))?;
        }
        window.set_size(tauri::PhysicalSize::new(width, tall as u32))?;
        window.set_focusable(true)?;
        #[cfg(windows)]
        desktop::styles(window.hwnd()?, true);
        let _ = window.set_focus();
        Ok(dir(up))
    } else {
        if !PANEL_OPEN.swap(false, Ordering::Relaxed) {
            return Ok("none");
        }
        let up = PANEL_UP.load(Ordering::Relaxed);
        let tall = PANEL_TALL.load(Ordering::Relaxed);
        window.set_size(tauri::PhysicalSize::new(width, compact as u32))?;
        if up {
            window.set_position(tauri::PhysicalPosition::new(
                pos.x,
                pos.y + (tall - compact),
            ))?;
        }
        window.set_focusable(false)?;
        #[cfg(windows)]
        desktop::styles(window.hwnd()?, false);
        position::suspend(false);
        Ok("none")
    }
}

/// Where the user left the widget, if that spot is still on a screen; else the bottom-right
/// corner of the primary monitor.
fn place_widget(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = window.primary_monitor()? else {
        return Ok(());
    };
    let scale = monitor.scale_factor();
    let size = (
        (WIDGET_SIZE.0 * scale).round() as i32,
        (WIDGET_SIZE.1 * scale).round() as i32,
    );

    let screens: Vec<layout::Rect> = window
        .available_monitors()?
        .iter()
        .map(|m| layout::Rect {
            x: m.position().x,
            y: m.position().y,
            w: m.size().width as i32,
            h: m.size().height as i32,
        })
        .collect();
    if let Some(saved) = position::load(window.app_handle())
        && layout::fits((saved.x, saved.y), size, &screens)
    {
        return window.set_position(tauri::PhysicalPosition::new(saved.x, saved.y));
    }

    let area = monitor.work_area();
    let work = layout::Rect {
        x: area.position.x,
        y: area.position.y,
        w: area.size.width as i32,
        h: area.size.height as i32,
    };
    let (x, y) = layout::bottom_right(work, size, (WIDGET_MARGIN * scale).round() as i32);
    window.set_position(tauri::PhysicalPosition::new(x, y))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // A second launch (e.g. double-clicking an mp3 while Carina runs) hands its files over.
        .plugin(tauri_plugin_single_instance::init(|_app, args, _cwd| {
            engine::open_paths(args.into_iter().skip(1));
        }))
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let widget = app
                .get_webview_window("widget")
                .expect("widget window is declared in tauri.conf.json");
            place_widget(&widget)?;
            position::track(&widget);
            // Show through Tauri first: it owns the window styles and rewrites them on show.
            widget.show()?;
            #[cfg(windows)]
            {
                desktop::pin(widget.hwnd()?);
                let _ = smtc::init(widget.hwnd()?);
            }
            media::spawn(app.handle().clone());
            engine::spawn(app.handle().clone());
            tray::init(app.handle())?;
            engine::open_paths(std::env::args().skip(1));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            media::media_toggle,
            media::media_next,
            media::media_previous,
            media::media_seek,
            engine::play_folder,
            engine::play_tracks,
            engine::queue_jump,
            engine::open_files,
            engine::queue_play_next,
            engine::queue_add,
            engine::queue_remove,
            engine::queue_move,
            engine::set_volume,
            engine::set_shuffle,
            engine::set_repeat,
            library::library_roots,
            library::library_add,
            library::library_remove,
            library::library_open,
            library::library_cover,
            library::library_search,
            library::folder_tracks,
            playlists::playlist_cover,
            playlists::playlists,
            playlists::playlist_tracks,
            playlists::playlist_create,
            playlists::playlist_rename,
            playlists::playlist_delete,
            playlists::playlist_add,
            playlists::playlist_remove,
            playlists::playlist_move,
            playlists::favorite_toggle,
            playlists::favorites,
            tray::set_tray_language,
            set_panel
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
