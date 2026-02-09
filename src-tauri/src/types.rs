use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    pub id: String,
    pub created_at: i64,
    pub duration_ms: u32,
    pub text: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub audio_path: Option<String>,
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub id: String,
    pub created_at: i64,
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub transcript_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceInfo {
    pub gpu_supported: bool,
    pub gpu_enabled: bool,
    pub thread_count: u32,
    #[serde(default)]
    pub gpu_error: Option<String>,
    #[serde(default)]
    pub gpu_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub audio_seconds: f32,
    pub duration_ms: u64,
    pub realtime_factor: f32,
    pub text_length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportFailure {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub transcripts: Vec<Transcript>,
    pub failures: Vec<ImportFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToggleResult {
    pub recording: bool,
    pub transcript: Option<Transcript>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInfo {
    pub session_type: String,
    pub hotkeys_supported: bool,
    pub paste_method: String,
    pub missing_helpers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacosPermissions {
    pub accessibility: bool,
    pub input_monitoring: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    pub data_bytes: u64,
    pub model_bytes: u64,
    pub db_bytes: u64,
    pub transcript_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub url: String,
}
