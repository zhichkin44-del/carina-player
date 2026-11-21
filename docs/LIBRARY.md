# Carina music browser: concept (2026-10-22)

**Principle:** Carina is a widget. There is no large window: everything opens from the widget and collapses back.

## Widget
- Top-right corner, on hover: ⚙ settings (background transparency, language).
- Bottom-right corner, on hover: ☰ **library**. A click grows the widget into a panel (about 400×560)
  toward the center of the screen (up if the widget is at the bottom; down if it is at the top). Another click or Esc collapses it.
- Mouse wheel over the widget changes volume. Clicking the progress line seeks.

## Library = your folders, not the whole system
- The user adds **their own music folders** (the "+" in the panel, or dragging a folder onto the widget).
  Nothing else is scanned: no drives, no Documents folder, no system paths.
- **Folder = playlist.** Subfolders = albums or compilations inside it. Order: by track number, otherwise by file name.
- Only audio is shown: mp3, flac, wav, m4a/aac, ogg, aiff (opus later). Folders without music are hidden.
- Folder cover: `cover.jpg` / `folder.jpg` / `front.jpg`, otherwise the cover from the first track's tags.
- Inside a folder: subfolder tiles at the top, the track list below (number, title, artist, duration).
  Clicking a track plays **this folder starting from that track**. The "▶" on a folder plays the whole folder with its subfolders in order.
- Search covers the current folder and its subfolders (title, artist, file name).
- "Queue" tab: what plays next; click to jump.

## Engine
- Own engine: rodio (cpal + symphonia); tags and covers via lofty. The queue is the folder; previous/next always work.
- When Carina plays its own audio, the widget shows it. When its own queue is empty, the widget shows other Windows players.
- Media keys and the system volume flyout go through Carina's own Windows media session (SMTC).

## Later
Index for global search, exact gapless playback, lyrics, equalizer.
