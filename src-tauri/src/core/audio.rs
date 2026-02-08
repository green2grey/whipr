use std::collections::HashSet;
#[cfg(target_os = "linux")]
use std::ffi::CString;
#[cfg(target_os = "linux")]
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

#[cfg(target_os = "linux")]
use alsa::card::Card;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};

use crate::overlay;
use crate::settings::AudioSettings;

const MAX_RECORDING_SECONDS: u32 = 600;

#[derive(Clone, serde::Serialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub enum AudioCommand {
    Start(AudioSettings, i64, mpsc::Sender<Result<(), String>>),
    Snapshot(usize, mpsc::Sender<Result<AudioSnapshot, String>>),
    Stop(mpsc::Sender<Result<RecordedAudio, String>>),
}

pub fn start_worker() -> mpsc::Sender<AudioCommand> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut recorder: Option<Recorder> = None;
        for command in rx {
            match command {
                AudioCommand::Start(settings, started_at_ms, reply) => {
                    if recorder.is_some() {
                        let _ = reply.send(Err("Recorder already running".to_string()));
                        continue;
                    }
                    match Recorder::start(&settings, started_at_ms) {
                        Ok(active) => {
                            recorder = Some(active);
                            let _ = reply.send(Ok(()));
                        }
                        Err(err) => {
                            let _ = reply.send(Err(err));
                        }
                    }
                }
                AudioCommand::Stop(reply) => match recorder.take() {
                    Some(active) => {
                        let result = active.stop();
                        let _ = reply.send(result);
                    }
                    None => {
                        let _ = reply.send(Err("No active recorder found".to_string()));
                    }
                },
                AudioCommand::Snapshot(from_index, reply) => match recorder.as_ref() {
                    Some(active) => {
                        let result = active.snapshot(from_index);
                        let _ = reply.send(result);
                    }
                    None => {
                        let _ = reply.send(Err("No active recorder found".to_string()));
                    }
                },
            }
        }
    });
    tx
}

pub fn start_recording(
    tx: &mpsc::Sender<AudioCommand>,
    settings: AudioSettings,
    started_at_ms: i64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(AudioCommand::Start(settings, started_at_ms, reply_tx))
        .map_err(|_| "Audio worker unavailable".to_string())?;
    reply_rx
        .recv()
        .map_err(|_| "Audio worker unavailable".to_string())?
}

pub fn stop_recording(tx: &mpsc::Sender<AudioCommand>) -> Result<RecordedAudio, String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(AudioCommand::Stop(reply_tx))
        .map_err(|_| "Audio worker unavailable".to_string())?;
    reply_rx
        .recv()
        .map_err(|_| "Audio worker unavailable".to_string())?
}

pub fn snapshot_audio(
    tx: &mpsc::Sender<AudioCommand>,
    from_index: usize,
) -> Result<AudioSnapshot, String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(AudioCommand::Snapshot(from_index, reply_tx))
        .map_err(|_| "Audio worker unavailable".to_string())?;
    reply_rx
        .recv()
        .map_err(|_| "Audio worker unavailable".to_string())?
}

pub fn list_input_devices() -> Vec<AudioDevice> {
    silence_alsa_errors();
    let host = cpal::default_host();
    let mut devices = Vec::new();
    let mut seen = HashSet::new();
    let mut discovered = Vec::new();

    // Add default option
    devices.push(AudioDevice {
        id: "default".to_string(),
        name: "Default".to_string(),
        is_default: true,
    });

    // Get default device name for comparison
    let default_name = host.default_input_device().and_then(|d| d.name().ok());

    let raw_names = match host.input_devices() {
        Ok(input_devices) => input_devices
            .filter_map(|device| device.name().ok())
            .collect(),
        Err(_) => Vec::new(),
    };

    let plughw_keys: HashSet<String> = raw_names
        .iter()
        .filter_map(|name| {
            name.strip_prefix("plughw:")
                .map(|rest| format!("hw:{rest}"))
        })
        .collect();

    for name in raw_names {
        if name.eq_ignore_ascii_case("default") {
            continue;
        }
        if !should_include_device_name(&name) {
            continue;
        }
        if name.starts_with("hw:") && plughw_keys.contains(&name) {
            continue;
        }
        if !seen.insert(name.clone()) {
            continue;
        }

        let is_default = default_name.as_ref().map(|d| d == &name).unwrap_or(false);
        let label = format_device_label(&name, is_default);
        discovered.push(AudioDevice {
            id: name.clone(),
            name: label,
            is_default,
        });
    }

    discovered.sort_by(|a, b| a.name.cmp(&b.name));
    devices.extend(discovered);
    devices
}

