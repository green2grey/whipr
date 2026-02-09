use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::app_tray;
use crate::core::audio::AudioDevice;
use crate::core::{
    audio, audio_import, automation, autostart, embedding, macos_permissions, models, runtime,
    storage, summary, transcription,
};
use crate::overlay;
use crate::settings::Settings;
use crate::state::AppState;
use crate::tray;
use crate::types::{
    BenchmarkResult, Clip, ImportFailure, ImportResult, MacosPermissions, ModelInfo,
    PerformanceInfo, RuntimeInfo, StorageStats, ToggleResult, Transcript, UpdateInfo,
};
struct ToggleOutcome {
    result: ToggleResult,
    started_at_ms: Option<i64>,
    focus_window_id: Option<String>,
    automation_settings: Option<crate::settings::AutomationSettings>,
}

#[derive(Clone, Serialize)]
struct RecordingEvent {
    recording: bool,
    started_at_ms: Option<i64>,
}

#[derive(Clone, Serialize)]
struct PreviewEvent {
    text: String,
}

fn emit_transcription_started(app: &AppHandle) {
    // UI uses this as a cue that recording has stopped and transcription is beginning.
    let _ = app.emit("transcription-started", true);
}

#[derive(Debug, Deserialize)]
pub struct TranscriptUpdate {
    text: Option<String>,
    title: Option<String>,
    summary: Option<String>,
    tags: Option<Vec<String>>,
}

#[derive(Clone, Serialize)]
struct ModelDownloadProgress {
    id: String,
    downloaded: u64,
    total: u64,
}

#[derive(Clone, Serialize)]
struct ImportProgress {
    index: usize,
    total: usize,
    path: String,
}

#[derive(Clone, Serialize)]
struct AutomationErrorEvent {
    message: String,
}

const PREVIEW_MIN_SECONDS: f32 = 1.2;
const PREVIEW_INACTIVE_POLL_MS: u64 = 650;
const PREVIEW_INTERVAL_CPU_MS: u64 = 7000;
const PREVIEW_INTERVAL_GPU_MS: u64 = 4500;
const PREVIEW_INTERVAL_MIN_MS: u64 = 3000;
const PREVIEW_INTERVAL_MAX_MS: u64 = 12000;
const PREVIEW_BACKLOG_SECONDS: f32 = 12.0;

fn emit_recording_event(app: &AppHandle, outcome: &ToggleOutcome) {
    let payload = RecordingEvent {
        recording: outcome.result.recording,
        started_at_ms: outcome.started_at_ms,
    };
    let _ = app.emit("recording-state", payload);
}

fn emit_transcript_event(app: &AppHandle, transcript: &Option<Transcript>) {
    if let Some(transcript) = transcript {
        let _ = app.emit("transcript-created", transcript);
    }
}

fn emit_preview_event(app: &AppHandle, text: String) {
    let payload = PreviewEvent { text };
    let _ = app.emit("transcript-preview", payload);
}

