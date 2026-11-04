//! Diagnostic: list system media sessions and which controls they allow (read-only).
#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager as Manager;
    let manager = Manager::RequestAsync()?.join()?;
    let current = manager
        .GetCurrentSession()
        .and_then(|s| s.SourceAppUserModelId())
        .ok();
    println!("current: {current:?}");
    let list = manager.GetSessions()?;
    for i in 0..list.Size()? {
        let s = list.GetAt(i)?;
        let id = s.SourceAppUserModelId()?;
        let Ok(props) = s.TryGetMediaPropertiesAsync().and_then(|op| op.join()) else {
            println!("{id} | (unreadable — closed session)");
            continue;
        };
        let info = s.GetPlaybackInfo()?;
        let c = info.Controls()?;
        println!(
            "{} | {:?} | '{}' | next={} prev={} play/pause={} seek={}",
            id,
            info.PlaybackStatus()?,
            props.Title()?,
            c.IsNextEnabled()?,
            c.IsPreviousEnabled()?,
            c.IsPlayPauseToggleEnabled()?,
            c.IsPlaybackPositionEnabled()?,
        );
    }
    Ok(())
}
#[cfg(not(windows))]
fn main() {}
