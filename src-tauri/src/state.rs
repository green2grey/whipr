use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use std::sync::mpsc::Sender;

use crate::core::audio::{self, AudioCommand};
use crate::core::storage::{load_clips, load_settings, load_transcripts_with_retention};
use crate::settings::Settings;
use crate::types::{Clip, Transcript};

pub struct AppState {
    pub settings: Settings,
    pub transcripts: Vec<Transcript>,
    pub clips: Vec<Clip>,
    pub recording: bool,
    pub recording_started_at: Option<Instant>,
    pub recording_started_at_ms: Option<i64>,
    pub preview_cancel: Option<Arc<AtomicBool>>,
    pub ui_active: Arc<AtomicBool>,
    pub audio_tx: Sender<AudioCommand>,
    pub last_focus_window: Option<String>,
}

impl AppState {
    pub fn load() -> Self {
        let settings = load_settings();
        let transcripts = load_transcripts_with_retention(&settings);
        let clips = load_clips(&settings);
        let audio_tx = audio::start_worker();
        let ui_active = Arc::new(AtomicBool::new(false));

        Self {
            settings,
            transcripts,
            clips,
            recording: false,
            recording_started_at: None,
            recording_started_at_ms: None,
            preview_cancel: None,
            ui_active,
            audio_tx,
            last_focus_window: None,
        }
    }
}