fn stop_preview_thread(state: &Mutex<AppState>) {
    if let Ok(mut guard) = state.lock() {
        if let Some(cancel) = guard.preview_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

fn start_preview_thread(app: AppHandle, state: &Mutex<AppState>) {
    let (audio_tx, settings, cancel, ui_active) = {
        let mut guard = match state.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if let Some(cancel) = guard.preview_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        guard.preview_cancel = Some(cancel.clone());
        (
            guard.audio_tx.clone(),
            guard.settings.clone(),
            cancel,
            guard.ui_active.clone(),
        )
    };

    std::thread::spawn(move || {
        // Warm up the shared context cache so the "stop recording -> transcribe" path doesn't
        // pay model load / backend init costs (especially noticeable with Metal).
        let _ = transcription::ensure_context(&settings);

        let mut cursor = 0_usize;
        let mut preview = String::new();
        let wants_gpu = settings.transcription.use_gpu && cfg!(feature = "_gpu");
        let mut interval_ms = if wants_gpu {
            PREVIEW_INTERVAL_GPU_MS
        } else {
            PREVIEW_INTERVAL_CPU_MS
        };

        loop {
            if cancel.load(Ordering::Relaxed) {
                break;
            }

            if !ui_active.load(Ordering::Relaxed) {
                // UI isn't visible/focused; avoid expensive snapshot+inference.
                // Keep cursor near the tail so we won't allocate huge snapshots if UI becomes
                // active mid-recording.
                if let Ok(stats) = audio::stats(&audio_tx) {
                    let keep = ((stats.sample_rate as f32)
                        * (stats.channels as f32).max(1.0)
                        * PREVIEW_BACKLOG_SECONDS)
                        .round()
                        .max(0.0) as usize;
                    cursor = stats.total_samples.saturating_sub(keep);
                }
                std::thread::sleep(Duration::from_millis(PREVIEW_INACTIVE_POLL_MS));
                continue;
            }

            let snapshot = match audio::snapshot_audio(&audio_tx, cursor) {
                Ok(snapshot) => snapshot,
                Err(_) => break,
            };

            if snapshot.samples.is_empty() {
                interval_ms = (interval_ms + 500).min(PREVIEW_INTERVAL_MAX_MS);
                std::thread::sleep(Duration::from_millis(interval_ms));
                continue;
            }

            cursor = snapshot.total_samples;

            let seconds = snapshot.samples.len() as f32
                / (snapshot.sample_rate as f32 * snapshot.channels as f32).max(1.0);
            if seconds < PREVIEW_MIN_SECONDS {
                interval_ms = (interval_ms + 250).min(PREVIEW_INTERVAL_MAX_MS);
                std::thread::sleep(Duration::from_millis(interval_ms));
                continue;
            }

            let audio = audio::RecordedAudio {
                samples: snapshot.samples,
                sample_rate: snapshot.sample_rate,
                channels: snapshot.channels,
            };

            if cancel.load(Ordering::Relaxed) {
                break;
            }

            let started = Instant::now();
            match transcription::transcribe_preview(&settings, audio) {
                Ok(chunk) => {
                    let chunk = chunk.trim();
                    if !chunk.is_empty() {
                        preview = merge_preview_text(&preview, chunk);
                        emit_preview_event(&app, preview.clone());
                    }
                }
                Err(err) => {
                    // Non-fatal: preview transcription is best-effort, but log failures for debugging.
                    eprintln!(
                        "[DEBUG] preview transcription failed: {err} (cursor={cursor}, preview_len={}, model={}, use_gpu={})",
                        preview.len(),
                        settings.transcription.model,
                        settings.transcription.use_gpu
                    );
                }
            }

            // Dynamic interval:
            // - Back off on slow inference (CPU-only systems) to avoid pegging cores.
            // - Speed up a bit when inference is fast and we have enough audio.
            let took_ms = started.elapsed().as_millis() as u64;
            if took_ms >= interval_ms.saturating_sub(250) {
                interval_ms = (took_ms + 1250).min(PREVIEW_INTERVAL_MAX_MS);
            } else if seconds >= 3.0 && took_ms < 600 {
                interval_ms = interval_ms.saturating_sub(500).max(PREVIEW_INTERVAL_MIN_MS);
            }

            std::thread::sleep(Duration::from_millis(interval_ms));
        }
    });
}

fn merge_preview_text(current: &str, incoming: &str) -> String {
    if current.is_empty() {
        return incoming.to_string();
    }
    if incoming.is_empty() {
        return current.to_string();
    }

    let current_lower = current.to_lowercase();
    let incoming_lower = incoming.to_lowercase();
    let incoming_chars: Vec<char> = incoming.chars().collect();
    let incoming_lower_chars: Vec<char> = incoming_lower.chars().collect();
    let incoming_len = incoming_chars.len().min(incoming_lower_chars.len());
    let max_overlap = current_lower.chars().count().min(incoming_len).min(48);
    let mut overlap = 0;

    for i in 1..=max_overlap {
        let candidate: String = incoming_lower_chars[..i].iter().collect();
        if current_lower.ends_with(&candidate) {
            overlap = i;
        }
    }

    if overlap > 0 {
        let suffix: String = incoming_chars[overlap..].iter().collect();
        let trimmed = suffix.trim_start();
        if trimmed.is_empty() {
            return current.to_string();
        }
        return format!("{current} {trimmed}");
    }

    format!("{current} {incoming}")
}

fn normalize_optional(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[tauri::command]
pub fn ping() -> &'static str {
    "ok"
}

#[tauri::command]
pub fn get_default_settings() -> Settings {
    Settings::default()
}

#[tauri::command]
pub fn get_settings(state: State<'_, Mutex<AppState>>) -> Result<Settings, String> {
    let guard = state
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?;
    Ok(guard.settings.clone())
}

#[tauri::command]
pub fn set_ui_active(state: State<'_, Mutex<AppState>>, active: bool) -> Result<bool, String> {
    let ui_active = state
        .lock()
        .map(|guard| guard.ui_active.clone())
        .map_err(|_| "state lock poisoned".to_string())?;
    ui_active.store(active, Ordering::Relaxed);
    Ok(true)
}

#[tauri::command]
pub fn set_audio_input_device(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
    input_device_id: String,
) -> Result<Settings, String> {
    let input_device_id = input_device_id.trim().to_string();
    if input_device_id.is_empty() {
        return Err("Input device is required".to_string());
    }

    if !audio::input_device_available(&input_device_id) {
        return Err(format!("Input device not available: {input_device_id}"));
    }

    let settings = {
        let mut guard = state
            .lock()
            .map_err(|_| "state lock poisoned".to_string())?;
        guard.settings.audio.input_device_id = input_device_id;
        storage::save_settings(&guard.settings)?;
        guard.settings.clone()
    };

    let _ = app.emit("settings-updated", settings.clone());
    Ok(settings)
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
    settings: Settings,
) -> Result<Settings, String> {
    let previous_settings = state
        .lock()
        .map(|guard| guard.settings.clone())
        .map_err(|_| "state lock poisoned".to_string())?;
    let launch_changed = previous_settings.app.launch_on_login != settings.app.launch_on_login;
    let transcription_context_changed = previous_settings.transcription.model
        != settings.transcription.model
        || previous_settings.transcription.model_dir != settings.transcription.model_dir
        || previous_settings.transcription.use_gpu != settings.transcription.use_gpu;

    if !audio::input_device_available(&settings.audio.input_device_id) {
        return Err(format!(
            "Input device not available: {}",
            settings.audio.input_device_id
        ));
    }

    if launch_changed {
        autostart::apply_launch_on_login(settings.app.launch_on_login)
            .map_err(|err| format!("Failed to update launch on login: {err}"))?;
    }

    if let Err(err) = storage::save_settings(&settings) {
        if launch_changed {
            let _ = autostart::apply_launch_on_login(previous_settings.app.launch_on_login);
        }
        return Err(err);
    }
    {
        let mut guard = state
            .lock()
            .map_err(|_| "state lock poisoned".to_string())?;
        let data_dir_changed = previous_settings.storage.data_dir != settings.storage.data_dir;
        guard.settings = settings.clone();
        if data_dir_changed {
            guard.transcripts = storage::load_transcripts_with_retention(&guard.settings);
            guard.clips = storage::load_clips(&guard.settings);
        }
        let last_transcript_at_ms = guard.transcripts.first().map(|item| item.created_at);
        let _ = tray::write_recents(&guard.settings, &guard.transcripts, last_transcript_at_ms);
    }

    if transcription_context_changed {
        transcription::invalidate_context_cache();
        if !previous_settings.transcription.use_gpu && settings.transcription.use_gpu {
            transcription::clear_last_gpu_error();
        }
    }

    app_tray::refresh_tray(&app, state.inner());

    // Apply HUD visibility immediately when the user toggles it.
    if let Some(window) = app.get_webview_window("recording_hud") {
        let (recording, hud_enabled) = state
            .inner()
            .lock()
            .ok()
            .map(|guard| (guard.recording, guard.settings.ui.recording_hud_enabled))
            .unwrap_or((false, true));

        if recording && hud_enabled {
            let _ = window.show();
        } else {
            let _ = window.hide();
        }
    }

    let _ = app.emit("settings-updated", settings.clone());
    Ok(settings)
}

#[tauri::command]
pub fn list_transcripts(state: State<'_, Mutex<AppState>>) -> Result<Vec<Transcript>, String> {
    let guard = state
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?;
    Ok(guard.transcripts.clone())
}

#[tauri::command]
pub fn search_transcripts(
    state: State<'_, Mutex<AppState>>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<Transcript>, String> {
    let query = query.trim().to_string();

    if query.is_empty() {
        let guard = state
            .lock()
            .map_err(|_| "state lock poisoned".to_string())?;
        let mut results = guard.transcripts.clone();
        if let Some(limit) = limit {
            results.truncate(limit);
        }
        return Ok(results);
    }

    let query_embedding = embedding::embed_text(&query);
    let query_has_signal = query_embedding.iter().any(|value| *value != 0.0);
    let query_lower = query.to_lowercase();

    // Collect transcripts missing embeddings while holding the state lock, then compute embeddings
    // outside the lock to avoid blocking other commands.
    let to_embed: Vec<(String, String)> = {
        let guard = state
            .lock()
            .map_err(|_| "state lock poisoned".to_string())?;
        guard
            .transcripts
            .iter()
            .filter(|t| t.embedding.is_none())
            .map(|t| (t.id.clone(), t.text.clone()))
            .collect()
    };

    use std::collections::HashMap;
    let mut embeddings_by_id: HashMap<String, Vec<f32>> = HashMap::with_capacity(to_embed.len());
    for (id, text) in to_embed {
        embeddings_by_id.insert(id, embedding::embed_text(&text));
    }

    let mut updated: Vec<Transcript> = Vec::new();
    let (settings, transcripts_snapshot) = {
        let mut guard = state
            .lock()
            .map_err(|_| "state lock poisoned".to_string())?;

        if !embeddings_by_id.is_empty() {
            for transcript in guard.transcripts.iter_mut() {
                if transcript.embedding.is_none() {
                    if let Some(embedding) = embeddings_by_id.remove(&transcript.id) {
                        transcript.embedding = Some(embedding);
                        updated.push(transcript.clone());
                    }
                }
            }
        }

        (guard.settings.clone(), guard.transcripts.clone())
    };

    // Persist any newly computed embeddings.
    for transcript in updated.iter() {
        let _ = storage::upsert_transcript(&settings, transcript);
    }

    let mut scored: Vec<(f32, Transcript)> = Vec::with_capacity(transcripts_snapshot.len());
    for transcript in transcripts_snapshot.into_iter() {
        let embedding = transcript
            .embedding
            .clone()
            .unwrap_or_else(|| embedding::embed_text(&transcript.text));
        let mut score = if query_has_signal {
            embedding::cosine_similarity(&query_embedding, &embedding)
        } else {
            0.0
        };

        if !query_lower.is_empty() {
            let title = transcript.title.as_deref().unwrap_or("");
            let summary = transcript.summary.as_deref().unwrap_or("");
            if transcript.text.to_lowercase().contains(&query_lower)
                || title.to_lowercase().contains(&query_lower)
                || summary.to_lowercase().contains(&query_lower)
            {
                score += 0.25;
            }
        }

        scored.push((score, transcript));
    }

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut results: Vec<Transcript> = scored
        .into_iter()
        .map(|(_, transcript)| transcript)
        .collect();

    if let Some(limit) = limit {
        results.truncate(limit);
    }

    Ok(results)
}

#[tauri::command]
pub fn list_clips(state: State<'_, Mutex<AppState>>) -> Result<Vec<Clip>, String> {
    let guard = state
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?;
    Ok(guard.clips.clone())
}

#[tauri::command]
pub fn create_clip(
    state: State<'_, Mutex<AppState>>,
    title: String,
    text: String,
    transcript_id: Option<String>,
) -> Result<Clip, String> {
    let title = title.trim();
    let text = text.trim();
    if title.is_empty() || text.is_empty() {
        return Err("Clip title and text are required".to_string());
    }

    let clip = Clip {
        id: Uuid::new_v4().to_string(),
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0),
        title: title.to_string(),
        text: text.to_string(),
        transcript_id,
    };

    let mut guard = state
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?;
    storage::insert_clip(&guard.settings, &clip)?;
    guard.clips.insert(0, clip.clone());
    Ok(clip)
}

#[tauri::command]
pub fn delete_clip(state: State<'_, Mutex<AppState>>, id: String) -> Result<bool, String> {
    let mut guard = state
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?;
    storage::delete_clip(&guard.settings, &id)?;
    guard.clips.retain(|clip| clip.id != id);
    Ok(true)
}

#[tauri::command]
pub fn update_transcript(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
    id: String,
    update: TranscriptUpdate,
) -> Result<Transcript, String> {
    let transcript = {
        let mut guard = state
            .lock()
            .map_err(|_| "state lock poisoned".to_string())?;
        let transcript = guard
            .transcripts
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| "Transcript not found".to_string())?;

        if let Some(text) = update.text {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Err("Transcript text cannot be empty".to_string());
            }
            transcript.text = trimmed.to_string();
            transcript.title = summary::generate_title(trimmed);
            transcript.summary = summary::generate_summary(trimmed);
            transcript.embedding = Some(embedding::embed_text(trimmed));
        }
        if let Some(title) = update.title {
            transcript.title = normalize_optional(title);
        }
        if let Some(summary) = update.summary {
            transcript.summary = normalize_optional(summary);
        }
        if let Some(tags) = update.tags {
            transcript.tags = tags;
        }

        let cloned = transcript.clone();
        storage::upsert_transcript(&guard.settings, &cloned)?;
        let last_transcript_at_ms = guard.transcripts.first().map(|item| item.created_at);
        let _ = tray::write_recents(&guard.settings, &guard.transcripts, last_transcript_at_ms);
        cloned
    };
    app_tray::refresh_tray(&app, state.inner());

    Ok(transcript)
}

