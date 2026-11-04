//! Carina's own playback: a queue of files (a folder is a playlist) played through rodio.
//! Follows the system's default output device (headphones plugged in or out), and remembers the
//! queue and position so the next start continues where the user stopped.

use std::cell::Cell;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::{Decoder, MixerDeviceSink, Player};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::library::{self, TrackInfo};
use crate::media::NowPlaying;

/// "Back" within this many seconds of the start goes to the previous track, later it restarts.
const RESTART_AFTER: f64 = 3.0;
const TICK: Duration = Duration::from_millis(250);
/// How often we look for a changed output device and save the position.
const DEVICE_CHECK: Duration = Duration::from_secs(1);
const SAVE_EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

#[derive(Debug)]
pub enum Cmd {
    Play { queue: Vec<PathBuf>, start: usize },
    Toggle,
    Next,
    Previous,
    Jump(usize),
    Seek(f64),
    Volume(f32),
    Shuffle(bool),
    Repeat(Repeat),
    PlayNext(Vec<PathBuf>),
    Enqueue(Vec<PathBuf>),
    Remove(usize),
    Move { from: usize, to: usize },
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
    pub shuffle: bool,
    pub repeat: Repeat,
}

/// What survives a restart.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Session {
    queue: Vec<PathBuf>,
    index: usize,
    position: f64,
    shuffle: bool,
    repeat: Repeat,
    /// Queue order before shuffling, to restore it when shuffle is switched off.
    unshuffled: Vec<PathBuf>,
    /// Sessions saved before "repeat all" became the default (23.10.2026) get it switched on once.
    #[serde(default)]
    repeat_default_applied: bool,
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

// ---- pure queue logic ----

/// Where "back" goes: restart the track, or step to the previous one.
pub fn previous_index(index: usize, position: f64) -> Option<usize> {
    if position > RESTART_AFTER || index == 0 {
        None
    } else {
        Some(index - 1)
    }
}

/// The track after `index` (on "next" or when one finishes); `None` stops playback.
pub fn next_index(index: usize, len: usize, repeat: Repeat, finished: bool) -> Option<usize> {
    if finished && repeat == Repeat::One {
        Some(index)
    } else if index + 1 < len {
        Some(index + 1)
    } else if repeat != Repeat::Off && len > 0 {
        Some(0)
    } else {
        None
    }
}

/// Fisher–Yates with a small xorshift generator (no dependency, deterministic for tests).
pub fn shuffled<T>(mut items: Vec<T>, mut seed: u64) -> Vec<T> {
    seed |= 1;
    for i in (1..items.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        items.swap(i, (seed % (i as u64 + 1)) as usize);
    }
    items
}

/// Remove item `at`, keeping `index` on the same track (or the one that took its place).
pub fn remove_at<T>(queue: &mut Vec<T>, index: usize, at: usize) -> usize {
    if at >= queue.len() {
        return index;
    }
    queue.remove(at);
    if at < index {
        index - 1
    } else {
        index.min(queue.len().saturating_sub(1))
    }
}

/// Move item `from` to `to`, keeping `index` on the same track.
pub fn move_item<T>(queue: &mut Vec<T>, index: usize, from: usize, to: usize) -> usize {
    if from >= queue.len() || to >= queue.len() || from == to {
        return index;
    }
    let item = queue.remove(from);
    queue.insert(to, item);
    if index == from {
        to
    } else if from < index && to >= index {
        index - 1
    } else if from > index && to <= index {
        index + 1
    } else {
        index
    }
}

fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(7)
}

// ---- output device ----

fn default_device_id() -> Option<rodio::cpal::DeviceId> {
    rodio::cpal::default_host()
        .default_output_device()?
        .id()
        .ok()
}

/// Open the default output; `failed` is raised if the device disappears mid-stream.
fn open_output(failed: &Arc<AtomicBool>) -> Option<MixerDeviceSink> {
    failed.store(false, Ordering::Relaxed);
    let flag = failed.clone();
    let mut sink = rodio::DeviceSinkBuilder::from_default_device()
        .ok()?
        .with_error_callback(move |_| flag.store(true, Ordering::Relaxed))
        .open_sink_or_fallback()
        .ok()?;
    sink.log_on_drop(false);
    Some(sink)
}

