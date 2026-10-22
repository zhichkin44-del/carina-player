//! Carina's own playback: a queue of files (a folder is a playlist) played through rodio.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use rodio::{Decoder, Player};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::library::{self, TrackInfo};
use crate::media::NowPlaying;

/// "Back" within this many seconds of the start goes to the previous track, later it restarts.
const RESTART_AFTER: f64 = 3.0;
const TICK: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub enum Cmd {
    Play { queue: Vec<PathBuf>, start: usize },
    Toggle,
    Next,
    Previous,
    Jump(usize),
    Seek(f64),
    Volume(f32),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub path: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueState {
    pub items: Vec<QueueItem>,
    pub index: usize,
}

static TX: OnceLock<Mutex<Sender<Cmd>>> = OnceLock::new();
/// Carina has its own queue: the widget shows Carina instead of other players.
static ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn send(cmd: Cmd) {
    if let Some(tx) = TX.get() {
        let _ = tx.lock().unwrap().send(cmd);
    }
}

/// Where "back" goes: restart the track, or step to the previous one.
pub fn previous_index(index: usize, position: f64) -> Option<usize> {
    if position > RESTART_AFTER || index == 0 {
        None
    } else {
        Some(index - 1)
    }
}

pub fn spawn(app: AppHandle) {
    let (tx, rx) = channel();
    let _ = TX.set(Mutex::new(tx));
    std::thread::spawn(move || {
        // The device sink must live on this thread for as long as we play.
        let Ok(sink) = rodio::DeviceSinkBuilder::open_default_sink() else {
            return;
        };
        let player = Player::connect_new(sink.mixer());
        Engine { app, player, queue: Vec::new(), index: 0, track: None, cover: None, loaded: false }
            .run(rx);
    });
}

struct Engine {
    app: AppHandle,
    player: Player,
    queue: Vec<PathBuf>,
    index: usize,
    track: Option<TrackInfo>,
    cover: Option<String>,
    /// A source is in the player (false after the queue ran out).
    loaded: bool,
}

impl Engine {
    fn run(mut self, rx: Receiver<Cmd>) {
        let mut last = NowPlaying::default();
        loop {
            // After a command (seek, skip…) always tell the widget; otherwise only on change —
            // the widget moves the progress line itself between updates.
            let mut force = false;
            match rx.recv_timeout(TICK) {
                Ok(cmd) => {
                    self.handle(cmd);
                    force = true;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            // Track finished on its own: next one, or stop at the end of the folder.
            if self.loaded && self.player.empty() {
                if self.index + 1 < self.queue.len() {
                    self.load(self.index + 1, true);
                } else {
                    self.loaded = false;
                }
            }
            if !active() {
                continue;
            }
            let now = self.now_playing();
            let changed = NowPlaying { position: 0.0, ..now.clone() }
                != NowPlaying { position: 0.0, ..last.clone() };
            if changed || force {
                let _ = self.app.emit("now-playing", &now);
                last = now;
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play { queue, start } if !queue.is_empty() => {
                self.queue = queue;
                ACTIVE.store(true, Ordering::Relaxed);
                self.emit_queue();
                self.load(start.min(self.queue.len() - 1), true);
            }
            Cmd::Play { .. } => {}
            Cmd::Toggle if !self.loaded && !self.queue.is_empty() => self.load(self.index, true),
            Cmd::Toggle => {
                if self.player.is_paused() {
                    self.player.play();
                } else {
                    self.player.pause();
                }
            }
            Cmd::Next if self.index + 1 < self.queue.len() => self.load(self.index + 1, true),
            Cmd::Next => {}
            Cmd::Previous => match previous_index(self.index, self.position()) {
                Some(i) => self.load(i, true),
                None => self.seek(0.0),
            },
            Cmd::Jump(i) if i < self.queue.len() => self.load(i, true),
            Cmd::Jump(_) => {}
            Cmd::Seek(seconds) => self.seek(seconds),
            Cmd::Volume(v) => self.player.set_volume(v.clamp(0.0, 1.0)),
        }
    }

    fn position(&self) -> f64 {
        if self.loaded { self.player.get_pos().as_secs_f64() } else { 0.0 }
    }

    fn seek(&mut self, seconds: f64) {
        if !self.loaded {
            self.load(self.index, false);
        }
        let _ = self.player.try_seek(Duration::from_secs_f64(seconds.max(0.0)));
    }

    /// Put track `index` into the player; unreadable files are skipped forward.
    fn load(&mut self, index: usize, play: bool) {
        let was_paused = self.player.is_paused();
        self.player.clear();
        self.loaded = false;
        for i in index..self.queue.len() {
            let path = self.queue[i].clone();
            if let Some(source) = open(&path) {
                self.index = i;
                self.player.append(source);
                self.loaded = true;
                let mut track = library::read_track(&path);
                // Untagged files: the folder is the best "album" we have.
                if track.album.is_empty()
                    && let Some(folder) = path.parent().and_then(|p| p.file_name())
                {
                    track.album = folder.to_string_lossy().into_owned();
                }
                self.track = Some(track);
                self.cover = library::cover_of(&path).or_else(|| path.parent().and_then(library::cover_of));
                if play || !was_paused {
                    self.player.play();
                }
                self.emit_queue();
                return;
            }
        }
    }

    fn now_playing(&self) -> NowPlaying {
        let Some(track) = &self.track else {
            return NowPlaying::default();
        };
        NowPlaying {
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            playing: self.loaded && !self.player.is_paused(),
            position: self.position(),
            duration: track.duration,
            cover: self.cover.clone(),
            can_next: self.index + 1 < self.queue.len(),
            can_previous: true,
        }
    }

    fn emit_queue(&self) {
        let items = self
            .queue
            .iter()
            .map(|p| QueueItem {
                path: p.to_string_lossy().into_owned(),
                title: p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            })
            .collect();
        let _ = self.app.emit("queue", QueueState { items, index: self.index });
    }
}

pub(crate) fn open(path: &Path) -> Option<Decoder<BufReader<File>>> {
    let file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let hint = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    Decoder::<BufReader<File>>::builder()
        .with_data(BufReader::new(file))
        .with_byte_len(len)
        .with_seekable(true)
        .with_gapless(true)
        .with_hint(&hint)
        .build()
        .ok()
}

// ---- commands ----

/// Play a folder (with its subfolders) starting at `start` (a track path), or from the top.
#[tauri::command]
pub async fn play_folder(app: AppHandle, path: String, start: Option<String>) -> Result<(), String> {
    let dir = PathBuf::from(&path);
    if !library::inside_library(&app, &dir) {
        return Err("Папка не в библиотеке".into());
    }
    let mut queue = Vec::new();
    library::collect(&dir, 0, &mut queue);
    let start = start
        .and_then(|s| queue.iter().position(|p| p == Path::new(&s)))
        .unwrap_or(0);
    send(Cmd::Play { queue, start });
    Ok(())
}

/// Play an explicit list (search results) starting at `start`.
#[tauri::command]
pub async fn play_tracks(app: AppHandle, paths: Vec<String>, start: usize) -> Result<(), String> {
    let queue: Vec<PathBuf> = paths
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| library::inside_library(&app, p))
        .collect();
    send(Cmd::Play { queue, start });
    Ok(())
}

#[tauri::command]
pub fn queue_jump(index: usize) {
    send(Cmd::Jump(index));
}

#[tauri::command]
pub fn set_volume(volume: f32) {
    send(Cmd::Volume(volume));
}

#[cfg(test)]
mod tests {
    use super::previous_index;

    #[test]
    fn back_restarts_after_three_seconds_else_steps_back() {
        assert_eq!(previous_index(4, 10.0), None);
        assert_eq!(previous_index(4, 1.0), Some(3));
        assert_eq!(previous_index(0, 1.0), None);
    }
}