#[tauri::command]
pub fn delete_transcript(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
    id: String,
) -> Result<bool, String> {
    let (settings, removed) = {
        let mut guard = state
            .lock()
            .map_err(|_| "state lock poisoned".to_string())?;
        let index = guard
            .transcripts
            .iter()
            .position(|item| item.id == id)
            .ok_or_else(|| "Transcript not found".to_string())?;
        let removed = guard.transcripts.remove(index);
        if let Err(err) = storage::delete_transcript_row(&guard.settings, &id) {
            guard.transcripts.insert(index, removed.clone());
            return Err(err);
        }
        let last_transcript_at_ms = guard.transcripts.first().map(|item| item.created_at);
        let _ = tray::write_recents(&guard.settings, &guard.transcripts, last_transcript_at_ms);
        (guard.settings.clone(), removed)
    };

    if let Some(path) = removed.audio_path.as_deref() {
        let _ = storage::delete_audio_file(&settings, path);
    }

    app_tray::refresh_tray(&app, state.inner());
    Ok(true)
}

#[tauri::command]
pub fn clear_transcripts(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
) -> Result<bool, String> {
    let (settings, audio_paths) = {
        let mut guard = state
            .lock()
            .map_err(|_| "state lock poisoned".to_string())?;
        let audio_paths = guard
            .transcripts
            .iter()
            .filter_map(|item| item.audio_path.clone())
            .collect::<Vec<_>>();
        guard.transcripts.clear();
        storage::clear_transcripts_table(&guard.settings)?;
        let _ = tray::write_recents(&guard.settings, &guard.transcripts, None);
        (guard.settings.clone(), audio_paths)
    };

    for path in audio_paths {
        let _ = storage::delete_audio_file(&settings, &path);
    }
    app_tray::refresh_tray(&app, state.inner());
    Ok(true)
}