pub fn input_device_available(input_device_id: &str) -> bool {
    silence_alsa_errors();
    let host = cpal::default_host();
    if input_device_id == "default" {
        return host.default_input_device().is_some();
    }

    match host.input_devices() {
        Ok(mut devices) => devices.any(|device| {
            device
                .name()
                .map(|name| name == input_device_id)
                .unwrap_or(false)
        }),
        Err(_) => false,
    }
}

fn should_include_device_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    let blocked_prefixes = [
        "pipewire",
        "pulse",
        "sysdefault",
        "front",
        "surround",
        "iec958",
        "spdif",
        "hdmi",
        "dmix",
        "dsnoop",
        "null",
    ];

    !blocked_prefixes
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

fn format_device_label(name: &str, is_default: bool) -> String {
    #[cfg(target_os = "linux")]
    let base = alsa_friendly_name(name).unwrap_or_else(|| name.to_string());
    #[cfg(not(target_os = "linux"))]
    let base = name.to_string();

    if is_default {
        format!("{base} (System Default)")
    } else {
        base
    }
}

#[cfg(target_os = "linux")]
fn alsa_friendly_name(name: &str) -> Option<String> {
    let (card_id, dev) = parse_alsa_device_name(name)?;
    let longname = alsa_card_longname(&card_id).unwrap_or(card_id);
    let label = match dev.as_deref() {
        Some("0") | None => longname,
        Some(dev_id) => format!("{longname} (Device {dev_id})"),
    };
    Some(label)
}

#[cfg(target_os = "linux")]
fn parse_alsa_device_name(name: &str) -> Option<(String, Option<String>)> {
    let rest = name
        .strip_prefix("plughw:")
        .or_else(|| name.strip_prefix("hw:"))?;
    let mut card: Option<String> = None;
    let mut dev: Option<String> = None;

    for part in rest.split(',') {
        if let Some(value) = part.strip_prefix("CARD=") {
            card = Some(value.to_string());
            continue;
        }
        if let Some(value) = part.strip_prefix("DEV=") {
            dev = Some(value.to_string());
            continue;
        }
        if card.is_none() {
            card = Some(part.to_string());
            continue;
        }
        if dev.is_none() {
            dev = Some(part.to_string());
        }
    }

    card.map(|card_id| (card_id, dev))
}

#[cfg(target_os = "linux")]
fn alsa_card_longname(card_id: &str) -> Option<String> {
    let cstr = CString::new(card_id).ok()?;
    let card = Card::from_str(&cstr).ok()?;
    let longname = card.get_longname().ok()?;
    let trimmed = longname
        .split_once(" at ")
        .map(|(head, _)| head.trim())
        .unwrap_or(longname.trim());
    Some(trimmed.to_string())
}

fn silence_alsa_errors() {
    #[cfg(target_os = "linux")]
    unsafe {
        extern "C" fn alsa_error_handler(
            _file: *const c_char,
            _line: c_int,
            _func: *const c_char,
            _err: c_int,
            _fmt: *const c_char,
            _arg: *mut alsa_sys::__va_list_tag,
        ) {
        }

        let _ = alsa_sys::snd_lib_error_set_local(Some(alsa_error_handler));
    }
}

