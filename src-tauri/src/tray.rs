#[cfg(target_os = "linux")]
mod linux_tray {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde::Serialize;

    use crate::settings::Settings;
    use crate::types::Transcript;

    const STATE_DIR: &str = "whispr";
    const TRAY_FILE: &str = "tray.json";
    const MAX_RECENTS: usize = 8;
    const PREVIEW_LEN: usize = 40;

    #[derive(Serialize)]
    struct TrayHotkeys {
        record_toggle: String,
        paste_last: String,
        open_app: String,
    }

    #[derive(Serialize)]
    struct TrayTranscript {
        id: String,
        created_at: i64,
        duration_ms: u32,
        text: String,
        preview: String,
    }

    #[derive(Serialize)]
    struct TrayState {
        updated_at_ms: i64,
        last_transcript_at_ms: Option<i64>,
        last_error_at_ms: Option<i64>,
        last_error: Option<String>,
        recent: Vec<TrayTranscript>,
        hotkeys: TrayHotkeys,
    }

    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0)
    }

    fn state_dir() -> PathBuf {
        if let Some(dir) = std::env::var_os("XDG_STATE_HOME") {
            return PathBuf::from(dir).join(STATE_DIR);
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".local/state").join(STATE_DIR);
        }
        std::env::temp_dir().join(STATE_DIR)
    }

    fn state_path() -> PathBuf {
        state_dir().join(TRAY_FILE)
    }

    fn preview_text(text: &str) -> String {
        let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            return "Empty transcript".to_string();
        }
        let length = collapsed.chars().count();
        if length <= PREVIEW_LEN {
            return collapsed;
        }
        let mut preview = collapsed.chars().take(PREVIEW_LEN).collect::<String>();
        preview.push_str("...");
        preview
    }

    fn build_recent(transcripts: &[Transcript]) -> Vec<TrayTranscript> {
        transcripts
            .iter()
            .take(MAX_RECENTS)
            .map(|transcript| TrayTranscript {
                id: transcript.id.clone(),
                created_at: transcript.created_at,
                duration_ms: transcript.duration_ms,
                text: transcript.text.clone(),
                preview: preview_text(&transcript.text),
            })
            .collect()
    }

    fn write_state(
        settings: &Settings,
        transcripts: &[Transcript],
        last_transcript_at_ms: Option<i64>,
        last_error_at_ms: Option<i64>,
        last_error: Option<String>,
    ) -> Result<(), String> {
        let path = state_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }

        let hotkeys = TrayHotkeys {
            record_toggle: settings.hotkeys.record_toggle.clone(),
            paste_last: settings.hotkeys.paste_last.clone(),
            open_app: settings.hotkeys.open_app.clone(),
        };

        let state = TrayState {
            updated_at_ms: now_ms(),
            last_transcript_at_ms,
            last_error_at_ms,
            last_error,
            recent: build_recent(transcripts),
            hotkeys,
        };

        let payload = serde_json::to_string(&state).map_err(|err| err.to_string())?;
        let tmp_path = path.with_extension("tmp");
        fs::write(&tmp_path, payload).map_err(|err| err.to_string())?;
        fs::rename(&tmp_path, &path).map_err(|err| err.to_string())
    }

    pub fn write_recents(
        settings: &Settings,
        transcripts: &[Transcript],
        last_transcript_at_ms: Option<i64>,
    ) -> Result<(), String> {
        write_state(settings, transcripts, last_transcript_at_ms, None, None)
    }

    pub fn write_error(
        settings: &Settings,
        transcripts: &[Transcript],
        message: &str,
    ) -> Result<(), String> {
        write_state(
            settings,
            transcripts,
            transcripts.first().map(|item| item.created_at),
            Some(now_ms()),
            Some(message.to_string()),
        )
    }
}

#[cfg(target_os = "linux")]
pub use linux_tray::{write_error, write_recents};

#[cfg(not(target_os = "linux"))]
pub fn write_recents(
    _settings: &crate::settings::Settings,
    _transcripts: &[crate::types::Transcript],
    _last_transcript_at_ms: Option<i64>,
) -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn write_error(
    _settings: &crate::settings::Settings,
    _transcripts: &[crate::types::Transcript],
    _message: &str,
) -> Result<(), String> {
    Ok(())
}
