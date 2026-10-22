#[cfg(windows)]
mod desktop;
mod layout;
mod media;

use tauri::Manager;

/// Widget size in logical pixels (must match `tauri.conf.json`).
const WIDGET_SIZE: (f64, f64) = (400.0, 132.0);
const WIDGET_MARGIN: f64 = 24.0;
/// Native acrylic tint, 0xAABBGGRR. Windows barely applies it, so the page paints the
/// user's background opacity itself; this only keeps acrylic from misbehaving at alpha 0.
#[cfg(windows)]
const GLASS_TINT: u32 = 0x101A1210;

/// Blur the wallpaper behind the widget, or keep it sharp.
#[tauri::command]
fn set_glass(window: tauri::WebviewWindow, blur: bool) -> tauri::Result<()> {
    #[cfg(windows)]
    desktop::glass(window.hwnd()?, GLASS_TINT, blur);
    #[cfg(not(windows))]
    let _ = (window, blur);
    Ok(())
}

fn place_widget(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = window.primary_monitor()? else {
        return Ok(());
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let work = layout::Rect {
        x: area.position.x,
        y: area.position.y,
        w: area.size.width as i32,
        h: area.size.height as i32,
    };
    let size = (
        (WIDGET_SIZE.0 * scale).round() as i32,
        (WIDGET_SIZE.1 * scale).round() as i32,
    );
    let (x, y) = layout::bottom_right(work, size, (WIDGET_MARGIN * scale).round() as i32);
    window.set_position(tauri::PhysicalPosition::new(x, y))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let widget = app
                .get_webview_window("widget")
                .expect("widget window is declared in tauri.conf.json");
            place_widget(&widget)?;
            // Show through Tauri first: it owns the window styles and rewrites them on show.
            widget.show()?;
            #[cfg(windows)]
            {
                let hwnd = widget.hwnd()?;
                desktop::glass(hwnd, GLASS_TINT, true);
                desktop::pin(hwnd);
            }
            media::spawn(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            media::media_toggle,
            media::media_next,
            media::media_previous,
            set_glass
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