#[tauri::command]
pub fn import_audio_files(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
    paths: Vec<String>,
) -> Result<ImportResult, String> {
    if paths.is_empty() {
        return Ok(ImportResult {
            transcripts: Vec::new(),
            failures: Vec::new(),
        });
    }

    let settings = state
        .lock()
        .map(|guard| guard.settings.clone())
        .map_err(|_| "state lock poisoned".to_string())?;

    let total = paths.len();
    let mut imported = Vec::new();
    let mut failures = Vec::new();

    for (index, path) in paths.iter().enumerate() {
        let _ = app.emit(
            "import-progress",
            ImportProgress {
                index: index + 1,
                total,
                path: path.clone(),
            },
        );

        let decoded = match audio_import::decode_audio_file(Path::new(path)) {
            Ok(decoded) => decoded,
            Err(err) => {
                failures.push(ImportFailure {
                    path: path.clone(),
                    error: err,
                });
                continue;
            }
        };

        let text = match transcription::transcribe(&settings, decoded.audio) {
            Ok(text) => text,
            Err(err) => {
                failures.push(ImportFailure {
                    path: path.clone(),
                    error: err,
                });
                continue;
            }
        };

        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);
        let title = summary::generate_title(&text);
        let summary = summary::generate_summary(&text);
        let embedding = embedding::embed_text(&text);

        let transcript = Transcript {
            id: Uuid::new_v4().to_string(),
            created_at,
            duration_ms: decoded.duration_ms,
            text,
            title,
            summary,
            tags: Vec::new(),
            audio_path: None,
            embedding: Some(embedding),
        };

        {
            let mut guard = state
                .lock()
                .map_err(|_| "state lock poisoned".to_string())?;
            storage::upsert_transcript(&guard.settings, &transcript)?;
            guard.transcripts.insert(0, transcript.clone());
            let last_transcript_at_ms = guard.transcripts.first().map(|item| item.created_at);
            let _ = tray::write_recents(&guard.settings, &guard.transcripts, last_transcript_at_ms);
        }

        emit_transcript_event(&app, &Some(transcript.clone()));
        imported.push(transcript);
    }

    app_tray::refresh_tray(&app, state.inner());

    Ok(ImportResult {
        transcripts: imported,
        failures,
    })
}

