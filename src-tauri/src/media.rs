//! What is playing right now. The prototype reads the system media session (Spotify, browser,
//! any player that shows up in the Windows media flyout); later our own engine feeds the same event.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::engine;

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
    /// The player allows skipping forward (a player with a one-track queue doesn't).
    pub can_next: bool,
    /// Back works: the player skips back, or at least we can restart the track.
    pub can_previous: bool,
}

/// What "back" does, given what the player allows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Back {
    Skip,
    Restart,
    Nothing,
}

pub fn back_action(previous_enabled: bool, seek_enabled: bool) -> Back {
    if previous_enabled {
        Back::Skip
    } else if seek_enabled {
        Back::Restart
    } else {
        Back::Nothing
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Command {
    Toggle,
    Next,
    Previous,
    /// Seconds from the start of the track.
    Seek(f64),
}

static COMMANDS: OnceLock<Mutex<Sender<Command>>> = OnceLock::new();

fn send(cmd: Command) {
    if let Some(tx) = COMMANDS.get() {
        let _ = tx.lock().unwrap().send(cmd);
    }
}

// The widget's buttons drive Carina's own engine when it has a queue, else the system player.

#[tauri::command]
pub fn media_toggle() {
    if engine::active() {
        engine::send(engine::Cmd::Toggle)
    } else {
        send(Command::Toggle)
    }
}

#[tauri::command]
pub fn media_next() {
    if engine::active() {
        engine::send(engine::Cmd::Next)
    } else {
        send(Command::Next)
    }
}

#[tauri::command]
pub fn media_previous() {
    if engine::active() {
        engine::send(engine::Cmd::Previous)
    } else {
        send(Command::Previous)
    }
}

#[tauri::command]
pub fn media_seek(seconds: f64) {
    if engine::active() {
        engine::send(engine::Cmd::Seek(seconds))
    } else {
        send(Command::Seek(seconds))
    }
}

/// Which session to show, from `(app id, is playing)` pairs: one that is playing wins — the one
/// we already show, then the system's current, then any; with nothing playing we stay on the one
/// we showed (so Play resumes it), else fall back to the system's current, else the first.
pub fn pick(
    sessions: &[(String, bool)],
    current: Option<&str>,
    shown: Option<&str>,
) -> Option<usize> {
    let find = |id: Option<&str>, playing_only: bool| {
        let id = id?;
        sessions
            .iter()
            .position(|(s, playing)| s == id && (*playing || !playing_only))
    };
    find(shown, true)
        .or_else(|| find(current, true))
        .or_else(|| sessions.iter().position(|(_, playing)| *playing))
        .or_else(|| find(shown, false))
        .or_else(|| find(current, false))
        .or_else(|| (!sessions.is_empty()).then_some(0))
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
    // App id of the session on the widget, so we don't jump between players.
    let mut shown: Option<String> = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(cmd) => system::command(&manager, &shown, cmd),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if engine::active() {
            continue;
        }
        let now = system::read(&manager, &mut shown, &last).unwrap_or_default();
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

    use super::{Back, Command, NowPlaying, back_action, pick};

    /// How Windows names Carina's own media session: the executable's file name.
    fn own_app_id() -> Option<String> {
        Some(std::env::current_exe().ok()?.file_name()?.to_string_lossy().into_owned())
    }

    /// WinRT time unit: 100 ns.
    const TICKS_PER_SECOND: f64 = 10_000_000.0;

    pub fn manager() -> Result<Manager> {
        Manager::RequestAsync()?.join()
    }

    /// The session to show and control, and its app id.
    fn session(manager: &Manager, shown: &Option<String>) -> Result<Option<(Session, String)>> {
        let list = manager.GetSessions()?;
        let mut sessions = Vec::new();
        let mut keys = Vec::new();
        let own = own_app_id();
        for i in 0..list.Size()? {
            // A session that can't be read (e.g. a closed one) must not hide the others.
            let Ok(session) = list.GetAt(i) else { continue };
            let Ok(id) = session.SourceAppUserModelId().map(|id| id.to_string()) else {
                continue;
            };
            // Carina's own playback is shown by the engine, not as "another player".
            if own.as_deref().is_some_and(|own| id.eq_ignore_ascii_case(own)) {
                continue;
            }
            let Ok(status) = session.GetPlaybackInfo().and_then(|p| p.PlaybackStatus()) else {
                continue;
            };
            keys.push((id.clone(), status == Status::Playing));
            sessions.push((session, id));
        }
        let current = manager
            .GetCurrentSession()
            .and_then(|s| s.SourceAppUserModelId())
            .map(|id| id.to_string())
            .ok()
            .filter(|id| !own.as_deref().is_some_and(|own| id.eq_ignore_ascii_case(own)));
        Ok(pick(&keys, current.as_deref(), shown.as_deref()).map(|i| sessions.swap_remove(i)))
    }

    pub fn command(manager: &Manager, shown: &Option<String>, cmd: Command) {
        let Ok(Some((session, _))) = session(manager, shown) else {
            return;
        };
        let op = match cmd {
            Command::Toggle => session.TryTogglePlayPauseAsync(),
            Command::Next => session.TrySkipNextAsync(),
            Command::Previous => match back(&session) {
                Ok(Back::Skip) => session.TrySkipPreviousAsync(),
                Ok(Back::Restart) => session.TryChangePlaybackPositionAsync(0),
                _ => return,
            },
            Command::Seek(seconds) => {
                let start = session
                    .GetTimelineProperties()
                    .and_then(|t| t.StartTime())
                    .map(|t| t.Duration)
                    .unwrap_or(0);
                session.TryChangePlaybackPositionAsync(start + (seconds * TICKS_PER_SECOND) as i64)
            }
        };
        if let Ok(op) = op {
            let _ = op.join();
        }
    }

    pub fn read(
        manager: &Manager,
        shown: &mut Option<String>,
        last: &NowPlaying,
    ) -> Result<NowPlaying> {
        let Some((session, id)) = session(manager, shown)? else {
            *shown = None;
            return Ok(NowPlaying::default());
        };
        *shown = Some(id);
        let props = session.TryGetMediaPropertiesAsync()?.join()?;
        let info = session.GetPlaybackInfo()?;
        let playing = info.PlaybackStatus()? == Status::Playing;
        let controls = info.Controls()?;
        let mut now = NowPlaying {
            can_next: controls.IsNextEnabled()?,
            can_previous: back(&session)? != Back::Nothing,
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

    fn back(session: &Session) -> Result<Back> {
        let c = session.GetPlaybackInfo()?.Controls()?;
        Ok(back_action(
            c.IsPreviousEnabled()?,
            c.IsPlaybackPositionEnabled()?,
        ))
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
        let mime = if mime.is_empty() {
            "image/png".into()
        } else {
            mime
        };
        Ok(format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{Back, back_action, pick};

    #[test]
    fn back_restarts_track_when_player_has_nothing_before_it() {
        // Windows Media Player with a single opened file: no previous, but seeking works.
        assert_eq!(back_action(false, true), Back::Restart);
        assert_eq!(back_action(true, true), Back::Skip);
        assert_eq!(back_action(false, false), Back::Nothing);
    }

    fn s(list: &[(&str, bool)]) -> Vec<(String, bool)> {
        list.iter().map(|(id, p)| (id.to_string(), *p)).collect()
    }

    #[test]
    fn playing_session_beats_systems_paused_current() {
        // The bug from the prototype: Windows made a paused player "current".
        let list = s(&[("chrome", true), ("aimp", false)]);
        assert_eq!(pick(&list, Some("aimp"), Some("chrome")), Some(0));
        assert_eq!(pick(&list, Some("aimp"), None), Some(0));
    }

    #[test]
    fn stays_on_shown_player_when_it_is_paused() {
        let list = s(&[("chrome", false), ("aimp", false)]);
        assert_eq!(pick(&list, Some("aimp"), Some("chrome")), Some(0));
    }

    #[test]
    fn switches_when_another_player_starts() {
        let list = s(&[("chrome", false), ("spotify", true)]);
        assert_eq!(pick(&list, Some("chrome"), Some("chrome")), Some(1));
    }

    #[test]
    fn two_playing_keeps_the_shown_one() {
        let list = s(&[("chrome", true), ("spotify", true)]);
        assert_eq!(pick(&list, Some("chrome"), Some("spotify")), Some(1));
    }

    #[test]
    fn shown_player_closed_falls_back_to_current_then_first() {
        let list = s(&[("a", false), ("b", false)]);
        assert_eq!(pick(&list, Some("b"), Some("gone")), Some(1));
        assert_eq!(pick(&list, None, Some("gone")), Some(0));
        assert_eq!(pick(&[], None, None), None);
    }
}
