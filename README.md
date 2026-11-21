<p align="center">
  <img src="design/icon-256.png" width="112" alt="Carina">
</p>

<h1 align="center">Carina</h1>

<p align="center">
  A lightweight audio player for Windows: a glass desktop widget and a music browser that opens right from it.
  <br><sub>by zhichkin44-del</sub>
</p>

<p align="center">
  <a href="https://github.com/zhichkin44-del/carina-player/releases/latest"><b>Download the installer</b></a>
</p>

## Install (Windows, prebuilt)

1. Open [Releases](https://github.com/zhichkin44-del/carina-player/releases/latest) and download `Carina_<version>_x64-setup.exe` (about 2 MB).
2. Run it. No administrator rights are needed: Carina installs for the current user.
3. The widget appears in the lower-right corner. Hover over it: ☰ opens the library, ⚙ opens settings.

The installer registers Carina for audio files: "Open with → Carina" plays the whole folder starting from the chosen track.

The installer is not code-signed, so Windows SmartScreen may warn about an unknown publisher. Choose "More info → Run anyway" only if you trust the download.

## Features

- **Desktop widget.** Glass background with adjustable transparency, survives Win+D, does not steal focus, remembers its position.
- **Own audio engine** (rodio + symphonia): mp3, flac (up to 96 kHz / 24 bit), m4a/aac, ogg, wav, aiff.
- **Library from your own folders.** A folder is a playlist, subfolders are albums, covers come from `cover.jpg` or tags, with search.
- **Playlists and Favorites (♥)** as tiles, a queue with drag reorder, "play next", shuffle and repeat.
- **Media keys and the Windows volume flyout**, tray icon, start with Windows.
- **Resumes** where you stopped after a restart.
- **Shows other players** (Spotify, YouTube in a browser) when Carina's own queue is empty.
- Mouse wheel over the widget changes volume; clicking the progress line seeks.
- **English and Russian interface:** English by default, switch EN / RU in the widget settings.

## Requirements

- Windows. The prebuilt installer needs no administrator rights.
- To build from source: Node.js 22.18 or newer, Rust (stable), and Visual Studio Build Tools with the "Desktop development with C++" workload. The full list is under Build from source.

## Build from source (Windows)

You need:

- [Git for Windows](https://git-scm.com/download/win)
- [Node.js](https://nodejs.org) 22.18 or newer (LTS)
- [Rust](https://rustup.rs) (stable): download `rustup-init.exe` and accept the defaults
- Visual Studio Build Tools with the "Desktop development with C++" workload (required by Rust on Windows)
- WebView2 runtime (already included in Windows 11)

Steps, in PowerShell:

```powershell
git clone https://github.com/zhichkin44-del/carina-player.git
cd carina-player
npm install
npx tauri build
```

The installer is written to `src-tauri\target\release\bundle\nsis\`. Install it, or run `npx tauri dev` to start the app from source with hot reload.

> Build only with `npx tauri build` (or `tauri dev`). A plain `cargo build` produces an exe that looks for a dev server and shows a blank window.

## Development

```bash
npx tauri dev                                         # the app with hot reload
npm run dev                                           # UI test stand in a browser: /preview
cargo run --release --example check -- <folder>       # engine check on real files (no sound needed)
cargo run --release --example sessions                # Windows media session diagnostics
```

The Rust build cache (`src-tauri/target`, up to 15 GB) can be deleted at any time with `cargo clean` inside `src-tauri`.

## Usage

- Hover over the widget: ☰ opens the library, ⚙ opens settings.
- Mouse wheel over the widget changes volume; clicking the progress line seeks.
- "Open with → Carina" plays the whole folder starting from the chosen track.

## Tests

```bash
npm run check                     # svelte-check: types and UI markup
npm test                          # UI settings tests
cd src-tauri && cargo test        # core tests: engine, library, playlists, media sessions, window layout
```

## Tech stack

Tauri 2 · Rust (rodio, symphonia, lofty, Windows SMTC) · SvelteKit · TypeScript · NSIS

## Project structure

```
src/                 interface (SvelteKit): widget, panel, /preview stand
src-tauri/src/       Rust core: engine, library, playlists, SMTC, tray, desktop window
src-tauri/examples/  checks without the interface (engine, media sessions)
design/              icon and previews
docs/                library concept
```

## Status

Version **0.1.1**, Windows. Next: code signing, Microsoft Store, then macOS.

## License

[MIT](LICENSE). Third-party components and their licenses: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
