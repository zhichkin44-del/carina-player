//! What is playing right now. The prototype reads the system media session (Spotify, browser,
//! any player that shows up in the Windows media flyout); later our own engine feeds the same event.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// Seconds.
    pub position: f64,
    /// Seconds, 0 when unknown.
    pub duration: f64,
    /// `data:` URL of the cover, if the source provides one.
    pub cover: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum Command {
    Toggle,
    Next,
    Previous,
}

static COMMANDS: OnceLock<Mutex<Sender<Command>>> = OnceLock::new();

fn send(cmd: Command) {
    if let Some(tx) = COMMANDS.get() {
        let _ = tx.lock().unwrap().send(cmd);
    }
}

#[tauri::command]
pub fn media_toggle() {
    send(Command::Toggle);
}

#[tauri::command]
pub fn media_next() {
    send(Command::Next);
}

#[tauri::command]
pub fn media_previous() {
    send(Command::Previous);
}

/// Start the background thread that polls the system session and emits `now-playing`.
pub fn spawn(app: AppHandle) {
    let (tx, rx) = channel();
    let _ = COMMANDS.set(Mutex::new(tx));
    std::thread::spawn(move || run(app, rx));
}

#[cfg(windows)]
fn run(app: AppHandle, rx: Receiver<Command>) {
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let Ok(manager) = system::manager() else {
        return;
    };
    let mut last = NowPlaying::default();
    loop {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(cmd) => system::command(&manager, cmd),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let now = system::read(&manager, &last).unwrap_or_default();
        if now != last {
            let _ = app.emit("now-playing", &now);
            last = now;
        }
    }
}

#[cfg(not(windows))]
fn run(_app: AppHandle, rx: Receiver<Command>) {
    while rx.recv().is_ok() {}
}

#[cfg(windows)]
mod system {
    use base64::Engine;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    use windows::Storage::Streams::DataReader;
    use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
    use windows::core::Result;

    use super::{Command, NowPlaying};

    /// WinRT time unit: 100 ns.
    const TICKS_PER_SECOND: f64 = 10_000_000.0;

    pub fn manager() -> Result<Manager> {
        Manager::RequestAsync()?.join()
    }

    pub fn command(manager: &Manager, cmd: Command) {
        let Ok(session) = manager.GetCurrentSession() else {
            return;
        };
        let op = match cmd {
            Command::Toggle => session.TryTogglePlayPauseAsync(),
            Command::Next => session.TrySkipNextAsync(),
            Command::Previous => session.TrySkipPreviousAsync(),
        };
        if let Ok(op) = op {
            let _ = op.join();
        }
    }

    pub fn read(manager: &Manager, last: &NowPlaying) -> Result<NowPlaying> {
        let session = manager.GetCurrentSession()?;
        let props = session.TryGetMediaPropertiesAsync()?.join()?;
        let playing = session.GetPlaybackInfo()?.PlaybackStatus()? == Status::Playing;
        let mut now = NowPlaying {
            title: props.Title()?.to_string(),
            artist: props.Artist()?.to_string(),
            album: props.AlbumTitle()?.to_string(),
            playing,
            ..Default::default()
        };
        (now.position, now.duration) = timeline(&session, playing).unwrap_or((0.0, 0.0));

        let same_track = now.title == last.title && now.artist == last.artist;
        now.cover = if same_track && last.cover.is_some() {
            last.cover.clone()
        } else {
            cover(&props).ok()
        };
        Ok(now)
    }

    /// Position extrapolated to "now": sessions only report it when something changes.
    fn timeline(session: &Session, playing: bool) -> Result<(f64, f64)> {
        let t = session.GetTimelineProperties()?;
        let start = t.StartTime()?.Duration as f64;
        let end = t.EndTime()?.Duration as f64;
        let mut pos = t.Position()?.Duration as f64;
        if playing {
            let updated = t.LastUpdatedTime()?.UniversalTime as f64;
            let ft = unsafe { GetSystemTimeAsFileTime() };
            let now = (((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64) as f64;
            if updated > 0.0 && now > updated {
                pos += now - updated;
            }
        }
        let duration = ((end - start) / TICKS_PER_SECOND).max(0.0);
        let position = ((pos - start) / TICKS_PER_SECOND).clamp(0.0, duration.max(0.0));
        Ok((position, duration))
    }

    fn cover(
        props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties,
    ) -> Result<String> {
        let stream = props.Thumbnail()?.OpenReadAsync()?.join()?;
        let size = stream.Size()? as u32;
        let mime = stream.ContentType()?.to_string();
        let reader = DataReader::CreateDataReader(&stream)?;
        reader.LoadAsync(size)?.join()?;
        let mut bytes = vec![0u8; size as usize];
        reader.ReadBytes(&mut bytes)?;
        let mime = if mime.is_empty() { "image/png".into() } else { mime };
        Ok(format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    }
}