#[tauri::command]
pub fn get_runtime_info(state: State<'_, Mutex<AppState>>) -> RuntimeInfo {
    let settings = state
        .lock()
        .map(|guard| guard.settings.clone())
        .unwrap_or_else(|_| Settings::default());
    runtime::runtime_info(
        &settings.automation.paste_method,
        settings.automation.copy_to_clipboard || settings.automation.preserve_clipboard,
        settings.automation.preserve_clipboard && !settings.automation.copy_to_clipboard,
    )
}

#[tauri::command]
pub fn get_macos_permissions() -> MacosPermissions {
    MacosPermissions {
        accessibility: macos_permissions::accessibility_enabled(),
        input_monitoring: macos_permissions::input_monitoring_enabled(),
    }
}

#[tauri::command]
pub fn request_macos_accessibility_permission() -> MacosPermissions {
    let _ = macos_permissions::request_accessibility_prompt();
    MacosPermissions {
        accessibility: macos_permissions::accessibility_enabled(),
        input_monitoring: macos_permissions::input_monitoring_enabled(),
    }
}

#[tauri::command]
pub fn request_macos_input_monitoring_permission() -> MacosPermissions {
    let _ = macos_permissions::request_input_monitoring_prompt();
    MacosPermissions {
        accessibility: macos_permissions::accessibility_enabled(),
        input_monitoring: macos_permissions::input_monitoring_enabled(),
    }
}

#[tauri::command]
pub fn open_macos_permission_settings(permission: String) -> Result<bool, String> {
    macos_permissions::open_privacy_settings(&permission)?;
    Ok(true)
}

#[tauri::command]
pub fn get_performance_info(state: State<'_, Mutex<AppState>>) -> PerformanceInfo {
    let settings = state
        .lock()
        .map(|guard| guard.settings.clone())
        .unwrap_or_else(|_| Settings::default());
    let gpu_supported = cfg!(feature = "_gpu");
    let gpu_name = transcription::detect_gpu_name();
    let gpu_error = if settings.transcription.use_gpu && gpu_supported {
        transcription::last_gpu_error()
    } else {
        None
    };
    let gpu_enabled = settings.transcription.use_gpu && gpu_supported && gpu_error.is_none();
    let thread_count = transcription::resolve_thread_count(&settings, None);

    PerformanceInfo {
        gpu_supported,
        gpu_enabled,
        thread_count,
        gpu_error,
        gpu_name,
    }
}

#[tauri::command]
pub fn benchmark_transcription(
    state: State<'_, Mutex<AppState>>,
    path: String,
) -> Result<BenchmarkResult, String> {
    let settings = state
        .lock()
        .map(|guard| guard.settings.clone())
        .map_err(|_| "state lock poisoned".to_string())?;

    let decoded = audio_import::decode_audio_file(Path::new(&path))?;
    let audio_seconds = decoded.duration_ms as f32 / 1000.0;

    let started = Instant::now();
    let text = transcription::transcribe(&settings, decoded.audio)?;
    let duration_ms = started.elapsed().as_millis() as u64;

    let duration_seconds = (duration_ms as f32 / 1000.0).max(0.001);
    let realtime_factor = audio_seconds / duration_seconds;

    Ok(BenchmarkResult {
        audio_seconds,
        duration_ms,
        realtime_factor,
        text_length: text.len(),
    })
}

