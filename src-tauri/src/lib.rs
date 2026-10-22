#[cfg(windows)]
mod desktop;
mod layout;
mod media;

use tauri::Manager;

/// Widget size in logical pixels (must match `tauri.conf.json`).
const WIDGET_SIZE: (f64, f64) = (400.0, 132.0);
const WIDGET_MARGIN: f64 = 24.0;
/// Glass tint, 0xAABBGGRR: deep space blue at ~35% opacity.
#[cfg(windows)]
const GLASS_TINT: u32 = 0x5A1A1210;

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
            #[cfg(windows)]
            {
                let hwnd = widget.hwnd()?;
                desktop::glass(hwnd, GLASS_TINT);
                desktop::pin(hwnd);
            }
            widget.show()?;
            media::spawn(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            media::media_toggle,
            media::media_next,
            media::media_previous
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
