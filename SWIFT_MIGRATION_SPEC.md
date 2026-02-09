# Whipr: Native Swift/SwiftUI macOS Rewrite Plan

## Context

Migrate Whipr from Tauri (Rust + Svelte) to a fully native macOS app using Swift and SwiftUI. This drops Linux/Windows support in exchange for native macOS integration, smaller binary, and direct access to Apple frameworks. The current app is ~6,900 lines of Rust backend + ~3,000 lines of Svelte frontend across 37 IPC commands.

**Expected outcome:** ~4,000-4,500 lines of Swift (72% backend reduction by eliminating cross-platform code and the IPC layer), native macOS feel, same whisper.cpp transcription quality.

---

## Architecture

**Pattern:** MVVM with `@Observable` (Swift 5.9+, macOS 14+)

| Current (Rust/Tauri) | Native (Swift) |
|---|---|
| `AppState` (Mutex singleton) | `@Observable class AppViewModel` on `@MainActor` |
| `Settings` (serde structs) | `Codable` struct saved as JSON |
| Audio worker thread + mpsc channel | `actor AudioEngine` with async methods |
| `WhisperContext` static cache | `actor TranscriptionEngine` |
| Tauri `emit()` events | Combine publishers or `AsyncStream` |
| `commands.rs` IPC handlers | Direct method calls on view models |
| `types.rs` IPC structs | Plain `Codable` structs |

---

## Project Structure

```
Whipr/
  Whipr.xcodeproj/
  Whipr/
    WhiprApp.swift                 -- @main, MenuBarExtra, window management
    Info.plist / Whipr.entitlements
    Assets.xcassets/

    Models/                        -- Codable data types
      Transcript.swift, Clip.swift, Settings.swift, ModelInfo.swift, etc.

    Services/                      -- Business logic (non-UI)
      AudioEngine.swift            -- AVAudioEngine capture, ring buffer, levels
      TranscriptionEngine.swift    -- whisper.cpp C bridging, context cache
      AutomationService.swift      -- NSPasteboard + CGEvent paste
      StorageService.swift         -- SwiftData + SQLite migration
      ModelManager.swift           -- URLSession downloads
      HotkeyService.swift          -- MASShortcut / Carbon HotKey API
      PermissionService.swift      -- AXIsProcessTrusted, CGPreflightListenEventAccess
      EmbeddingService.swift       -- FNV1a hash embeddings (port from Rust)
      SummaryService.swift         -- Title/summary heuristics (port from Rust)
      UpdateService.swift          -- Sparkle integration
      AudioImportService.swift     -- AVAssetReader for file decoding

    ViewModels/
      AppViewModel.swift, RecordingViewModel.swift, TranscriptListViewModel.swift,
      ClipListViewModel.swift, SettingsViewModel.swift, ModelViewModel.swift,
      OnboardingViewModel.swift

    Views/
      MainWindow/   -- ContentView, HomeView, TranscriptDetailView, ClipListView
      Settings/     -- SettingsView + 9 section views
      Onboarding/   -- OnboardingView (4-step wizard)
      HUD/          -- RecordingHUDWindow (NSPanel), RecordingHUDView
      Shared/       -- ConfirmDialog, EmptyStateView, HotkeyRecorderView, AudioMeterView

  Packages/
    whisper-cpp/                   -- Local SPM package wrapping whisper.cpp
      Package.swift
      Sources/whisper-cpp/include/ -- whisper.h, ggml.h

  WhiprTests/
```

---

## Phase 0: Foundation & Project Setup (3-5 days)

- Create Xcode project (macOS 14+, SwiftUI lifecycle)
- **Integrate whisper.cpp as local SPM package** with Metal + Accelerate flags
- Create `WhisperWrapper.swift` — Swift-friendly class around the C API (`whisper_init_from_file_with_params`, `whisper_full`, etc.)
- Set up entitlements: `com.apple.security.device.audio-input`, hardened runtime
- Add external dependencies: **MASShortcut** (SPM, hotkeys), **Sparkle** (SPM, updates)

**Key decision:** Target macOS 14+ (Sonoma) to use SwiftData. Direct distribution first (hardened runtime), Mac App Store later.

---

## Phase 1: Core Engine — Audio + Transcription (7-10 days)

### AudioEngine.swift (replaces `audio.rs`, 900 lines → ~400)

