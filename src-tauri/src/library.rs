//! The music library: only folders the user added, only audio files inside them.
//! A folder is a playlist; its subfolders are albums/collections within it.

use std::path::{Path, PathBuf};

use base64::Engine;
use lofty::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "wav", "m4a", "aac", "ogg", "aif", "aiff"];
const COVER_FILES: &[&str] = &["cover", "folder", "front", "album"];
/// How deep we look for music when deciding whether a folder is worth showing.
const MAX_DEPTH: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub number: Option<u32>,
    /// Seconds.
    pub duration: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderEntry {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderView {
    pub path: String,
    pub name: String,
    pub folders: Vec<FolderEntry>,
    pub tracks: Vec<TrackInfo>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Config {
    folders: Vec<String>,
}

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn is_hidden(path: &Path) -> bool {
    name_of(path).starts_with('.')
}

/// Sorted direct children: (subfolders, audio files).
fn children(dir: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if is_hidden(&path) {
                continue;
            }
            match entry.file_type() {
                Ok(t) if t.is_dir() => dirs.push(path),
                Ok(t) if t.is_file() && is_audio(&path) => files.push(path),
                _ => {}
            }
        }
    }
    dirs.sort_by_key(|p| natural_key(&name_of(p)));
    files.sort_by_key(|p| natural_key(&name_of(p)));
    (dirs, files)
}

/// "2. Song" before "10. Song": numbers compare by value.
pub fn natural_key(name: &str) -> Vec<(u64, String)> {
    let mut key = Vec::new();
    let mut digits = String::new();
    let mut text = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_digit() {
            if !text.is_empty() {
                key.push((u64::MAX, std::mem::take(&mut text)));
            }
            digits.push(c);
        } else {
            if !digits.is_empty() {
                key.push((digits.parse().unwrap_or(u64::MAX), String::new()));
                digits.clear();
            }
            text.push(c);
        }
    }
    if !digits.is_empty() {
        key.push((digits.parse().unwrap_or(u64::MAX), String::new()));
    }
    if !text.is_empty() {
        key.push((u64::MAX, text));
    }
    key
}

/// Whether there is any audio in `dir` or below (folders without music are hidden).
pub fn has_audio(dir: &Path, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    let (dirs, files) = children(dir);
    !files.is_empty() || dirs.iter().any(|d| has_audio(d, depth + 1))
}

/// Every audio file in `dir` and below, folder by folder in display order.
pub fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let (dirs, files) = children(dir);
    out.extend(files);
    for d in dirs {
        collect(&d, depth + 1, out);
    }
}

pub fn read_track(path: &Path) -> TrackInfo {
    let mut info = TrackInfo {
        path: path.to_string_lossy().into_owned(),
        title: path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        ..Default::default()
    };
    if let Ok(file) = lofty::read_from_path(path) {
        info.duration = file.properties().duration().as_secs_f64();
        if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
            if let Some(t) = tag.title().filter(|t| !t.trim().is_empty()) {
                info.title = t.into_owned();
            }
            info.artist = tag.artist().map(|a| a.into_owned()).unwrap_or_default();
            info.album = tag.album().map(|a| a.into_owned()).unwrap_or_default();
            info.number = tag.track();
        }
    }
    info
}

/// Tracks of one folder: by track number when every file has one, else by file name.
fn read_tracks(files: &[PathBuf]) -> Vec<TrackInfo> {
    let mut tracks: Vec<TrackInfo> = files.iter().map(|p| read_track(p)).collect();
    if tracks.iter().all(|t| t.number.is_some()) {
        tracks.sort_by_key(|t| t.number);
    }
    tracks
}

/// Cover art as a `data:` URL: an image next to the music, else art embedded in a track.
pub fn cover_of(path: &Path) -> Option<String> {
    if path.is_file() {
        return embedded_cover(path);
    }
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_lowercase());
            let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase());
            if let (Some(stem), Some(ext)) = (stem, ext)
                && COVER_FILES.contains(&stem.as_str())
                && matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp")
                && let Ok(bytes) = std::fs::read(&p)
            {
                let mime = if ext == "png" { "image/png" } else if ext == "webp" { "image/webp" } else { "image/jpeg" };
                return Some(data_url(mime, &bytes));
            }
        }
    }
    let mut files = Vec::new();
    collect(path, 0, &mut files);
    files.iter().take(3).find_map(|f| embedded_cover(f))
}

fn embedded_cover(path: &Path) -> Option<String> {
    let file = lofty::read_from_path(path).ok()?;
    let tag = file.primary_tag().or_else(|| file.first_tag())?;
    let pic = tag.pictures().first()?;
    let mime = pic.mime_type().map(|m| m.as_str().to_string()).unwrap_or("image/jpeg".into());
    Some(data_url(&mime, pic.data()))
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

// ---- library roots (persisted) ----

fn config_file(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_config_dir().ok()?.join("library.json"))
}