#[tauri::command]
pub fn copy_text(text: String) -> Result<bool, String> {
    automation::copy_text(&text)?;
    Ok(true)
}

#[tauri::command]
pub fn export_transcript(path: String, text: String) -> Result<bool, String> {
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return Err("Transcript text is empty".to_string());
    }
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::write(&path, trimmed).map_err(|err| err.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn check_for_updates(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let current_version = app.package_info().version.to_string();
    let url = std::env::var("WHISPR_UPDATE_URL").unwrap_or_else(|_| {
        "https://api.github.com/repos/greenuni/whispr/releases/latest".to_string()
    });
    if url.trim().is_empty() {
        return Ok(None);
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|err| err.to_string())?;

    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, "whispr")
        .send()
        .map_err(|err| err.to_string())?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let payload: serde_json::Value = response
        .json::<serde_json::Value>()
        .map_err(|err| err.to_string())?;
    let tag = payload
        .get("tag_name")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string());
    let html_url = payload
        .get("html_url")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string());

    let Some(latest_version) = tag else {
        return Ok(None);
    };
    let Some(url) = html_url else {
        return Ok(None);
    };

    if is_newer_version(&latest_version, &current_version) {
        return Ok(Some(UpdateInfo {
            current_version,
            latest_version,
            url,
        }));
    }

    Ok(None)
}

#[tauri::command]
pub fn get_storage_stats(state: State<'_, Mutex<AppState>>) -> Result<StorageStats, String> {
    let guard = state
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?;
    let settings = guard.settings.clone();
    let transcript_count = guard.transcripts.len();
    drop(guard);

    let data_dir = storage::data_dir(&settings);
    let model_dir = storage::expand_tilde(&settings.transcription.model_dir);
    let db_path = storage::db_path(&settings);

    let data_bytes = dir_size(&data_dir);
    let model_bytes = dir_size(&model_dir);
    let db_bytes = file_size(&db_path);

    Ok(StorageStats {
        data_bytes,
        model_bytes,
        db_bytes,
        transcript_count,
    })
}

#[tauri::command]
pub fn list_audio_devices() -> Vec<AudioDevice> {
    audio::list_input_devices()
}

fn toggle_recording_with_state(
    app: &AppHandle,
    state: &Mutex<AppState>,
) -> Result<ToggleOutcome, String> {
    let mut guard = state
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?;

    if !guard.recording {
        let audio_settings = guard.settings.audio.clone();
        let audio_tx = guard.audio_tx.clone();
        let settings_snapshot = guard.settings.clone();
        let transcripts_snapshot = guard.transcripts.clone();
        let started_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);
        guard.recording = true;
        guard.recording_started_at_ms = Some(started_at_ms);
        drop(guard);

        if let Err(err) = audio::start_recording(&audio_tx, audio_settings, started_at_ms) {
            let mut guard = state
                .lock()
                .map_err(|_| "state lock poisoned".to_string())?;
            guard.recording = false;
            guard.recording_started_at = None;
            guard.recording_started_at_ms = None;
            guard.last_focus_window = None;
            let _ = overlay::write_state(false, None, Some(0.0));
            let _ = tray::write_error(&settings_snapshot, &transcripts_snapshot, &err);
            return Err(err);
        }

        // Pre-warm the transcription cache in the background. This removes the worst-case
        // "hang" when stopping a recording for the first transcription (Metal init + model load).
        let warm_settings = settings_snapshot.clone();
        std::thread::spawn(move || {
            let _ = transcription::ensure_context(&warm_settings);
        });

        let mut guard = state
            .lock()
            .map_err(|_| "state lock poisoned".to_string())?;
        guard.recording_started_at = Some(std::time::Instant::now());
        guard.recording_started_at_ms = Some(started_at_ms);
        guard.last_focus_window = automation::capture_focus_window();
        let _ = overlay::write_state(true, Some(started_at_ms), Some(0.0));
        return Ok(ToggleOutcome {
            result: ToggleResult {
                recording: true,
                transcript: None,
            },
            started_at_ms: Some(started_at_ms),
            focus_window_id: None,
            automation_settings: None,
        });
    }

    guard.recording = false;
    guard.recording_started_at_ms = None;
    // Stop live preview before running the (potentially expensive) final transcription so we
    // don't run two Whisper inferences concurrently.
    if let Some(cancel) = guard.preview_cancel.take() {
        cancel.store(true, Ordering::Relaxed);
    }
    let duration_ms = guard
        .recording_started_at
        .take()
        .map(|start| start.elapsed().as_millis() as u32)
        .unwrap_or(0);
    let settings = guard.settings.clone();
    let transcripts_snapshot = guard.transcripts.clone();
    let audio_tx = guard.audio_tx.clone();
    let focus_window = guard.last_focus_window.take();
    drop(guard);

    let audio = match audio::stop_recording(&audio_tx) {
        Ok(audio) => audio,
        Err(err) => {
            let _ = overlay::write_state(false, None, Some(0.0));
            let _ = tray::write_error(&settings, &transcripts_snapshot, &err);
            return Err(err);
        }
    };
    let _ = overlay::write_state(false, None, Some(0.0));

    // Fire immediately after recording has stopped and we have audio to transcribe.
    emit_transcription_started(app);

    let audio_for_save = if settings.storage.keep_audio {
        Some(audio.clone())
    } else {
        None
    };
    let text = match transcription::transcribe(&settings, audio) {
        Ok(text) => text,
        Err(err) => {
            let _ = tray::write_error(&settings, &transcripts_snapshot, &err);
            return Err(err);
        }
    };
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);

    let title = summary::generate_title(&text);
    let summary = summary::generate_summary(&text);
    let embedding = embedding::embed_text(&text);
    let id = Uuid::new_v4().to_string();
    let audio_path = if let Some(audio) = audio_for_save {
        storage::save_audio_recording(&settings, &id, &audio)
            .ok()
            .map(|path| path.to_string_lossy().to_string())
    } else {
        None
    };
    let transcript = Transcript {
        id,
        created_at,
        duration_ms,
        text,
        title,
        summary,
        tags: Vec::new(),
        audio_path,
        embedding: Some(embedding),
    };

    let mut guard = state
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?;
    guard.transcripts.insert(0, transcript.clone());
    if let Err(err) = storage::upsert_transcript(&guard.settings, &transcript) {
        guard.transcripts.retain(|item| item.id != transcript.id);
        return Err(err);
    }
    let _ = tray::write_recents(&guard.settings, &guard.transcripts, Some(created_at));

    let automation_settings = guard.settings.automation.clone();
    drop(guard);

    Ok(ToggleOutcome {
        result: ToggleResult {
            recording: false,
            transcript: Some(transcript),
        },
        started_at_ms: None,
        focus_window_id: focus_window,
        automation_settings: Some(automation_settings),
    })
}

