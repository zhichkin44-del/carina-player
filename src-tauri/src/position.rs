//! Remembers where the user dragged the widget.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow, WindowEvent};

/// Physical pixels, top-left corner of the widget.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub x: i32,
    pub y: i32,
}

/// A drag produces a stream of moves; write once it has settled.
const SETTLE: Duration = Duration::from_millis(400);

/// While the panel is open the window is taller and may have grown upwards: that is not a spot
/// the user chose, so don't save it.
static SUSPENDED: AtomicBool = AtomicBool::new(false);

pub fn suspend(on: bool) {
    SUSPENDED.store(on, Ordering::Relaxed);
}

fn file(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_config_dir().ok()?.join("widget.json"))
}

pub fn load(app: &AppHandle) -> Option<Saved> {
    let text = std::fs::read_to_string(file(app)?).ok()?;
    serde_json::from_str(&text).ok()
}

fn store(app: &AppHandle, pos: Saved) {
    let Some(path) = file(app) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(&pos) {
        let _ = std::fs::write(path, text);
    }
}

/// Save the widget's position whenever it is moved (after the drag settles).
pub fn track(window: &WebviewWindow) {
    let (tx, rx) = channel::<PhysicalPosition<i32>>();
    window.on_window_event(move |event| {
        if let WindowEvent::Moved(pos) = event {
            let _ = tx.send(*pos);
        }
    });
    let app = window.app_handle().clone();
    std::thread::spawn(move || {
        while let Ok(mut pos) = rx.recv() {
            loop {
                match rx.recv_timeout(SETTLE) {
                    Ok(newer) => pos = newer,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            if !SUSPENDED.load(Ordering::Relaxed) {
                store(&app, Saved { x: pos.x, y: pos.y });
            }
        }
    });
}
