#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "macos")]
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::core::audio::RecordedAudio;
use crate::core::models;
use crate::settings::Settings;

const TARGET_SAMPLE_RATE: u32 = 16_000;
const PREVIEW_MAX_SECONDS: f32 = 10.0;
const GPU_FALLBACK_PREFIX: &str = "GPU init failed, falling back to CPU: ";

static LAST_GPU_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static GPU_NAME: OnceLock<Option<String>> = OnceLock::new();

pub fn detect_gpu_name() -> Option<String> {
    GPU_NAME
        .get_or_init(detect_gpu_name_uncached)
        .as_ref()
        .cloned()
}

#[cfg(target_os = "linux")]
fn detect_gpu_name_uncached() -> Option<String> {
    detect_linux_nvidia_gpu_name().or_else(detect_linux_drm_gpu_name)
}

#[cfg(target_os = "linux")]
fn detect_linux_nvidia_gpu_name() -> Option<String> {
    let entries = fs::read_dir("/proc/driver/nvidia/gpus").ok()?;
    for entry in entries.flatten() {
        let info_path = entry.path().join("information");
        if let Ok(text) = fs::read_to_string(info_path) {
            for line in text.lines() {
                let trimmed = line.trim();
                if let Some(value) = trimmed
                    .strip_prefix("Model:")
                    .or_else(|| trimmed.strip_prefix("Model"))
                {
                    let name = value.trim().trim_start_matches(':').trim();
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn detect_linux_drm_gpu_name() -> Option<String> {
    let entries = fs::read_dir("/sys/class/drm").ok()?;
    let mut candidates: Vec<(u8, String)> = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // Prefer real DRM cards; skip render nodes / connectors.
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }

        let dev_path = entry.path().join("device");
        let vendor_text = fs::read_to_string(dev_path.join("vendor")).ok();
        let device_text = fs::read_to_string(dev_path.join("device")).ok();
        let (vendor, device) = match (vendor_text, device_text) {
            (Some(vendor), Some(device)) => (vendor, device),
            _ => continue,
        };

        let vendor = vendor.trim().trim_start_matches("0x");
        let device = device.trim().trim_start_matches("0x");
        let vendor_id = match u16::from_str_radix(vendor, 16) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let device_id = match u16::from_str_radix(device, 16) {
            Ok(value) => value,
            Err(_) => continue,
        };

        let (rank, vendor_name) = match vendor_id {
            0x10de => (0, "NVIDIA"),
            0x1002 => (1, "AMD"),
            0x8086 => (2, "Intel"),
            _ => (3, "GPU"),
        };
        candidates.push((
            rank,
            format!("{vendor_name} ({vendor_id:04x}:{device_id:04x})"),
        ));
    }

    candidates.sort_by_key(|(rank, _)| *rank);
    candidates.first().map(|(_, value)| value.clone())
}

#[cfg(target_os = "windows")]
fn detect_gpu_name_uncached() -> Option<String> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
    };
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    // Best-effort; if COM is already initialized in a different mode, we can still try DXGI.
    let com_initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };

    let result = (|| {
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1().ok()? };
        for index in 0..64_u32 {
            let adapter: IDXGIAdapter1 = match unsafe { factory.EnumAdapters1(index) } {
                Ok(adapter) => adapter,
                Err(_) => break,
            };

            let desc = match unsafe { adapter.GetDesc1() } {
                Ok(desc) => desc,
                Err(_) => continue,
            };

            if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                continue;
            }

            let end = desc
                .Description
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(desc.Description.len());
            let name = String::from_utf16_lossy(&desc.Description[..end])
                .trim()
                .to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
        None
    })();

    if com_initialized {
        unsafe {
            CoUninitialize();
        }
    }

    result
}