- `AVAudioEngine` with input node tap for capture
- Ring buffer for sample storage (port atomic ring buffer or use `TPCircularBuffer`)
- VAD state machine (threshold-based, port from `audio.rs` lines 812-878)
- RMS level metering in the tap callback
- Device enumeration via Core Audio `AudioDeviceID`
- Methods: `startRecording()`, `stopRecording() -> RecordedAudio`, `snapshot()`, `currentLevel()`

### TranscriptionEngine.swift (replaces `transcription.rs`, 522 lines → ~300)

- Wraps `WhisperWrapper` with context caching (`ContextKey` = model path + GPU flag)
- GPU fallback: try Metal init, store error on failure, fall back to CPU
- `to_mono()` and `resample_linear()` audio conversion (direct port)
- `transcribe()` for full inference, `transcribePreview()` capped at 10s window

### AudioImportService.swift (replaces `audio_import.rs`, 103 lines)

- `AVAssetReader` + `AVAssetReaderTrackOutput` decodes MP3/AAC/FLAC/WAV/M4A natively

**Critical files to reference:** `src-tauri/src/core/audio.rs`, `src-tauri/src/core/transcription.rs`

---

## Phase 2: Data Layer — Storage + Settings (5-7 days)

### StorageService.swift (replaces `storage.rs`, 811 lines → ~300)

- **SwiftData** with `@Model` entities: `TranscriptEntity`, `ClipEntity`
- Same schema as current SQLite (id, created_at, duration_ms, text, tags, embedding, etc.)
- Migration reader for existing `~/Library/Application Support/whispr/whispr.db`

### Settings (replaces `settings.rs`, 193 lines → ~250)

- `Codable` struct hierarchy matching current Rust types exactly
- Persisted as JSON to `~/Library/Application Support/whispr/settings.json`
- Same default values as current `settings.rs` lines 137-192

### EmbeddingService.swift + SummaryService.swift

- Direct algorithmic ports from `embedding.rs` (107 lines) and `summary.rs` (74 lines)
- FNV1a hash, 128-dim sparse vectors, cosine similarity
- Title = first 8 words, summary = first 2 sentences

**Critical files to reference:** `src-tauri/src/core/storage.rs`, `src-tauri/src/settings.rs`

---

## Phase 3: Automation — Clipboard, Paste, Permissions (5-7 days)

### AutomationService.swift (replaces `automation.rs`, 710 lines → ~150)

Massive reduction — current file is 80% Linux/Windows code paths.

- **Clipboard:** `NSPasteboard` directly (replaces `pbcopy`/`pbpaste` shell commands)
- **Paste:** `CGEvent` for Cmd+V (replaces `osascript`), with `osascript` fallback
- **Focus capture:** `NSWorkspace.shared.frontmostApplication` (replaces X11 `xdotool`)
- Clipboard preserve/restore flow with configurable delay

### PermissionService.swift (replaces `macos_permissions.rs`, 109 lines)

- `AXIsProcessTrusted()` / `AXIsProcessTrustedWithOptions()` for Accessibility
- `CGPreflightListenEventAccess()` / `CGRequestListenEventAccess()` for Input Monitoring
- Deep-link to System Settings privacy panes

**Critical files to reference:** `src-tauri/src/core/automation.rs`, `src-tauri/src/core/macos_permissions.rs`

---

## Phase 4: UI Shell — App Lifecycle, Menu Bar, HUD (7-10 days)

### WhiprApp.swift

- `WindowGroup` for main window, `MenuBarExtra` for status bar, `Settings` scene
- Single-instance via `NSRunningApplication` + `NSDistributedNotificationCenter`
- Close-to-tray behavior, start-hidden option

### Menu Bar (replaces tray/overlay, ~960 lines → ~50)

- `MenuBarExtra` with recording state icon, recent transcripts, quick actions
- Eliminates GNOME extension and Windows tray entirely

### Recording HUD (replaces `hud.rs` + `RecordingHud.svelte`, ~370 lines)

- `NSPanel` (floating, borderless, transparent, non-activating)
- SwiftUI content: red pulsing dot, elapsed time, 14-bar audio meter, stop button
- Show/hide with animation on recording state change

### HotkeyService.swift

- **MASShortcut** for registration + recording UI widget
- Three hotkeys: record toggle, paste last, open app
- Login item via `SMAppService.register()` (replaces `autostart.rs`)

---

## Phase 5: Home View — Recording, Transcripts, Search (7-10 days)

### HomeView.swift