pub struct Recorder {
    stream: Stream,
    samples: Arc<AudioRingBuffer>,
    sample_rate: u32,
    channels: u16,
    meter_stop: Arc<AtomicBool>,
    meter_thread: Option<thread::JoinHandle<()>>,
    active: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct RecordedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

pub struct AudioSnapshot {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
    pub total_samples: usize,
}

// Single-producer (CPAL callback) and single-reader (audio worker thread).
// Uses atomic slots so the callback never takes a mutex.
struct AudioRingBuffer {
    cap: usize,
    data: Vec<AtomicU32>,
    head: AtomicUsize, // total written samples (monotonic)
    tail: AtomicUsize, // oldest retained sample index (monotonic)
}

impl AudioRingBuffer {
    fn new(cap: usize) -> Self {
        let cap = cap.max(1);
        let data = (0..cap).map(|_| AtomicU32::new(0)).collect();
        Self {
            cap,
            data,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    fn push_slice(&self, input: &[f32]) {
        if input.is_empty() {
            return;
        }

        let mut head = self.head.load(Ordering::Relaxed);
        for &sample in input {
            let idx = head % self.cap;
            self.data[idx].store(sample.to_bits(), Ordering::Relaxed);
            head = head.wrapping_add(1);
        }

        self.head.store(head, Ordering::Release);

        let tail = self.tail.load(Ordering::Relaxed);
        let len = head.saturating_sub(tail);
        if len > self.cap {
            self.tail.store(head - self.cap, Ordering::Release);
        }
    }

    fn snapshot_from(&self, from_index: usize) -> (Vec<f32>, usize) {
        // Best-effort stable read of indices without blocking the audio thread.
        //
        // There is still a race: the producer can wrap and overwrite slots after we
        // sample head/tail but before we finish reading data. To avoid returning
        // corrupted samples, validate after the read by reloading tail (and head if
        // needed) and retry a bounded number of times when the base becomes stale.
        const SNAPSHOT_RETRIES: usize = 3;
        let cap = self.cap;
        let data = &self.data;

        for _ in 0..SNAPSHOT_RETRIES {
            let mut head = self.head.load(Ordering::Acquire);
            let mut tail = self.tail.load(Ordering::Acquire);
            for _ in 0..3 {
                let head2 = self.head.load(Ordering::Acquire);
                let tail2 = self.tail.load(Ordering::Acquire);
                if head == head2 && tail == tail2 {
                    break;
                }
                head = head2;
                tail = tail2;
            }

            let total_samples = head;
            let len = head.saturating_sub(tail).min(cap);
            let base = head.saturating_sub(len);
            let start = if from_index <= base {
                0
            } else {
                (from_index - base).min(len)
            };

            let mut out = Vec::with_capacity(len.saturating_sub(start));
            for offset in start..len {
                let abs = base + offset;
                let idx = abs % cap;
                let bits = data[idx].load(Ordering::Relaxed);
                out.push(f32::from_bits(bits));
            }

            // Detect stale reads: if the producer advanced tail past our base (or head
            // moved such that the recomputed base differs), some of the slots we read
            // may have been overwritten mid-snapshot.
            let head2 = self.head.load(Ordering::Acquire);
            let tail2 = self.tail.load(Ordering::Acquire);
            let len2 = head2.saturating_sub(tail2).min(cap);
            let base2 = head2.saturating_sub(len2);
            if tail2 > base || base2 != base {
                continue;
            }

            return (out, total_samples);
        }

        // Give up rather than returning potentially corrupted samples.
        (Vec::new(), self.head.load(Ordering::Acquire))
    }
}

impl Recorder {
    pub fn start(settings: &AudioSettings, started_at_ms: i64) -> Result<Self, String> {
        silence_alsa_errors();
        let host = cpal::default_host();
        let device = select_device(&host, &settings.input_device_id)?;
        let (config, sample_format) = select_config(&device, settings)?;

        let active = Arc::new(AtomicBool::new(true));
        let level = Arc::new(AtomicU16::new(0));
        let meter_stop = Arc::new(AtomicBool::new(false));

        let processing = AudioProcessingConfig {
            gain: db_to_gain(settings.input_gain_db),
            gate_enabled: settings.noise_gate_enabled,
            gate_threshold: settings.noise_gate_threshold.clamp(0.0, 1.0),
            vad_enabled: settings.vad_enabled,
            vad_threshold: settings.vad_threshold.clamp(0.0, 1.0),
            vad_silence_ms: settings.vad_silence_ms,
            vad_resume_ms: settings.vad_resume_ms,
        };

        let sample_rate = config.sample_rate.0.max(1);
        let channels = config.channels.max(1) as usize;
        let max_samples = (sample_rate as usize)
            .saturating_mul(channels)
            .saturating_mul(MAX_RECORDING_SECONDS as usize);
        let samples = Arc::new(AudioRingBuffer::new(max_samples));

        let stream = match sample_format {
            SampleFormat::F32 => build_stream::<f32>(
                &device,
                &config,
                samples.clone(),
                active.clone(),
                level.clone(),
                processing,
            )?,
            SampleFormat::I16 => build_stream::<i16>(
                &device,
                &config,
                samples.clone(),
                active.clone(),
                level.clone(),
                processing,
            )?,
            SampleFormat::U16 => build_stream::<u16>(
                &device,
                &config,
                samples.clone(),
                active.clone(),
                level.clone(),
                processing,
            )?,
            SampleFormat::I8 => build_stream::<i8>(
                &device,
                &config,
                samples.clone(),
                active.clone(),
                level.clone(),
                processing,
            )?,
            SampleFormat::U8 => build_stream::<u8>(
                &device,
                &config,
                samples.clone(),
                active.clone(),
                level.clone(),
                processing,
            )?,
            SampleFormat::I32 => build_stream::<i32>(
                &device,
                &config,
                samples.clone(),
                active.clone(),
                level.clone(),
                processing,
            )?,
            SampleFormat::U32 => build_stream::<u32>(
                &device,
                &config,
                samples.clone(),
                active.clone(),
                level.clone(),
                processing,
            )?,
            SampleFormat::I64 => build_stream::<i64>(
                &device,
                &config,
                samples.clone(),
                active.clone(),
                level.clone(),
                processing,
            )?,
            SampleFormat::U64 => build_stream::<u64>(
                &device,
                &config,
                samples.clone(),
                active.clone(),
                level.clone(),
                processing,
            )?,
            SampleFormat::F64 => build_stream::<f64>(
                &device,
                &config,
                samples.clone(),
                active.clone(),
                level.clone(),
                processing,
            )?,
            _ => return Err("Unsupported audio sample format".to_string()),
        };

        stream.play().map_err(|err| err.to_string())?;

        let meter_level = level.clone();
        let meter_stop_flag = meter_stop.clone();
        let meter_thread = thread::spawn(move || {
            while !meter_stop_flag.load(Ordering::Relaxed) {
                let raw = meter_level.load(Ordering::Relaxed) as f32;
                let normalized = (raw / 1000.0).clamp(0.0, 1.0);
                let _ = overlay::write_state(true, Some(started_at_ms), Some(normalized));
                thread::sleep(Duration::from_millis(120));
            }
        });

        Ok(Self {
            stream,
            samples,
            sample_rate: config.sample_rate.0,
            channels: config.channels,
            meter_stop,
            meter_thread: Some(meter_thread),
            active,
        })
    }

    pub fn stop(mut self) -> Result<RecordedAudio, String> {
        self.meter_stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.meter_thread.take() {
            let _ = handle.join();
        }
        // Stop accepting callback writes before pausing/dropping the stream.
        self.active.store(false, Ordering::Relaxed);
        let _ = self.stream.pause();
        let (samples, _total_samples) = self.samples.snapshot_from(0);

        Ok(RecordedAudio {
            samples,
            sample_rate: self.sample_rate,
            channels: self.channels,
        })
    }

    pub fn snapshot(&self, from_index: usize) -> Result<AudioSnapshot, String> {
        let (samples, total_samples) = self.samples.snapshot_from(from_index);
        Ok(AudioSnapshot {
            samples,
            sample_rate: self.sample_rate,
            channels: self.channels,
            total_samples,
        })
    }
}

fn select_device(host: &cpal::Host, input_device_id: &str) -> Result<cpal::Device, String> {
    if input_device_id != "default" {
        if let Ok(mut devices) = host.input_devices() {
            if let Some(device) = devices.find(|device| {
                device
                    .name()
                    .map(|name| name == input_device_id)
                    .unwrap_or(false)
            }) {
                return Ok(device);
            }
        }

        // Avoid silently recording from a different microphone than the user selected.
        return Err(format!("Input device not found: {input_device_id}"));
    }

    host.default_input_device()
        .ok_or_else(|| "No input audio device available".to_string())
}

fn select_config(
    device: &cpal::Device,
    settings: &AudioSettings,
) -> Result<(StreamConfig, SampleFormat), String> {
    let mut fallback: Option<(StreamConfig, SampleFormat)> = None;
    let target_rate = settings.sample_rate_hz;
    let target_channels = settings.channels;

    let configs = device
        .supported_input_configs()
        .map_err(|err| err.to_string())?;

    for config_range in configs {
        let min_rate = config_range.min_sample_rate().0;
        let max_rate = config_range.max_sample_rate().0;
        let sample_rate = clamp_sample_rate(target_rate, min_rate, max_rate);
        let config = config_range.with_sample_rate(cpal::SampleRate(sample_rate));
        let sample_format = config.sample_format();

        if fallback.is_none() {
            fallback = Some((to_stream_config(&config), sample_format));
        }

        if config.channels() == target_channels {
            return Ok((to_stream_config(&config), sample_format));
        }
    }

    if let Some((config, sample_format)) = fallback {
        return Ok((config, sample_format));
    }

    let config = device
        .default_input_config()
        .map_err(|err| err.to_string())?;
    let sample_format = config.sample_format();
    Ok((config.into(), sample_format))
}

fn to_stream_config(config: &cpal::SupportedStreamConfig) -> StreamConfig {
    StreamConfig {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        buffer_size: BufferSize::Default,
    }
}

fn clamp_sample_rate(target: u32, min: u32, max: u32) -> u32 {
    if target < min {
        min
    } else if target > max {
        max
    } else {
        target
    }
}

fn db_to_gain(db: f32) -> f32 {
    if db == 0.0 {
        return 1.0;
    }
    10.0_f32.powf(db / 20.0)
}

#[derive(Clone, Copy, Debug)]
struct AudioProcessingConfig {
    gain: f32,
    gate_enabled: bool,
    gate_threshold: f32,
    vad_enabled: bool,
    vad_threshold: f32,
    vad_silence_ms: u32,
    vad_resume_ms: u32,
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    samples: Arc<AudioRingBuffer>,
    active: Arc<AtomicBool>,
    level: Arc<AtomicU16>,
    processing: AudioProcessingConfig,
) -> Result<Stream, String>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let AudioProcessingConfig {
        gain,
        gate_enabled,
        gate_threshold,
        vad_enabled,
        vad_threshold,
        vad_silence_ms,
        vad_resume_ms,
    } = processing;

    let err_fn = |err| {
        eprintln!("Audio input stream error: {err}");
    };

    let mut vad_state = VadState {
        active: !vad_enabled,
        silence_ms: 0,
        speech_ms: 0,
    };
    let sample_rate = config.sample_rate.0.max(1);
    let channels = config.channels.max(1) as usize;
    let mut scratch: Vec<f32> = Vec::new();

    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                if !active.load(Ordering::Relaxed) {
                    return;
                }
                if data.is_empty() {
                    return;
                }

                scratch.clear();
                scratch.reserve(data.len());

                let mut sum = 0.0_f32;
                for sample in data {
                    let value = f32::from_sample(*sample) * gain;
                    sum += value * value;
                    scratch.push(value);
                }

                let rms = (sum / data.len() as f32).sqrt();
                let normalized = (rms * 2.5).clamp(0.0, 1.0);
                level.store((normalized * 1000.0) as u16, Ordering::Relaxed);

                if vad_enabled {
                    let frames = data.len() / channels;
                    let chunk_ms = if sample_rate > 0 {
                        ((frames as u64).saturating_mul(1000) / sample_rate as u64) as u32
                    } else {
                        0
                    };
                    let speech = rms >= vad_threshold;

                    if vad_state.active {
                        if speech {
                            vad_state.silence_ms = 0;
                        } else {
                            vad_state.silence_ms = vad_state.silence_ms.saturating_add(chunk_ms);
                            if vad_state.silence_ms >= vad_silence_ms {
                                vad_state.active = false;
                                vad_state.speech_ms = 0;
                            }
                        }
                    } else if speech {
                        vad_state.speech_ms = vad_state.speech_ms.saturating_add(chunk_ms);
                        if vad_state.speech_ms >= vad_resume_ms {
                            vad_state.active = true;
                            vad_state.silence_ms = 0;
                        }
                    } else {
                        vad_state.speech_ms = 0;
                    }

                    if !vad_state.active {
                        return;
                    }
                }

                if gate_enabled && rms < gate_threshold {
                    return;
                }

                if scratch.is_empty() {
                    return;
                }

                samples.push_slice(&scratch);
            },
            err_fn,
            None,
        )
        .map_err(|err| err.to_string())
}

struct VadState {
    active: bool,
    silence_ms: u32,
    speech_ms: u32,
}
