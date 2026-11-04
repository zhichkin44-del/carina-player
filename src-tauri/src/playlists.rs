//! The user's own playlists and Favorites (♥): lists of tracks from anywhere in the library.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::library::{self, TrackInfo};

/// Favorites is a playlist with a fixed id.
pub const FAVORITES: &str = "favorites";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub tracks: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    favorites: Vec<String>,
    playlists: Vec<Playlist>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSummary {
    pub id: String,
    pub name: String,
    pub count: usize,
}

/// One writer at a time: commands run concurrently on the async pool.
static LOCK: Mutex<()> = Mutex::new(());

fn file(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_config_dir().ok()?.join("playlists.json"))
}

fn load(app: &AppHandle) -> Store {
    file(app)
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, store: &Store) {
    if let Some(f) = file(app) {
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(t) = serde_json::to_string_pretty(store) {
            let _ = std::fs::write(f, t);
        }
    }
}

/// Load, change, save — under the lock.
fn edit<T>(app: &AppHandle, f: impl FnOnce(&mut Store) -> T) -> T {
    let _guard = LOCK.lock().unwrap();
    let mut store = load(app);
    let out = f(&mut store);
    save(app, &store);
    out
}

fn tracks_of<'a>(store: &'a mut Store, id: &str) -> Option<&'a mut Vec<String>> {
    if id == FAVORITES {
        Some(&mut store.favorites)
    } else {
        store
            .playlists
            .iter_mut()
            .find(|p| p.id == id)
            .map(|p| &mut p.tracks)
    }
}

/// Append paths that aren't in the list yet; returns how many were added.
pub fn add_unique(list: &mut Vec<String>, paths: &[String]) -> usize {
    let before = list.len();
    for p in paths {
        if !list.contains(p) {
            list.push(p.clone());
        }
    }
    list.len() - before
}

fn new_id(store: &Store) -> String {
    (1..)
        .map(|n| format!("p{n}"))
        .find(|id| !store.playlists.iter().any(|p| &p.id == id))
        .unwrap()
}

#[tauri::command]
pub async fn playlists(app: AppHandle) -> Vec<PlaylistSummary> {
    let store = load(&app);
    std::iter::once(PlaylistSummary {
        id: FAVORITES.into(),
        name: "Favorites".into(),
        count: store.favorites.len(),
    })
    .chain(store.playlists.iter().map(|p| PlaylistSummary {
        id: p.id.clone(),
        name: p.name.clone(),
        count: p.tracks.len(),
    }))
    .collect()
}

/// Tracks of a playlist; files that no longer exist are left out.
#[tauri::command]
pub async fn playlist_tracks(app: AppHandle, id: String) -> Vec<TrackInfo> {
    let mut store = load(&app);
    let paths = tracks_of(&mut store, &id).cloned().unwrap_or_default();
    let tracks = paths
        .iter()
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .map(|p| library::track_info(&app, &p))
        .collect();
    library::flush_cache(&app);
    tracks
}

#[tauri::command]
pub async fn playlist_create(app: AppHandle, name: String, paths: Vec<String>) -> PlaylistSummary {
    edit(&app, |store| {
        let id = new_id(store);
        // The widget always sends a name in its language; this English fallback is only for empty input.
        let name = if name.trim().is_empty() {
            format!("Playlist {}", store.playlists.len() + 1)
        } else {
            name.trim().to_string()
        };
        store.playlists.push(Playlist {
            id: id.clone(),
            name: name.clone(),
            tracks: paths.clone(),
        });
        PlaylistSummary {
            id,
            name,
            count: paths.len(),
        }
    })
}

#[tauri::command]
pub async fn playlist_rename(app: AppHandle, id: String, name: String) {
    edit(&app, |store| {
        if let Some(p) = store.playlists.iter_mut().find(|p| p.id == id)
            && !name.trim().is_empty()
        {
            p.name = name.trim().to_string();
        }
    });
}

#[tauri::command]
pub async fn playlist_delete(app: AppHandle, id: String) {
    edit(&app, |store| store.playlists.retain(|p| p.id != id));
}

#[tauri::command]
pub async fn playlist_add(app: AppHandle, id: String, paths: Vec<String>) -> usize {
    edit(&app, |store| {
        tracks_of(store, &id)
            .map(|t| add_unique(t, &paths))
            .unwrap_or(0)
    })
}

#[tauri::command]
pub async fn playlist_remove(app: AppHandle, id: String, path: String) {
    edit(&app, |store| {
        if let Some(t) = tracks_of(store, &id) {
            t.retain(|p| p != &path);
        }
    });
}

#[tauri::command]
pub async fn playlist_move(app: AppHandle, id: String, from: usize, to: usize) {
    edit(&app, |store| {
        if let Some(t) = tracks_of(store, &id)
            && from < t.len()
            && to < t.len()
        {
            let item = t.remove(from);
            t.insert(to, item);
        }
    });
}

/// Cover for a playlist tile: the first track (or its folder) that has art.
#[tauri::command]
pub async fn playlist_cover(app: AppHandle, id: String) -> Option<String> {
    let mut store = load(&app);
    let paths = tracks_of(&mut store, &id).cloned().unwrap_or_default();
    paths.iter().take(8).map(PathBuf::from).find_map(|p| {
        library::cover_of(&p).or_else(|| p.parent().and_then(library::cover_of))
    })
}

/// ♥ on/off; returns the new state.
#[tauri::command]
pub async fn favorite_toggle(app: AppHandle, path: String) -> bool {
    edit(&app, |store| {
        if let Some(i) = store.favorites.iter().position(|p| p == &path) {
            store.favorites.remove(i);
            false
        } else {
            store.favorites.push(path);
            true
        }
    })
}

/// All favorite paths, so the page can draw filled hearts.
#[tauri::command]
pub async fn favorites(app: AppHandle) -> Vec<String> {
    load(&app).favorites
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_skips_duplicates() {
        let mut list = vec!["a".to_string()];
        assert_eq!(
            add_unique(&mut list, &["a".into(), "b".into(), "b".into()]),
            1
        );
        assert_eq!(list, vec!["a", "b"]);
    }

    #[test]
    fn new_ids_fill_gaps() {
        let store = Store {
            favorites: vec![],
            playlists: vec![
                Playlist {
                    id: "p1".into(),
                    ..Default::default()
                },
                Playlist {
                    id: "p3".into(),
                    ..Default::default()
                },
            ],
        };
        assert_eq!(new_id(&store), "p2");
    }
}
