//! Silent self-check of the player on real files: tags, cover, decoding, seeking, duration.
//! Run: `cargo run --release --example check -- <music folder>`.

use std::path::Path;
use std::time::{Duration, Instant};

use rodio::Source;

use crate::{engine, library};

/// One file's result; `problems` is empty when everything works.
pub struct FileReport {
    pub name: String,
    pub info: String,
    pub problems: Vec<String>,
}

pub fn file(path: &Path) -> FileReport {
    let mut problems = Vec::new();
    let t = Instant::now();
    let track = library::read_track(path);
    let tag_ms = t.elapsed().as_millis();
    if track.duration <= 0.0 {
        problems.push("tags: no duration".into());
    }
    let cover = library::cover_of(path).is_some();

    let t = Instant::now();
    let Some(mut source) = engine::open(path) else {
        return FileReport {
            name: name(path),
            info: String::new(),
            problems: vec!["decoder: cannot open".into()],
        };
    };
    let open_ms = t.elapsed().as_millis();
    let (rate, channels) = (source.sample_rate(), source.channels());
    let decoded = source.total_duration().map(|d| d.as_secs_f64());

    // Decode the first 2 s, then seek to the middle and decode 2 s more.
    let two_seconds = (rate.get() as usize) * (channels.get() as usize) * 2;
    let head = source.by_ref().take(two_seconds).count();
    if head < two_seconds / 2 {
        problems.push(format!("decode: only {head} samples at start"));
    }
    let middle = Duration::from_secs_f64(track.duration / 2.0);
    let t = Instant::now();
    match source.try_seek(middle) {
        Ok(()) => {
            let after = source.by_ref().take(two_seconds).count();
            if after < two_seconds / 2 {
                problems.push(format!("seek: only {after} samples after seeking"));
            }
        }
        Err(e) => problems.push(format!("seek: {e}")),
    }
    let seek_ms = t.elapsed().as_millis();

    if let Some(d) = decoded
        && (d - track.duration).abs() > 1.0
    {
        problems.push(format!(
            "duration: tags {:.1}s vs decoder {d:.1}s",
            track.duration
        ));
    }
    FileReport {
        name: name(path),
        info: format!(
            "{rate} Hz {channels}ch {:.0}s | '{}' / '{}' | cover {} | tags {tag_ms}ms open {open_ms}ms seek {seek_ms}ms",
            track.duration,
            track.title,
            if track.artist.is_empty() {
                "-"
            } else {
                &track.artist
            },
            if cover { "yes" } else { "no" },
        ),
        problems,
    }
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Check every audio file under `dir`; returns (files, failed).
pub fn folder(dir: &Path) -> (usize, usize) {
    let mut files = Vec::new();
    library::collect(dir, 0, &mut files);
    let mut failed = 0;
    for f in &files {
        let r = file(f);
        let mark = if r.problems.is_empty() { "ok " } else { "ERR" };
        println!("{mark} {} | {}", r.name, r.info);
        for p in &r.problems {
            println!("      ! {p}");
        }
        failed += usize::from(!r.problems.is_empty());
    }
    (files.len(), failed)
}
