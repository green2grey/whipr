#[cfg(target_os = "linux")]
mod linux_overlay {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde::Serialize;

    const STATE_DIR: &str = "whispr";
    const STATE_FILE: &str = "overlay.json";

    #[derive(Serialize)]
    struct OverlayState {
        recording: bool,
        started_at_ms: Option<i64>,
        updated_at_ms: i64,
        level: Option<f32>,
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
        state_dir().join(STATE_FILE)
    }

    pub fn write_state(
        recording: bool,
        started_at_ms: Option<i64>,
        level: Option<f32>,
    ) -> Result<(), String> {
        let path = state_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }

        let state = OverlayState {
            recording,
            started_at_ms,
            updated_at_ms: now_ms(),
            level,
        };
        let payload = serde_json::to_string(&state).map_err(|err| err.to_string())?;
        let tmp_path = path.with_extension("tmp");
        fs::write(&tmp_path, payload).map_err(|err| err.to_string())?;
        fs::rename(&tmp_path, &path).map_err(|err| err.to_string())
    }
}

#[cfg(target_os = "linux")]
pub use linux_overlay::write_state;

#[cfg(not(target_os = "linux"))]
pub fn write_state(
    _recording: bool,
    _started_at_ms: Option<i64>,
    _level: Option<f32>,
) -> Result<(), String> {
    Ok(())
}