pub fn spawn(app: AppHandle) {
    let (tx, rx) = channel();
    let _ = TX.set(Mutex::new(tx));
    std::thread::spawn(move || {
        // The device sink must live on this thread for as long as we play.
        let failed = Arc::new(AtomicBool::new(false));
        let Some(sink) = open_output(&failed) else {
            return;
        };
        let player = Player::connect_new(sink.mixer());
        let mut engine = Engine {
            app,
            device: default_device_id(),
            sink,
            failed,
            player,
            volume: 1.0,
            queue: Vec::new(),
            unshuffled: Vec::new(),
            index: 0,
            shuffle: false,
            // A folder keeps playing in a loop unless the user switches repeat off.
            repeat: Repeat::All,
            track: None,
            cover: None,
            loaded: false,
            stale_position: Cell::new(None),
        };
        engine.restore();
        engine.run(rx);
    });
}

struct Engine {
    app: AppHandle,
    device: Option<rodio::cpal::DeviceId>,
    sink: MixerDeviceSink,
    failed: Arc<AtomicBool>,
    player: Player,
    volume: f32,
    queue: Vec<PathBuf>,
    unshuffled: Vec<PathBuf>,
    index: usize,
    shuffle: bool,
    repeat: Repeat,
    track: Option<TrackInfo>,
    cover: Option<String>,
    /// A source is in the player (false after the queue ran out).
    loaded: bool,
    /// rodio keeps reporting the previous track's position until the new one has been polled;
    /// while `get_pos` still returns this value, the new track is at 0.
    stale_position: Cell<Option<Duration>>,
}

