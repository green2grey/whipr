use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub audio: AudioSettings,
    pub hotkeys: HotkeySettings,
    pub transcription: TranscriptionSettings,
    pub automation: AutomationSettings,
    pub storage: StorageSettings,
    pub app: AppSettings,
    pub ui: UiSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    pub input_device_id: String,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub input_gain_db: f32,
    pub noise_gate_enabled: bool,
    pub noise_gate_threshold: f32,
    pub vad_enabled: bool,
    pub vad_threshold: f32,
    pub vad_silence_ms: u32,
    pub vad_resume_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeySettings {
    pub record_toggle: String,
    pub paste_last: String,
    pub open_app: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionSettings {
    pub model: String,
    pub model_dir: String,
    pub threads: u32,
    pub language: String,
    pub custom_vocab: String,
    pub use_gpu: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationSettings {
    pub auto_paste_enabled: bool,
    pub paste_delay_ms: u32,
    pub copy_to_clipboard: bool,
    // If true, auto-paste will temporarily use the clipboard then restore the previous contents.
    // This is ignored when copy_to_clipboard is enabled (since the user explicitly wants the
    // transcript in the clipboard after completion).
    pub preserve_clipboard: bool,
    /// Delay (in milliseconds) to wait after triggering a clipboard-based paste before restoring
    /// the user's previous clipboard contents when `preserve_clipboard` is enabled.
    ///
    /// Default: 90ms. This is a pragmatic compromise: long enough for most apps to read the
    /// clipboard on paste, while still feeling effectively instantaneous.
    ///
    /// Increase this if you see intermittent failures where the paste uses the wrong clipboard
    /// contents (a race), especially on slower machines, under high CPU load, or when pasting via
    /// remote desktop / VMs.
    ///
    /// Values of 0 fall back to the default. Values above 2000ms are clamped.
    pub clipboard_restore_delay_ms: u64,
    pub paste_method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSettings {
    pub data_dir: String,
    pub keep_audio: bool,
    pub retention_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub launch_on_login: bool,
    pub start_in_tray: bool,
    pub close_to_tray: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiSettings {
    pub list_compact: bool,
    pub onboarding_seen: bool,
    pub live_preview_enabled: bool,
    pub recording_hud_enabled: bool,
}

fn default_data_dir_path() -> PathBuf {
    if cfg!(target_os = "windows") {
        if let Some(base) = std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .or_else(|| std::env::var_os("USERPROFILE"))
        {
            return PathBuf::from(base).join("whispr");
        }
    }

    if cfg!(target_os = "macos") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("whispr");
        }
    }

    if cfg!(target_os = "linux") {
        if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(dir).join("whispr");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("whispr");
        }
    }

    std::env::temp_dir().join("whispr")
}

fn default_data_dir() -> String {
    default_data_dir_path().to_string_lossy().to_string()
}

fn default_model_dir() -> String {
    default_data_dir_path()
        .join("models")
        .to_string_lossy()
        .to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            audio: AudioSettings {
                input_device_id: "default".to_string(),
                sample_rate_hz: 16_000,
                channels: 1,
                input_gain_db: 0.0,
                noise_gate_enabled: false,
                noise_gate_threshold: 0.02,
                vad_enabled: false,
                vad_threshold: 0.02,
                vad_silence_ms: 800,
                vad_resume_ms: 200,
            },
            hotkeys: HotkeySettings {
                // Avoid macOS reserved Option+Command+Space (Spotlight / Finder search).
                record_toggle: "CommandOrControl+Shift+Space".to_string(),
                paste_last: "CommandOrControl+Alt+V".to_string(),
                open_app: "CommandOrControl+Alt+O".to_string(),
            },
            transcription: TranscriptionSettings {
                model: "small.en".to_string(),
                model_dir: default_model_dir(),
                threads: 0,
                language: "en".to_string(),
                custom_vocab: String::new(),
                use_gpu: false,
            },
            automation: AutomationSettings {
                auto_paste_enabled: true,
                paste_delay_ms: 250,
                copy_to_clipboard: true,
                preserve_clipboard: false,
                clipboard_restore_delay_ms: 90,
                paste_method: "auto".to_string(),
            },
            storage: StorageSettings {
                data_dir: default_data_dir(),
                keep_audio: false,
                retention_days: 0,
            },
            app: AppSettings {
                launch_on_login: false,
                start_in_tray: true,
                close_to_tray: true,
            },
            ui: UiSettings {
                list_compact: false,
                onboarding_seen: false,
                live_preview_enabled: true,
                recording_hud_enabled: true,
            },
        }
    }
}
