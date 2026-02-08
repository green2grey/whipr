# Whipr

Fast, private, local voice transcription desktop app. Press a hotkey, speak, and your words are transcribed and pasted into any application. All processing happens on-device via whisper.cpp.

## Architecture

Tauri v2 app with three layers:

- **Rust backend** (`src-tauri/src/`) — audio capture, whisper transcription, clipboard/paste automation, settings, storage
- **Svelte frontend** (`src/`) — single-page UI in `App.svelte` with settings, transcript history, clips, and model management
- **GNOME Shell extension** (`gnome-extension/`) — Linux-only floating overlay that shows recording state by polling `$XDG_STATE_HOME/whispr/overlay.json`

### Backend modules (`src-tauri/src/core/`)

| Module | Purpose |
|--------|---------|
| `audio.rs` | Audio capture via cpal, background worker thread with channel-based commands |
| `transcription.rs` | whisper-rs inference, model loading |
| `automation.rs` | Auto-paste via enigo (keyboard sim) or clipboard + Ctrl+V |
| `storage.rs` | SQLite via rusqlite — transcripts, clips, settings, embeddings |
| `models.rs` | Download/manage whisper GGML models |
| `embedding.rs` | Semantic search embeddings |
| `summary.rs` | Transcript summarization |
| `hotkeys.rs` | Global hotkey registration |
| `autostart.rs` | Launch-on-login setup |
| `runtime.rs` | Session type detection, paste method selection |
| `audio_import.rs` | Import audio files for batch transcription |

### Key types

- `AppState` (`state.rs`) — singleton behind `Mutex`, holds settings, transcripts, clips, recording state, audio worker channel
- `Settings` (`settings.rs`) — nested struct (audio, hotkeys, transcription, automation, storage, app, ui)
- All IPC types in `types.rs` — `Transcript`, `Clip`, `ModelInfo`, `ToggleResult`, etc.

### IPC pattern

All Tauri commands are in `commands.rs` and registered in `main.rs` via `invoke_handler`. Frontend calls them through typed wrappers in `src/lib/api.ts`. State access pattern: `State<Mutex<AppState>>` → `lock()` → operate → drop guard.

## Development

```bash
# Dev mode (starts Vite dev server + Rust backend)
npm run tauri dev

# Build for current platform
npm run tauri build

# Build with CUDA support (Linux)
npm run tauri:build:cuda

# Run Rust tests
cd src-tauri && cargo test

# Type-check frontend
npx tsc --noEmit
```

### Linux build prerequisites

```
libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev libasound2-dev clang
```

## Multi-platform

Targets Linux, macOS, and Windows. Platform-specific code uses `#[cfg(target_os = "...")]` guards.

| Platform | Config | Bundle targets | Notes |
|----------|--------|---------------|-------|
| Linux | `tauri.conf.json` | AppImage, deb | ALSA audio, GNOME overlay, XDG paths |
| macOS | `tauri.macos.conf.json` | app, dmg | Metal GPU feature, `Library/Application Support` paths |
| Windows | `tauri.conf.windows.json` | MSI, NSIS | `%LOCALAPPDATA%` paths |

The overlay (`overlay.rs`, `tray.rs`) communicates with the GNOME extension via JSON files in `$XDG_STATE_HOME/whispr/`. On non-Linux platforms these are no-ops.

## Dependency version pins

These are intentionally held back — do not upgrade without testing:

- `enigo = "0.1"` — 0.6.x has breaking API changes
- `cpal = "0.15"` — 0.17.x has breaking Sample trait changes
- `reqwest = "0.12"` — 0.13.x has breaking changes

## GPU feature flags

```
cuda, hipblas, intel-sycl, metal, vulkan
```

All gate through `whisper-rs` feature flags. The `_gpu` meta-feature is internal.

## CLI

The app accepts CLI args for single-instance IPC: `--toggle`, `--paste-last`, `--show`, `--settings`, `--quit`. Second launches send the action to the running instance via `tauri-plugin-single-instance`.

## Conventions

- Rust: standard `cargo fmt` formatting, serde derives on all IPC types
- Frontend: Svelte 4, TypeScript strict, no CSS framework (custom styles in `src/styles.css`)
- Version is tracked in three places: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` (plus the Windows and macOS conf variants)
- No test framework on the frontend; Rust tests exist in-module (`#[cfg(test)]`)