fn load(app: &AppHandle) -> Config {
    config_file(app)
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, config: &Config) {
    if let Some(f) = config_file(app) {
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(t) = serde_json::to_string_pretty(config) {
            let _ = std::fs::write(f, t);
        }
    }
}

/// Only folders inside the user's library may be opened: Carina is not a system file browser.
pub fn inside_library(app: &AppHandle, path: &Path) -> bool {
    load(app).folders.iter().any(|root| path.starts_with(root))
}

fn check(app: &AppHandle, path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if inside_library(app, &path) {
        Ok(path)
    } else {
        Err("Папка не в библиотеке".into())
    }
}

#[tauri::command]
pub async fn library_roots(app: AppHandle) -> Vec<FolderEntry> {
    load(&app)
        .folders
        .iter()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .map(|p| FolderEntry { name: name_of(&p), path: p.to_string_lossy().into_owned() })
        .collect()
}

#[tauri::command]
pub async fn library_add(app: AppHandle, path: String) -> Result<FolderEntry, String> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err("Это не папка".into());
    }
    if !has_audio(&dir, 0) {
        return Err("В папке нет музыки".into());
    }
    let mut config = load(&app);
    if !config.folders.iter().any(|f| Path::new(f) == dir) {
        config.folders.push(path);
        save(&app, &config);
    }
    Ok(FolderEntry { name: name_of(&dir), path: dir.to_string_lossy().into_owned() })
}

#[tauri::command]
pub async fn library_remove(app: AppHandle, path: String) {
    let mut config = load(&app);
    config.folders.retain(|f| f != &path);
    save(&app, &config);
}

#[tauri::command]
pub async fn library_open(app: AppHandle, path: String) -> Result<FolderView, String> {
    let dir = check(&app, &path)?;
    let (dirs, files) = children(&dir);
    Ok(FolderView {
        name: name_of(&dir),
        path,
        folders: dirs
            .into_iter()
            .filter(|d| has_audio(d, 1))
            .map(|d| FolderEntry { name: name_of(&d), path: d.to_string_lossy().into_owned() })
            .collect(),
        tracks: read_tracks(&files),
    })
}

#[tauri::command]
pub async fn library_cover(app: AppHandle, path: String) -> Result<Option<String>, String> {
    Ok(cover_of(&check(&app, &path)?))
}

/// Tracks under `path` whose file path (folders included) or tags match every word of `query`.
#[tauri::command]
pub async fn library_search(app: AppHandle, path: String, query: String) -> Result<Vec<TrackInfo>, String> {
    let dir = check(&app, &path)?;
    let words: Vec<String> = query.to_lowercase().split_whitespace().map(String::from).collect();
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect(&dir, 0, &mut files);
    let mut found = Vec::new();
    for f in files {
        let rel = f.strip_prefix(&dir).unwrap_or(&f).to_string_lossy().to_lowercase();
        if words.iter().all(|w| rel.contains(w)) {
            found.push(read_track(&f));
        } else {
            let t = read_track(&f);
            let text = format!("{} {} {}", t.title, t.artist, t.album).to_lowercase();
            if words.iter().all(|w| text.contains(w)) {
                found.push(t);
            }
        }
        if found.len() >= 200 {
            break;
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_only_by_extension() {
        assert!(is_audio(Path::new("a/Song.MP3")));
        assert!(is_audio(Path::new("a/b.flac")));
        assert!(!is_audio(Path::new("a/cover.jpg")));
        assert!(!is_audio(Path::new("a/project.flp")));
    }

    #[test]
    fn natural_order_puts_2_before_10() {
        let mut names = vec!["10. Ten", "2. Two", "1. One", "Intro"];
        names.sort_by_key(|n| natural_key(n));
        assert_eq!(names, vec!["1. One", "2. Two", "10. Ten", "Intro"]);
    }

    #[test]
    fn folders_without_music_are_hidden_and_collect_walks_in_order() {
        let root = std::env::temp_dir().join(format!("carina-lib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for p in ["A/2. b.mp3", "A/10. c.mp3", "A/Sub/1. d.flac", "Empty/notes.txt", "Art/cover.jpg"] {
            let f = root.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, b"x").unwrap();
        }
        assert!(has_audio(&root.join("A"), 0));
        assert!(!has_audio(&root.join("Empty"), 0));
        assert!(!has_audio(&root.join("Art"), 0));

        let mut files = Vec::new();
        collect(&root.join("A"), 0, &mut files);
        let names: Vec<String> = files.iter().map(|f| name_of(f)).collect();
        assert_eq!(names, vec!["2. b.mp3", "10. c.mp3", "1. d.flac"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