pub fn toggle_recording_with_state_and_emit(
    app: &AppHandle,
    state: &Mutex<AppState>,
) -> Result<ToggleResult, String> {
    let outcome = toggle_recording_with_state(app, state)?;
    if outcome.result.recording {
        let preview_enabled = state
            .lock()
            .map(|guard| guard.settings.ui.live_preview_enabled)
            .unwrap_or(true);
        if preview_enabled {
            start_preview_thread(app.clone(), state);
        }
    } else {
        stop_preview_thread(state);
        emit_preview_event(app, String::new());
    }

    // Ensure the HUD window becomes visible as soon as recording starts (if enabled).
    // Some platforms may not fully initialize a hidden webview until the first show().
    if let Some(window) = app.get_webview_window("recording_hud") {
        let hud_enabled = state
            .lock()
            .ok()
            .map(|guard| guard.settings.ui.recording_hud_enabled)
            .unwrap_or(true);

        if outcome.result.recording && hud_enabled {
            let _ = window.show();
        } else {
            // Let the HUD animate out if it's running; otherwise, best-effort hide.
            let window = window.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(260));
                let _ = window.hide();
            });
        }
    }
    emit_recording_event(app, &outcome);
    emit_transcript_event(app, &outcome.result.transcript);
    app_tray::refresh_tray(app, state);

    // Run automation (paste/copy) after state + UI have been updated.
    if let (Some(transcript), Some(automation_settings)) = (
        outcome.result.transcript.clone(),
        outcome.automation_settings.clone(),
    ) {
        if automation_settings.auto_paste_enabled || automation_settings.copy_to_clipboard {
            let app = app.clone();
            let focus_window_id = outcome.focus_window_id.clone();
            std::thread::spawn(move || {
                let preserve = automation_settings.preserve_clipboard
                    && !automation_settings.copy_to_clipboard;
                let use_clipboard = automation_settings.copy_to_clipboard || preserve;

                let (delay_ms, paste_method, focus) = if automation_settings.auto_paste_enabled {
                    (
                        automation_settings.paste_delay_ms,
                        automation_settings.paste_method.clone(),
                        focus_window_id.as_deref(),
                    )
                } else {
                    (0, "clipboard_only".to_string(), None)
                };

                if let Err(err) = automation::paste_text(
                    &transcript.text,
                    delay_ms,
                    automation_settings.clipboard_restore_delay_ms,
                    use_clipboard,
                    preserve,
                    &paste_method,
                    focus,
                ) {
                    // Surface error to tray (GNOME extension) and the UI.
                    if let Ok(guard) = app.state::<Mutex<AppState>>().lock() {
                        let _ = tray::write_error(&guard.settings, &guard.transcripts, &err);
                    }
                    let _ = app.emit("automation-error", AutomationErrorEvent { message: err });
                }
            });
        }
    }

    Ok(outcome.result)
}

#[tauri::command]
pub fn toggle_recording(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
) -> Result<ToggleResult, String> {
    toggle_recording_with_state_and_emit(&app, state.inner())
}

