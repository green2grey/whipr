use std::sync::{Mutex, OnceLock};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::core::audio::RecordedAudio;
use crate::core::models;
use crate::settings::Settings;

const TARGET_SAMPLE_RATE: u32 = 16_000;
const PREVIEW_MAX_SECONDS: f32 = 10.0;
const GPU_FALLBACK_PREFIX: &str = "GPU init failed, falling back to CPU: ";

static LAST_GPU_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();

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

pub fn transcribe_preview_with_context(
    ctx: &WhisperContext,
    settings: &Settings,
    audio: RecordedAudio,
) -> Result<String, String> {
    let audio = trim_audio(audio, PREVIEW_MAX_SECONDS);
    transcribe_with_context(ctx, settings, audio, Some(1))
}

pub fn build_context(settings: &Settings) -> Result<WhisperContext, String> {
    let model_path = models::resolve_model_path(settings, &settings.transcription.model)?;
    let model_path = model_path
        .to_str()
        .ok_or_else(|| "Model path is not valid UTF-8".to_string())?
        .to_string();
    let gpu_supported = cfg!(feature = "_gpu");
    let wants_gpu = settings.transcription.use_gpu && gpu_supported;

    let (ctx, _used_gpu) = build_with_fallback(wants_gpu, |use_gpu| {
        build_context_with_params(&model_path, use_gpu)
    })?;
    Ok(ctx)
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
        if cached.used_gpu == false && wants_gpu {
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

    let mut mono = to_mono(&audio);
    if audio.sample_rate != TARGET_SAMPLE_RATE {
        mono = resample_linear(&mono, audio.sample_rate, TARGET_SAMPLE_RATE);
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

fn to_mono(audio: &RecordedAudio) -> Vec<f32> {
    if audio.channels <= 1 {
        return audio.samples.clone();
    }

    let channels = audio.channels as usize;
    let frames = audio.samples.len() / channels;
    let mut mono = Vec::with_capacity(frames);

    for frame in 0..frames {
        let mut sum = 0.0_f32;
        for ch in 0..channels {
            sum += audio.samples[frame * channels + ch];
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