#[cfg(target_os = "macos")]
fn detect_gpu_name_uncached() -> Option<String> {
    let output = Command::new("system_profiler")
        .args(["SPDisplaysDataType"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("Chipset Model:") {
            let name = value.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
        if let Some(value) = trimmed.strip_prefix("Model:") {
            let name = value.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }

    None
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn detect_gpu_name_uncached() -> Option<String> {
    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContextKey {
    model_path: String,
    wants_gpu: bool,
}

struct CachedContext {
    key: ContextKey,
    ctx: WhisperContext,
    used_gpu: bool,
}

static CONTEXT_CACHE: OnceLock<Mutex<Option<CachedContext>>> = OnceLock::new();

pub fn transcribe(settings: &Settings, audio: RecordedAudio) -> Result<String, String> {
    with_cached_context(settings, |ctx| {
        transcribe_with_context(ctx, settings, audio, None)
    })
}

/// Ensure the global Whisper context cache is initialized for the current settings.
///
/// On GPU builds (Metal/CUDA/etc), the first context initialization can be noticeably slow due to
/// backend setup and model loading. Calling this in a background thread (for example, when
/// recording starts) shifts that cost away from the "stop recording -> transcribe" hot path.
pub fn ensure_context(settings: &Settings) -> Result<(), String> {
    with_cached_context(settings, |_ctx| Ok(()))
}

/// Best-effort preview transcription using the shared cached context.
///
/// This avoids loading the model a second time for preview vs. final transcription.
pub fn transcribe_preview(settings: &Settings, audio: RecordedAudio) -> Result<String, String> {
    with_cached_context(settings, |ctx| {
        transcribe_preview_with_context(ctx, settings, audio)
    })
}

pub fn transcribe_preview_with_context(
    ctx: &WhisperContext,
    settings: &Settings,
    audio: RecordedAudio,
) -> Result<String, String> {
    let audio = trim_audio(audio, PREVIEW_MAX_SECONDS);
    transcribe_with_context(ctx, settings, audio, Some(1))
}

pub fn last_gpu_error() -> Option<String> {
    let store = LAST_GPU_ERROR.get_or_init(|| Mutex::new(None));
    store.lock().ok().and_then(|guard| guard.clone())
}

fn set_last_gpu_error(value: Option<String>) {
    let store = LAST_GPU_ERROR.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = store.lock() {
        *guard = value;
    }
}

pub fn clear_last_gpu_error() {
    set_last_gpu_error(None);
}

pub fn invalidate_context_cache() {
    let cache = CONTEXT_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = cache.lock() {
        *guard = None;
    }
}

fn with_cached_context<T, F>(settings: &Settings, f: F) -> Result<T, String>
where
    F: FnOnce(&WhisperContext) -> Result<T, String>,
{
    let model_path = models::resolve_model_path(settings, &settings.transcription.model)?;
    let model_path = model_path
        .to_str()
        .ok_or_else(|| "Model path is not valid UTF-8".to_string())?
        .to_string();
    let gpu_supported = cfg!(feature = "_gpu");
    let wants_gpu = settings.transcription.use_gpu && gpu_supported;

    let key = ContextKey {
        model_path: model_path.clone(),
        wants_gpu,
    };

    let cache = CONTEXT_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache
        .lock()
        .map_err(|_| "transcription context cache lock poisoned".to_string())?;

    let should_rebuild = match guard.as_ref() {
        Some(cached) => cached.key != key,
        None => true,
    };

    if should_rebuild {
        let (ctx, used_gpu) = build_with_fallback(wants_gpu, |use_gpu| {
            build_context_with_params(&model_path, use_gpu)
        })?;
        *guard = Some(CachedContext { key, ctx, used_gpu });
    }

    // If GPU init failed earlier, we cache the CPU context under the wants_gpu=true key to avoid
    // retrying GPU on every transcription. Users can retry by toggling GPU (which should invalidate).
    if let Some(cached) = guard.as_ref() {
        if !cached.used_gpu && wants_gpu {
            // Keep last GPU error visible; nothing to do here.
        }
    }

    let ctx = &guard
        .as_ref()
        .ok_or_else(|| "Failed to build transcription context".to_string())?
        .ctx;

    f(ctx)
}

fn build_context_with_params(model_path: &str, use_gpu: bool) -> Result<WhisperContext, String> {
    let mut ctx_params = WhisperContextParameters::default();
    ctx_params.use_gpu(use_gpu);
    WhisperContext::new_with_params(model_path, ctx_params).map_err(|err| err.to_string())
}

fn build_with_fallback<T, F>(wants_gpu: bool, mut builder: F) -> Result<(T, bool), String>
where
    F: FnMut(bool) -> Result<T, String>,
{
    if wants_gpu {
        match builder(true) {
            Ok(value) => {
                set_last_gpu_error(None);
                return Ok((value, true));
            }
            Err(err) => {
                let message = format!("{GPU_FALLBACK_PREFIX}{err}");
                set_last_gpu_error(Some(message.clone()));
                eprintln!("{message}");
            }
        }
    }

    builder(false).map(|value| (value, false))
}

fn transcribe_with_context(
    ctx: &WhisperContext,
    settings: &Settings,
    audio: RecordedAudio,
    thread_override: Option<u32>,
) -> Result<String, String> {
    if audio.samples.is_empty() {
        return Err("No audio captured".to_string());
    }

    let mut state = ctx.create_state().map_err(|err| err.to_string())?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

    if !settings.transcription.language.is_empty() {
        params.set_language(Some(settings.transcription.language.as_str()));
    }

    let thread_count = resolve_thread_count(settings, thread_override);

    params.set_n_threads(thread_count as i32);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    let prompt = settings.transcription.custom_vocab.trim();
    if !prompt.is_empty() {
        let sanitized = prompt.replace('\0', " ");
        params.set_initial_prompt(&sanitized);
    }

    // `RecordedAudio` is already owned here, so avoid cloning the full buffer on the
    // common mono path.
    let RecordedAudio {
        samples,
        sample_rate,
        channels,
    } = audio;

    let mut mono = if channels <= 1 {
        samples
    } else {
        to_mono(&samples, channels)
    };

    if sample_rate != TARGET_SAMPLE_RATE {
        mono = resample_linear(&mono, sample_rate, TARGET_SAMPLE_RATE);
    }

    if mono.is_empty() {
        return Err("No usable audio after conversion".to_string());
    }

    state.full(params, &mono).map_err(|err| err.to_string())?;

    let mut text = String::new();
    for segment in state.as_iter() {
        let segment_text = segment.to_string();
        let trimmed = segment_text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(trimmed);
    }

    Ok(text)
}

pub fn resolve_thread_count(settings: &Settings, thread_override: Option<u32>) -> u32 {
    if let Some(override_count) = thread_override {
        return override_count.max(1);
    }
    if settings.transcription.threads > 0 {
        return settings.transcription.threads;
    }
    std::thread::available_parallelism()
        .map(|count| count.get() as u32)
        .unwrap_or(4)
}

fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    let channels = channels.max(1) as usize;
    if channels <= 1 {
        return samples.to_vec();
    }

    let frames = samples.len() / channels;
    let mut mono = Vec::with_capacity(frames);

    for frame in 0..frames {
        let mut sum = 0.0_f32;
        for ch in 0..channels {
            sum += samples[frame * channels + ch];
        }
        mono.push(sum / channels as f32);
    }

    mono
}

fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if input.is_empty() || from_rate == 0 || to_rate == 0 {
        return Vec::new();
    }
    if from_rate == to_rate {
        return input.to_vec();
    }

    let ratio = to_rate as f32 / from_rate as f32;
    let output_len = (input.len() as f32 * ratio).round() as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let position = i as f32 / ratio;
        let index = position.floor() as usize;
        let frac = position - index as f32;
        let s0 = input.get(index).copied().unwrap_or(0.0);
        let s1 = input.get(index + 1).copied().unwrap_or(s0);
        output.push(s0 + (s1 - s0) * frac);
    }

    output
}

fn trim_audio(audio: RecordedAudio, max_seconds: f32) -> RecordedAudio {
    if max_seconds <= 0.0 || audio.samples.is_empty() {
        return audio;
    }

    let frames = audio.samples.len() / audio.channels.max(1) as usize;
    let max_frames = (audio.sample_rate as f32 * max_seconds).round() as usize;
    if frames <= max_frames {
        return audio;
    }

    let start_frame = frames.saturating_sub(max_frames);
    let start_index = start_frame * audio.channels as usize;
    let samples = audio.samples[start_index..].to_vec();

    RecordedAudio {
        samples,
        sample_rate: audio.sample_rate,
        channels: audio.channels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static GPU_ERROR_TEST_GUARD: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn gpu_fallback_records_error() {
        let _guard = GPU_ERROR_TEST_GUARD
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap();
        set_last_gpu_error(None);
        let result = build_with_fallback(true, |use_gpu| {
            if use_gpu {
                Err("gpu init error".to_string())
            } else {
                Ok(42)
            }
        })
        .expect("fallback");

        assert_eq!(result, (42, false));
        let err = last_gpu_error().unwrap_or_default();
        assert!(err.contains("gpu init error"));
    }

    #[test]
    fn gpu_success_clears_error() {
        let _guard = GPU_ERROR_TEST_GUARD
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap();
        set_last_gpu_error(Some("previous error".to_string()));
        let result = build_with_fallback(true, |_use_gpu| Ok("ok")).expect("gpu ok");

        assert_eq!(result, ("ok", true));
        assert!(last_gpu_error().is_none());
    }
}