pub fn paste_last_transcript_with_state(state: &Mutex<AppState>) -> Result<bool, String> {
    let guard = state
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?;
    let transcript = guard.transcripts.first().cloned();
    let automation_settings = guard.settings.automation.clone();
    drop(guard);

    if let Some(transcript) = transcript {
        let preserve =
            automation_settings.preserve_clipboard && !automation_settings.copy_to_clipboard;
        let use_clipboard = automation_settings.copy_to_clipboard || preserve;
        automation::paste_text(
            &transcript.text,
            automation_settings.paste_delay_ms,
            automation_settings.clipboard_restore_delay_ms,
            use_clipboard,
            preserve,
            &automation_settings.paste_method,
            None,
        )?;
        return Ok(true);
    }

    Ok(false)
}

#[tauri::command]
pub fn get_recording_level(state: State<'_, Mutex<AppState>>) -> Result<Option<f32>, String> {
    let (audio_tx, recording) = {
        let guard = state
            .lock()
            .map_err(|_| "state lock poisoned".to_string())?;
        (guard.audio_tx.clone(), guard.recording)
    };

    if !recording {
        return Ok(None);
    }

    let level = audio::recording_level(&audio_tx)?;
    Ok(Some(level))
}

#[derive(Clone, Serialize)]
pub struct RecordingState {
    pub recording: bool,
    pub started_at_ms: Option<i64>,
    pub hud_enabled: bool,
}

#[tauri::command]
pub fn get_recording_state(state: State<'_, Mutex<AppState>>) -> Result<RecordingState, String> {
    let guard = state
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?;
    Ok(RecordingState {
        recording: guard.recording,
        started_at_ms: guard.recording_started_at_ms,
        hud_enabled: guard.settings.ui.recording_hud_enabled,
    })
}

#[tauri::command]
pub fn paste_last_transcript(state: State<'_, Mutex<AppState>>) -> Result<bool, String> {
    paste_last_transcript_with_state(state.inner())
}

#[tauri::command]
pub fn list_models(state: State<'_, Mutex<AppState>>) -> Result<Vec<ModelInfo>, String> {
    let guard = state
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?;
    Ok(models::list_models(&guard.settings))
}

#[tauri::command]
pub fn download_model(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
    model_id: String,
) -> Result<Vec<ModelInfo>, String> {
    let settings = state
        .lock()
        .map(|guard| guard.settings.clone())
        .map_err(|_| "state lock poisoned".to_string())?;
    let mut last_emit = 0_u64;
    let model_clone = model_id.clone();
    models::download_model_with_progress(&settings, &model_id, |downloaded, total| {
        if downloaded.saturating_sub(last_emit) < 1_000_000 && downloaded != total {
            return;
        }
        last_emit = downloaded;
        let _ = app.emit(
            "model-download-progress",
            ModelDownloadProgress {
                id: model_clone.clone(),
                downloaded,
                total,
            },
        );
    })?;
    Ok(models::list_models(&settings))
}

#[tauri::command]
pub fn delete_model(
    state: State<'_, Mutex<AppState>>,
    model_id: String,
) -> Result<Vec<ModelInfo>, String> {
    transcription::invalidate_context_cache();
    let mut guard = state
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?;
    models::delete_model(&guard.settings, &model_id)?;

    let installed = models::list_models(&guard.settings)
        .into_iter()
        .filter(|model| model.installed)
        .collect::<Vec<_>>();

    if installed.is_empty() {
        guard.settings.transcription.model = "small.en".to_string();
    } else if !installed
        .iter()
        .any(|model| model.id == guard.settings.transcription.model)
    {
        guard.settings.transcription.model = installed[0].id.clone();
    }

    storage::save_settings(&guard.settings)?;
    transcription::invalidate_context_cache();
    Ok(models::list_models(&guard.settings))
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

fn dir_size(path: &Path) -> u64 {
    let mut total: u64 = 0;
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return 0,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        match entry.metadata() {
            Ok(meta) => {
                if meta.is_dir() {
                    total = total.saturating_add(dir_size(&path));
                } else if meta.is_file() {
                    total = total.saturating_add(meta.len());
                }
            }
            Err(_) => continue,
        }
    }

    total
}

fn parse_version(version: &str) -> Option<(u32, u32, u32)> {
    let trimmed = version.trim().trim_start_matches('v');
    let mut parts = trimmed.split(['.', '-']);
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    Some((major, minor, patch))
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

#[tauri::command]
pub fn activate_model(
    state: State<'_, Mutex<AppState>>,
    model_id: String,
) -> Result<Vec<ModelInfo>, String> {
    let mut guard = state
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?;
    models::activate_model(&mut guard.settings, &model_id)?;
    storage::save_settings(&guard.settings)?;
    transcription::invalidate_context_cache();
    Ok(models::list_models(&guard.settings))
}

#[tauri::command]
pub fn cycle_model(state: State<'_, Mutex<AppState>>) -> Result<Vec<ModelInfo>, String> {
    let mut guard = state
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?;
    let _ = models::cycle_model(&mut guard.settings)?;
    storage::save_settings(&guard.settings)?;
    transcription::invalidate_context_cache();
    Ok(models::list_models(&guard.settings))
}