impl Engine {
    fn run(mut self, rx: Receiver<Cmd>) {
        let mut last = NowPlaying::default();
        let mut device_checked = Instant::now();
        let mut saved = Instant::now();
        loop {
            // After a command (seek, skip…) always tell the widget; otherwise only on change —
            // the widget moves the progress line itself between updates.
            let mut force = false;
            match rx.recv_timeout(TICK) {
                Ok(cmd) => {
                    self.handle(cmd);
                    force = true;
                    self.save();
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            // Track finished on its own.
            if self.loaded && self.player.empty() {
                match next_index(self.index, self.queue.len(), self.repeat, true) {
                    Some(i) => self.load(i, true),
                    None => {
                        self.loaded = false;
                        self.index = 0;
                    }
                }
                force = true;
                self.save();
            }
            if device_checked.elapsed() >= DEVICE_CHECK {
                device_checked = Instant::now();
                self.follow_device();
            }
            if self.loaded && !self.player.is_paused() && saved.elapsed() >= SAVE_EVERY {
                saved = Instant::now();
                self.save();
            }
            if !active() {
                continue;
            }
            let now = self.now_playing();
            let changed = NowPlaying {
                position: 0.0,
                ..now.clone()
            } != NowPlaying {
                position: 0.0,
                ..last.clone()
            };
            if changed || force {
                let _ = self.app.emit("now-playing", &now);
                #[cfg(windows)]
                crate::smtc::update(&now);
                last = now;
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play { queue, start } if !queue.is_empty() => {
                let start = start.min(queue.len() - 1);
                ACTIVE.store(true, Ordering::Relaxed);
                if self.shuffle {
                    // The chosen track plays first, the rest in random order.
                    self.unshuffled = queue.clone();
                    let mut rest = queue;
                    let first = rest.remove(start);
                    self.queue = std::iter::once(first)
                        .chain(shuffled(rest, seed()))
                        .collect();
                    self.load(0, true);
                } else {
                    self.unshuffled.clear();
                    self.queue = queue;
                    self.load(start, true);
                }
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
            Cmd::Next => {
                if let Some(i) = next_index(self.index, self.queue.len(), self.repeat, false) {
                    self.load(i, true);
                }
            }
            Cmd::Previous => match previous_index(self.index, self.position()) {
                Some(i) => self.load(i, true),
                None => self.seek(0.0),
            },
            Cmd::Jump(i) if i < self.queue.len() => self.load(i, true),
            Cmd::Jump(_) => {}
            Cmd::Seek(seconds) => self.seek(seconds),
            Cmd::Volume(v) => {
                self.volume = v.clamp(0.0, 1.0);
                self.player.set_volume(self.volume);
            }
            Cmd::Shuffle(on) => self.set_shuffle(on),
            Cmd::Repeat(mode) => {
                self.repeat = mode;
                self.emit_queue();
            }
            Cmd::PlayNext(paths) => {
                let at = if self.queue.is_empty() {
                    0
                } else {
                    self.index + 1
                };
                let start_now = self.queue.is_empty();
                self.queue.splice(at..at, paths);
                self.after_edit(start_now);
            }
            Cmd::Enqueue(paths) => {
                let start_now = self.queue.is_empty();
                self.queue.extend(paths);
                self.after_edit(start_now);
            }
            Cmd::Remove(at) => {
                let current = at == self.index;
                self.index = remove_at(&mut self.queue, self.index, at);
                if self.queue.is_empty() {
                    self.player.clear();
                    self.loaded = false;
                    self.track = None;
                    ACTIVE.store(false, Ordering::Relaxed);
                    let _ = self.app.emit("now-playing", NowPlaying::default());
                    #[cfg(windows)]
                    crate::smtc::update(&NowPlaying::default());
                } else if current {
                    let playing = self.loaded && !self.player.is_paused();
                    self.load(self.index, playing);
                }
                self.emit_queue();
            }
            Cmd::Move { from, to } => {
                self.index = move_item(&mut self.queue, self.index, from, to);
                self.emit_queue();
            }
        }
    }

    /// After adding to the queue: start playing if it was empty, else just show the new queue.
    fn after_edit(&mut self, start_now: bool) {
        if start_now && !self.queue.is_empty() {
            ACTIVE.store(true, Ordering::Relaxed);
            self.load(0, true);
        } else {
            self.emit_queue();
        }
    }

    fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle {
            return;
        }
        self.shuffle = on;
        if self.queue.is_empty() {
            self.emit_queue();
            return;
        }
        let current = self.queue[self.index].clone();
        if on {
            // Played and current tracks stay; what comes next is shuffled.
            self.unshuffled = self.queue.clone();
            let rest = self.queue.split_off(self.index + 1);
            self.queue.extend(shuffled(rest, seed()));
        } else if !self.unshuffled.is_empty() {
            self.queue = std::mem::take(&mut self.unshuffled);
            self.index = self.queue.iter().position(|p| *p == current).unwrap_or(0);
        }
        self.emit_queue();
    }

    fn position(&self) -> f64 {
        if !self.loaded {
            return 0.0;
        }
        let pos = self.player.get_pos();
        match self.stale_position.get() {
            Some(stale) if pos == stale => 0.0,
            _ => {
                self.stale_position.set(None);
                pos.as_secs_f64()
            }
        }
    }

    fn seek(&mut self, seconds: f64) {
        if !self.loaded {
            self.load(self.index, false);
        }
        let _ = self
            .player
            .try_seek(Duration::from_secs_f64(seconds.max(0.0)));
    }

    /// Headphones plugged in or out (or the device vanished): move playback to the new default
    /// output without losing the track, position or pause state.
    fn follow_device(&mut self) {
        let now = default_device_id();
        if now == self.device && !self.failed.load(Ordering::Relaxed) {
            return;
        }
        let Some(sink) = open_output(&self.failed) else {
            return; // no output at all right now; try again next second
        };
        let position = self.position();
        let playing = self.loaded && !self.player.is_paused();
        self.player.clear();
        self.device = now;
        self.sink = sink;
        self.player = Player::connect_new(self.sink.mixer());
        self.player.set_volume(self.volume);
        if self.loaded {
            self.load(self.index, playing);
            if !playing {
                self.player.pause();
            }
            let _ = self.player.try_seek(Duration::from_secs_f64(position));
        }
    }

    /// Put track `index` into the player; unreadable files are skipped forward.
    fn load(&mut self, index: usize, play: bool) {
        self.player.clear();
        self.loaded = false;
        self.stale_position.set(Some(self.player.get_pos()).filter(|p| !p.is_zero()));
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
                self.cover =
                    library::cover_of(&path).or_else(|| path.parent().and_then(library::cover_of));
                if play {
                    self.player.play();
                } else {
                    self.player.pause();
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
            can_next: next_index(self.index, self.queue.len(), self.repeat, false).is_some(),
            can_previous: true,
        }
    }

    fn emit_queue(&self) {
        let items = self
            .queue
            .iter()
            .map(|p| QueueItem {
                path: p.to_string_lossy().into_owned(),
                title: p
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            })
            .collect();
        let _ = self.app.emit(
            "queue",
            QueueState {
                items,
                index: self.index,
                shuffle: self.shuffle,
                repeat: self.repeat,
            },
        );
    }

    // ---- remembering the session ----

    fn session_file(&self) -> Option<PathBuf> {
        Some(self.app.path().app_config_dir().ok()?.join("session.json"))
    }

    fn save(&self) {
        let Some(file) = self.session_file() else {
            return;
        };
        let session = Session {
            queue: self.queue.clone(),
            index: self.index,
            position: self.position(),
            shuffle: self.shuffle,
            repeat: self.repeat,
            unshuffled: self.unshuffled.clone(),
            repeat_default_applied: true,
        };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(&session) {
            let _ = std::fs::write(file, text);
        }
    }

    /// Continue where the user stopped: same queue and track, paused at the same second.
    fn restore(&mut self) {
        let Some(session) = self
            .session_file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str::<Session>(&t).ok())
        else {
            return;
        };
        let queue: Vec<PathBuf> = session.queue.into_iter().filter(|p| p.is_file()).collect();
        if queue.is_empty() {
            return;
        }
        self.index = session.index.min(queue.len() - 1);
        self.queue = queue;
        self.unshuffled = session
            .unshuffled
            .into_iter()
            .filter(|p| p.is_file())
            .collect();
        self.shuffle = session.shuffle;
        self.repeat = if session.repeat_default_applied { session.repeat } else { Repeat::All };
        ACTIVE.store(true, Ordering::Relaxed);
        self.load(self.index, false);
        let _ = self
            .player
            .try_seek(Duration::from_secs_f64(session.position.max(0.0)));
        let _ = self.app.emit("now-playing", self.now_playing());
    }
}

pub(crate) fn open(path: &Path) -> Option<Decoder<BufReader<File>>> {
    let file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let hint = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    Decoder::<BufReader<File>>::builder()
        .with_data(BufReader::new(file))
        .with_byte_len(len)
        .with_seekable(true)
        .with_gapless(true)
        .with_hint(&hint)
        .build()
        .ok()
}

/// Files handed to Carina by Windows (command line or a second launch).
pub fn open_paths(args: impl IntoIterator<Item = String>) {
    let paths: Vec<PathBuf> = args.into_iter().map(PathBuf::from).filter(|p| p.exists()).collect();
    let (queue, start) = library::queue_for_opened(&paths);
    if !queue.is_empty() {
        send(Cmd::Play { queue, start });
    }
}

// ---- commands ----

fn library_paths(app: &AppHandle, paths: Vec<String>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| library::inside_library(app, p))
        .collect()
}

/// Play a folder (with its subfolders) starting at `start` (a track path), or from the top.
#[tauri::command]
pub async fn play_folder(
    app: AppHandle,
    path: String,
    start: Option<String>,
) -> Result<(), String> {
    let dir = PathBuf::from(&path);
    // Translated by exact text in the widget (`backendErrors`, src/routes/+page.svelte).
    if !library::inside_library(&app, &dir) {
        return Err("Folder is not in the library".into());
    }
    let mut queue = Vec::new();
    library::collect(&dir, 0, &mut queue);
    let start = start
        .and_then(|s| queue.iter().position(|p| p == Path::new(&s)))
        .unwrap_or(0);
    send(Cmd::Play { queue, start });
    Ok(())
}

/// Play an explicit list (search results, a playlist) starting at `start`.
#[tauri::command]
pub async fn play_tracks(app: AppHandle, paths: Vec<String>, start: usize) {
    send(Cmd::Play {
        queue: library_paths(&app, paths),
        start,
    });
}

#[tauri::command]
pub async fn queue_play_next(app: AppHandle, paths: Vec<String>) {
    send(Cmd::PlayNext(library_paths(&app, paths)));
}

#[tauri::command]
pub async fn queue_add(app: AppHandle, paths: Vec<String>) {
    send(Cmd::Enqueue(library_paths(&app, paths)));
}

#[tauri::command]
pub fn queue_remove(index: usize) {
    send(Cmd::Remove(index));
}

#[tauri::command]
pub fn queue_move(from: usize, to: usize) {
    send(Cmd::Move { from, to });
}

/// Audio files dropped on the widget play like files opened from Explorer.
#[tauri::command]
pub fn open_files(paths: Vec<String>) {
    open_paths(paths);
}

#[tauri::command]
pub fn queue_jump(index: usize) {
    send(Cmd::Jump(index));
}

#[tauri::command]
pub fn set_volume(volume: f32) {
    send(Cmd::Volume(volume));
}

#[tauri::command]
pub fn set_shuffle(on: bool) {
    send(Cmd::Shuffle(on));
}

#[tauri::command]
pub fn set_repeat(mode: Repeat) {
    send(Cmd::Repeat(mode));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_restarts_after_three_seconds_else_steps_back() {
        assert_eq!(previous_index(4, 10.0), None);
        assert_eq!(previous_index(4, 1.0), Some(3));
        assert_eq!(previous_index(0, 1.0), None);
    }

    #[test]
    fn next_respects_repeat_modes() {
        assert_eq!(next_index(1, 3, Repeat::Off, false), Some(2));
        assert_eq!(next_index(2, 3, Repeat::Off, true), None);
        assert_eq!(next_index(2, 3, Repeat::All, true), Some(0));
        assert_eq!(next_index(1, 3, Repeat::One, true), Some(1));
        // Pressing "next" on repeat-one still moves on.
        assert_eq!(next_index(1, 3, Repeat::One, false), Some(2));
        assert_eq!(next_index(0, 0, Repeat::All, true), None);
    }

    #[test]
    fn shuffle_is_a_permutation_and_depends_on_seed() {
        let a = shuffled((0..20).collect::<Vec<_>>(), 42);
        let mut sorted = a.clone();
        sorted.sort();
        assert_eq!(sorted, (0..20).collect::<Vec<_>>());
        assert_ne!(a, (0..20).collect::<Vec<_>>());
        assert_ne!(a, shuffled((0..20).collect::<Vec<_>>(), 1000));
    }

    #[test]
    fn removing_keeps_the_current_track() {
        let mut q = vec!['a', 'b', 'c', 'd'];
        assert_eq!(remove_at(&mut q, 2, 0), 1); // before current
        assert_eq!(q, vec!['b', 'c', 'd']);
        assert_eq!(remove_at(&mut q, 1, 2), 1); // after current
        assert_eq!(remove_at(&mut q, 1, 1), 0); // current and last: falls back to the one before
        assert_eq!(q, vec!['b']);
        assert_eq!(remove_at(&mut q, 0, 0), 0);
        assert!(q.is_empty());
    }

    #[test]
    fn moving_keeps_the_current_track() {
        let mut q = vec!['a', 'b', 'c', 'd'];
        assert_eq!(move_item(&mut q, 1, 1, 3), 3); // current moved
        assert_eq!(q, vec!['a', 'c', 'd', 'b']);
        assert_eq!(move_item(&mut q, 3, 0, 3), 2); // from before to after current
        assert_eq!(q, vec!['c', 'd', 'b', 'a']);
        assert_eq!(move_item(&mut q, 2, 3, 0), 3); // from after to before current
        assert_eq!(q, vec!['a', 'c', 'd', 'b']);
    }
}