- Recording controls (start/stop button, status pill with timer)
- Alert banners (error, GPU warning, hotkey conflict, update available)
- Live preview card during recording
- Search bar with keyword + semantic toggle, date filter, tag pills

### Recording Flow (port of `commands.rs` toggle logic, ~240 lines)

1. Start: capture focused app → start audio → pre-warm whisper context → start preview loop
2. Stop: stop audio → transcribe → save → auto-paste to focused app
3. Preview loop: Swift `Task` polling audio snapshots every 4.5-7s, partial transcription, text merging

### TranscriptList + TranscriptRow

- `List` with date rail, title, summary, duration, tags
- Click-to-copy with "Copied" feedback
- Hover effects, compact mode toggle

### ClipListView.swift

- Grid of clip cards with copy/delete actions
- Empty state

---

## Phase 6: Settings + Onboarding (5-7 days)

### SettingsView.swift (9 tabbed sections)

1. **Audio** — device picker, sample rate, channels, gain, noise gate, VAD
2. **Hotkeys** — three `MASShortcutView` widgets
3. **Transcription** — model selector, language, threads, GPU toggle, benchmark
4. **Automation** — auto-paste, delay, clipboard options
5. **Storage** — data directory, retention, keep audio
6. **App** — launch on login, start in tray, close to tray
7. **UI** — theme (light/dark/auto), compact list, live preview, HUD
8. **Models** — download/delete/activate with progress bars
9. **About** — version, update check, storage stats

### OnboardingView.swift (4 steps)

Welcome → Audio Device → Hotkeys → Permissions & Auto-Paste

---

## Phase 7: Model Management (3-5 days)

### ModelManager.swift (replaces `models.rs`, 166 lines)

- Three models: tiny.en, small.en, medium.en (HuggingFace URLs)
- `URLSession` with `URLSessionDownloadDelegate` for progress tracking
- Atomic download (temp file → rename)
- Reuse existing model files from `~/Library/Application Support/whispr/models/`

---

## Phase 8: Polish & Distribution (5-7 days)

- **Data migration** from existing Tauri SQLite database on first launch
- **Sparkle** for auto-updates (direct distribution)
- **CLI single-instance** forwarding via `NSDistributedNotificationCenter`
- Code signing + notarization for direct distribution
- Icon, about screen, final visual polish

---

## External Dependencies (3 total)

| Dependency | Via | Purpose |
|---|---|---|
| whisper.cpp | Local SPM package | Transcription engine |
| MASShortcut | SPM | Global hotkey registration + UI |
| Sparkle | SPM | Auto-update framework |

Everything else uses Apple frameworks: AVAudioEngine, SwiftData, NSPasteboard, CGEvent, URLSession, Metal, Accelerate, SMAppService.

---

## Verification / Testing

- **Unit tests:** Embedding (determinism, cosine similarity), Summary (title/sentence extraction), AudioRingBuffer (snapshot, cursor, overwrite), Settings (Codable round-trip), Storage (CRUD)
- **Integration tests:** Load tiny model → transcribe known WAV → verify output text; record silence → verify minimal transcript; import audio file → verify result
- **UI tests (XCUITest):** Onboarding flow, settings persistence, HUD show/hide
- **Manual verification:** Record with hotkey → transcript appears → auto-paste works; download model with progress; data migration from existing Tauri install

---

## Timeline Summary

| Phase | Days | Cumulative |
|---|---|---|
| 0: Foundation | 3-5 | 3-5 |
| 1: Audio + Transcription | 7-10 | 10-15 |
| 2: Data Layer | 5-7 | 15-22 |
| 3: Automation | 5-7 | 20-29 |
| 4: UI Shell | 7-10 | 27-39 |
| 5: Home View | 7-10 | 34-49 |
| 6: Settings + Onboarding | 5-7 | 39-56 |
| 7: Model Management | 3-5 | 42-61 |
| 8: Polish + Distribution | 5-7 | **47-68 days (~10-14 weeks)** |

Phases 1-3 are independent service layers and can be parallelized, compressing to ~8-10 weeks with two developers.

---

## Risks

| Risk | Severity | Mitigation |
|---|---|---|
| whisper.cpp SPM + Metal build config | High | Start Phase 1 here, iterate early |
| CGEvent paste reliability across apps | High | Support both CGEvent and osascript fallback |
| Live preview thread safety | Medium | Use Swift `actor` isolation |
| SwiftData maturity edge cases | Medium | Core Data fallback ready |
| Data migration from existing installs | Low | Same SQLite schema, same file paths |
