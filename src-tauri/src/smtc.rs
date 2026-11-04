//! Windows media integration: Carina appears in the system media flyout (volume overlay, lock
//! screen) and answers the keyboard's media keys and headset buttons.

use std::sync::Mutex;

use base64::Engine;
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
    SystemMediaTransportControls as Controls, SystemMediaTransportControlsButton as Button,
    SystemMediaTransportControlsButtonPressedEventArgs as Pressed,
    SystemMediaTransportControlsTimelineProperties as Timeline,
};
use windows::Storage::Streams::{
    DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
use windows::core::{HSTRING, Result};

use crate::engine::{self, Cmd};
use crate::media::NowPlaying;

const TICKS_PER_SECOND: f64 = 10_000_000.0;

struct State {
    controls: Controls,
    /// Title + artist + cover of what the flyout shows, to skip redundant updates.
    shown: (String, String, Option<usize>),
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Register the widget window with the system media controls.
pub fn init(hwnd: HWND) -> Result<()> {
    let interop = windows::core::factory::<Controls, ISystemMediaTransportControlsInterop>()?;
    let controls: Controls = unsafe { interop.GetForWindow(hwnd)? };
    controls.SetIsEnabled(true)?;
    controls.SetIsPlayEnabled(true)?;
    controls.SetIsPauseEnabled(true)?;
    controls.SetIsNextEnabled(true)?;
    controls.SetIsPreviousEnabled(true)?;
    controls.SetPlaybackStatus(MediaPlaybackStatus::Closed)?;
    controls.ButtonPressed(&TypedEventHandler::<Controls, Pressed>::new(|_, args| {
        if let Some(args) = args.as_ref() {
            match args.Button()? {
                Button::Play | Button::Pause => engine::send(Cmd::Toggle),
                Button::Next => engine::send(Cmd::Next),
                Button::Previous => engine::send(Cmd::Previous),
                _ => {}
            }
        }
        Ok(())
    }))?;
    controls.PlaybackPositionChangeRequested(&TypedEventHandler::<
        Controls,
        PlaybackPositionChangeRequestedEventArgs,
    >::new(|_, args| {
        if let Some(args) = args.as_ref() {
            let ticks = args.RequestedPlaybackPosition()?.Duration;
            engine::send(Cmd::Seek(ticks as f64 / TICKS_PER_SECOND));
        }
        Ok(())
    }))?;
    *STATE.lock().unwrap() = Some(State {
        controls,
        shown: Default::default(),
    });
    Ok(())
}

fn span(seconds: f64) -> TimeSpan {
    TimeSpan {
        Duration: (seconds.max(0.0) * TICKS_PER_SECOND) as i64,
    }
}

/// Show Carina's current track in the system flyout. Called whenever the engine's state changes.
pub fn update(now: &NowPlaying) {
    let mut guard = STATE.lock().unwrap();
    let Some(state) = guard.as_mut() else {
        return;
    };
    let _ = apply(state, now);
}

fn apply(state: &mut State, now: &NowPlaying) -> Result<()> {
    let controls = &state.controls;
    if now.title.is_empty() {
        controls.SetPlaybackStatus(MediaPlaybackStatus::Closed)?;
        state.shown = Default::default();
        return Ok(());
    }
    controls.SetPlaybackStatus(if now.playing {
        MediaPlaybackStatus::Playing
    } else {
        MediaPlaybackStatus::Paused
    })?;
    controls.SetIsNextEnabled(now.can_next)?;

    let shown = (
        now.title.clone(),
        now.artist.clone(),
        now.cover.as_ref().map(|c| c.len()),
    );
    if shown != state.shown {
        let updater = controls.DisplayUpdater()?;
        updater.SetType(MediaPlaybackType::Music)?;
        let music = updater.MusicProperties()?;
        music.SetTitle(&HSTRING::from(&now.title))?;
        music.SetArtist(&HSTRING::from(&now.artist))?;
        music.SetAlbumTitle(&HSTRING::from(&now.album))?;
        match now.cover.as_deref().and_then(cover_stream) {
            Some(cover) => updater.SetThumbnail(&cover)?,
            None => updater.SetThumbnail(None)?,
        }
        updater.Update()?;
        state.shown = shown;
    }

    let timeline = Timeline::new()?;
    timeline.SetStartTime(span(0.0))?;
    timeline.SetEndTime(span(now.duration))?;
    timeline.SetMinSeekTime(span(0.0))?;
    timeline.SetMaxSeekTime(span(now.duration))?;
    timeline.SetPosition(span(now.position))?;
    controls.UpdateTimelineProperties(&timeline)?;
    Ok(())
}

/// `data:` URL → stream the flyout can show.
fn cover_stream(data_url: &str) -> Option<RandomAccessStreamReference> {
    let (_, b64) = data_url.split_once(',')?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let stream = InMemoryRandomAccessStream::new().ok()?;
    let writer = DataWriter::CreateDataWriter(&stream).ok()?;
    writer.WriteBytes(&bytes).ok()?;
    writer.StoreAsync().ok()?.join().ok()?;
    writer.DetachStream().ok()?;
    stream.Seek(0).ok()?;
    RandomAccessStreamReference::CreateFromStream(&stream).ok()
}
