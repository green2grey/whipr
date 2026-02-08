use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::core::audio::RecordedAudio;
use crate::settings::Settings;
use crate::types::{Clip, Transcript};

const DB_FILE: &str = "whispr.db";
const LEGACY_SETTINGS_FILE: &str = "settings.json";
const LEGACY_TRANSCRIPTS_FILE: &str = "transcripts.json";
const TRANSCRIPT_SOURCE: &str = "mic";
const MILLIS_PER_DAY: i64 = 86_400_000;

fn encode_tags(tags: &[String]) -> Result<Option<String>, String> {
    if tags.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            serde_json::to_string(tags).map_err(|err| err.to_string())?,
        ))
    }
}

fn encode_embedding(embedding: &Option<Vec<f32>>) -> Result<Option<String>, String> {
    embedding
        .as_ref()
        .map(|vector| serde_json::to_string(vector))
        .transpose()
        .map_err(|err| err.to_string())
}

pub fn expand_tilde(path: &str) -> PathBuf {
    let stripped = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\"));

    if let Some(stripped) = stripped {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .or_else(|| {
                let drive = std::env::var_os("HOMEDRIVE");
                let path = std::env::var_os("HOMEPATH");
                match (drive, path) {
                    (Some(drive), Some(path)) => {
                        let mut combined = PathBuf::from(drive);
                        combined.push(path);
                        Some(combined.into_os_string())
                    }
                    _ => None,
                }
            });

        if let Some(home) = home {
            return PathBuf::from(home).join(stripped);
        }
    }

    PathBuf::from(path)
}

pub fn data_dir(settings: &Settings) -> PathBuf {
    expand_tilde(&settings.storage.data_dir)
}

pub fn db_path(settings: &Settings) -> PathBuf {
    data_dir(settings).join(DB_FILE)
}

pub fn audio_dir(settings: &Settings) -> PathBuf {
    data_dir(settings).join("audio")
}

pub fn audio_path(settings: &Settings, transcript_id: &str) -> PathBuf {
    audio_dir(settings).join(format!("{transcript_id}.wav"))
}

pub fn save_audio_recording(
    settings: &Settings,
    transcript_id: &str,
    audio: &RecordedAudio,
) -> Result<PathBuf, String> {
    if audio.sample_rate == 0 || audio.channels == 0 {
        return Err("Invalid audio metadata".to_string());
    }
    let dir = audio_dir(settings);
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let path = audio_path(settings, transcript_id);
    let spec = WavSpec {
        channels: audio.channels,
        sample_rate: audio.sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).map_err(|err| err.to_string())?;
    for sample in audio.samples.iter() {
        let clamped = sample.clamp(-1.0, 1.0);
        let value = (clamped * i16::MAX as f32) as i16;
        writer.write_sample(value).map_err(|err| err.to_string())?;
    }
    writer.finalize().map_err(|err| err.to_string())?;
    Ok(path)
}

pub fn delete_audio_file(settings: &Settings, path: &str) -> Result<(), String> {
    let data_dir = data_dir(settings);
    let path_buf = PathBuf::from(path);
    if path_buf.starts_with(&data_dir) && path_buf.exists() {
        fs::remove_file(&path_buf).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let contents = fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&contents).map_err(|err| err.to_string())
}

fn open_connection(path: &Path) -> Result<Connection, String> {
    ensure_dir(path)?;
    Connection::open(path).map_err(|err| err.to_string())
}

fn ensure_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS transcripts (
        id TEXT PRIMARY KEY,
        created_at INTEGER NOT NULL,
        duration_ms INTEGER NOT NULL,
        text TEXT NOT NULL,
        language TEXT,
        tags TEXT,
        title TEXT,
        summary TEXT,
        embedding TEXT,
        audio_path TEXT,
        source TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS clips (
        id TEXT PRIMARY KEY,
        created_at INTEGER NOT NULL,
        title TEXT NOT NULL,
        text TEXT NOT NULL,
        transcript_id TEXT
      );",
    )
    .map_err(|err| err.to_string())
    .and_then(|_| ensure_transcript_columns(conn))
}

fn ensure_transcript_columns(conn: &Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(transcripts)")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok(row.get::<_, String>(1)?))
        .map_err(|err| err.to_string())?;

    let mut columns = HashSet::new();
    for row in rows {
        if let Ok(name) = row {
            columns.insert(name);
        }
    }

    let add_column = |name: &str, decl: &str| -> Result<(), String> {
        if columns.contains(name) {
            return Ok(());
        }
        conn.execute(&format!("ALTER TABLE transcripts ADD COLUMN {decl}"), [])
            .map_err(|err| err.to_string())?;
        Ok(())
    };

    add_column("tags", "tags TEXT")?;
    add_column("title", "title TEXT")?;
    add_column("summary", "summary TEXT")?;
    add_column("embedding", "embedding TEXT")?;
    add_column("audio_path", "audio_path TEXT")?;

    Ok(())
}

fn settings_entries(settings: &Settings) -> Vec<(&'static str, Value)> {
    vec![
        (
            "audio.input_device_id",
            json!(settings.audio.input_device_id),
        ),
        ("audio.sample_rate_hz", json!(settings.audio.sample_rate_hz)),
        ("audio.channels", json!(settings.audio.channels)),
        ("audio.input_gain_db", json!(settings.audio.input_gain_db)),
        (
            "audio.noise_gate_enabled",
            json!(settings.audio.noise_gate_enabled),
        ),
        (
            "audio.noise_gate_threshold",
            json!(settings.audio.noise_gate_threshold),
        ),
        ("audio.vad_enabled", json!(settings.audio.vad_enabled)),
        ("audio.vad_threshold", json!(settings.audio.vad_threshold)),
        ("audio.vad_silence_ms", json!(settings.audio.vad_silence_ms)),
        ("audio.vad_resume_ms", json!(settings.audio.vad_resume_ms)),
        (
            "hotkey.record_toggle",
            json!(settings.hotkeys.record_toggle),
        ),
        ("hotkey.paste_last", json!(settings.hotkeys.paste_last)),
        ("hotkey.open_app", json!(settings.hotkeys.open_app)),
        ("transcription.model", json!(settings.transcription.model)),
        (
            "transcription.model_dir",
            json!(settings.transcription.model_dir),
        ),
        (
            "transcription.threads",
            json!(settings.transcription.threads),
        ),
        (
            "transcription.language",
            json!(settings.transcription.language),
        ),
        (
            "transcription.custom_vocab",
            json!(settings.transcription.custom_vocab),
        ),
        (
            "transcription.use_gpu",
            json!(settings.transcription.use_gpu),
        ),
        (
            "automation.auto_paste_enabled",
            json!(settings.automation.auto_paste_enabled),
        ),
        (
            "automation.paste_delay_ms",
            json!(settings.automation.paste_delay_ms),
        ),
        (
            "automation.copy_to_clipboard",
            json!(settings.automation.copy_to_clipboard),
        ),
        (
            "automation.preserve_clipboard",
            json!(settings.automation.preserve_clipboard),
        ),
        (
            "automation.clipboard_restore_delay_ms",
            json!(settings.automation.clipboard_restore_delay_ms),
        ),
        (
            "automation.paste_method",
            json!(settings.automation.paste_method),
        ),
        ("storage.data_dir", json!(settings.storage.data_dir)),
        ("storage.keep_audio", json!(settings.storage.keep_audio)),
        (
            "storage.retention_days",
            json!(settings.storage.retention_days),
        ),
        ("app.launch_on_login", json!(settings.app.launch_on_login)),
        ("app.start_in_tray", json!(settings.app.start_in_tray)),
        ("app.close_to_tray", json!(settings.app.close_to_tray)),
        ("ui.list_compact", json!(settings.ui.list_compact)),
        ("ui.onboarding_seen", json!(settings.ui.onboarding_seen)),
    ]
}

fn apply_setting(settings: &mut Settings, key: &str, value: Value) {
    match key {
        "audio.input_device_id" => assign(&mut settings.audio.input_device_id, value),
        "audio.sample_rate_hz" => assign(&mut settings.audio.sample_rate_hz, value),
        "audio.channels" => assign(&mut settings.audio.channels, value),
        "audio.input_gain_db" => assign(&mut settings.audio.input_gain_db, value),
        "audio.noise_gate_enabled" => assign(&mut settings.audio.noise_gate_enabled, value),
        "audio.noise_gate_threshold" => assign(&mut settings.audio.noise_gate_threshold, value),
        "audio.vad_enabled" => assign(&mut settings.audio.vad_enabled, value),
        "audio.vad_threshold" => assign(&mut settings.audio.vad_threshold, value),
        "audio.vad_silence_ms" => assign(&mut settings.audio.vad_silence_ms, value),
        "audio.vad_resume_ms" => assign(&mut settings.audio.vad_resume_ms, value),
        "hotkey.record_toggle" => assign(&mut settings.hotkeys.record_toggle, value),
        "hotkey.paste_last" => assign(&mut settings.hotkeys.paste_last, value),
        "hotkey.open_app" => assign(&mut settings.hotkeys.open_app, value),
        "transcription.model" => assign(&mut settings.transcription.model, value),
        "transcription.model_dir" => assign(&mut settings.transcription.model_dir, value),
        "transcription.threads" => assign(&mut settings.transcription.threads, value),
        "transcription.language" => assign(&mut settings.transcription.language, value),
        "transcription.custom_vocab" => assign(&mut settings.transcription.custom_vocab, value),
        "transcription.use_gpu" => assign(&mut settings.transcription.use_gpu, value),
        "automation.auto_paste_enabled" => {
            assign(&mut settings.automation.auto_paste_enabled, value)
        }
        "automation.paste_delay_ms" => assign(&mut settings.automation.paste_delay_ms, value),
        "automation.copy_to_clipboard" => assign(&mut settings.automation.copy_to_clipboard, value),
        "automation.preserve_clipboard" => {
            assign(&mut settings.automation.preserve_clipboard, value)
        }
        "automation.clipboard_restore_delay_ms" => {
            assign(&mut settings.automation.clipboard_restore_delay_ms, value)
        }
        "automation.paste_method" => assign(&mut settings.automation.paste_method, value),
        "storage.data_dir" => assign(&mut settings.storage.data_dir, value),
        "storage.keep_audio" => assign(&mut settings.storage.keep_audio, value),
        "storage.retention_days" => assign(&mut settings.storage.retention_days, value),
        "app.launch_on_login" => assign(&mut settings.app.launch_on_login, value),
        "app.start_in_tray" => assign(&mut settings.app.start_in_tray, value),
        "app.close_to_tray" => assign(&mut settings.app.close_to_tray, value),
        "ui.list_compact" => assign(&mut settings.ui.list_compact, value),
        "ui.onboarding_seen" => assign(&mut settings.ui.onboarding_seen, value),
        _ => {}
    }
}

fn assign<T: DeserializeOwned>(target: &mut T, value: Value) {
    if let Ok(parsed) = serde_json::from_value::<T>(value) {
        *target = parsed;
    }
}

fn load_settings_from_conn(
    conn: &Connection,
    fallback: &Settings,
) -> Result<Option<Settings>, String> {
    let mut stmt = conn
        .prepare("SELECT key, value FROM settings")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let key: String = row.get(0)?;
            let value: String = row.get(1)?;
            Ok((key, value))
        })
        .map_err(|err| err.to_string())?;

    let mut settings = fallback.clone();
    let mut found = false;

    for row in rows {
        let (key, value) = row.map_err(|err| err.to_string())?;
        found = true;
        if let Ok(parsed) = serde_json::from_str::<Value>(&value) {
            apply_setting(&mut settings, &key, parsed);
        }
    }

    if found {
        Ok(Some(settings))
    } else {
        Ok(None)
    }
}

fn save_settings_to_conn(conn: &mut Connection, settings: &Settings) -> Result<(), String> {
    let tx = conn.transaction().map_err(|err| err.to_string())?;
    tx.execute("DELETE FROM settings", [])
        .map_err(|err| err.to_string())?;

    {
        let mut stmt = tx
            .prepare("INSERT INTO settings (key, value) VALUES (?1, ?2)")
            .map_err(|err| err.to_string())?;
        for (key, value) in settings_entries(settings) {
            let encoded = serde_json::to_string(&value).map_err(|err| err.to_string())?;
            stmt.execute(params![key, encoded])
                .map_err(|err| err.to_string())?;
        }
    }

    tx.commit().map_err(|err| err.to_string())
}

fn load_legacy_settings(fallback: &Settings) -> Result<Settings, String> {
    let path = data_dir(fallback).join(LEGACY_SETTINGS_FILE);
    read_json(&path)
}

fn load_legacy_transcripts(settings: &Settings) -> Result<Vec<Transcript>, String> {
    let path = data_dir(settings).join(LEGACY_TRANSCRIPTS_FILE);
    read_json(&path)
}

fn maybe_migrate_legacy_transcripts(
    conn: &mut Connection,
    settings: &Settings,
) -> Result<(), String> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transcripts", [], |row| row.get(0))
        .optional()
        .map_err(|err| err.to_string())?
        .unwrap_or(0);

    if count > 0 {
        return Ok(());
    }

    let transcripts = match load_legacy_transcripts(settings) {
        Ok(transcripts) => transcripts,
        Err(_) => return Ok(()),
    };

    save_transcripts_to_conn(conn, settings, &transcripts)
}

fn save_transcripts_to_conn(
    conn: &mut Connection,
    settings: &Settings,
    transcripts: &[Transcript],
) -> Result<(), String> {
    let tx = conn.transaction().map_err(|err| err.to_string())?;
    tx.execute("DELETE FROM transcripts", [])
        .map_err(|err| err.to_string())?;

    {
        let mut stmt = tx
      .prepare(
        "INSERT INTO transcripts
          (id, created_at, duration_ms, text, language, tags, title, summary, embedding, audio_path, source)
          VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
      )
      .map_err(|err| err.to_string())?;

        let language = if settings.transcription.language.is_empty() {
            None
        } else {
            Some(settings.transcription.language.as_str())
        };

        for transcript in transcripts {
            let tags = if transcript.tags.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&transcript.tags).map_err(|err| err.to_string())?)
            };
            let embedding = transcript
                .embedding
                .as_ref()
                .map(|vector| serde_json::to_string(vector))
                .transpose()
                .map_err(|err| err.to_string())?;

            stmt.execute(params![
                transcript.id,
                transcript.created_at,
                transcript.duration_ms as i64,
                transcript.text,
                language,
                tags,
                transcript.title,
                transcript.summary,
                embedding,
                transcript.audio_path,
                TRANSCRIPT_SOURCE,
            ])
            .map_err(|err| err.to_string())?;
        }
    }

    tx.commit().map_err(|err| err.to_string())
}

pub fn load_settings() -> Settings {
    let fallback = Settings::default();
    let path = db_path(&fallback);
    let mut conn = match open_connection(&path) {
        Ok(conn) => conn,
        Err(_) => return fallback,
    };

    if ensure_schema(&conn).is_err() {
        return fallback;
    }

    match load_settings_from_conn(&conn, &fallback) {
        Ok(Some(settings)) => settings,
        Ok(None) => {
            if let Ok(settings) = load_legacy_settings(&fallback) {
                let _ = save_settings_to_conn(&mut conn, &settings);
                let _ = maybe_migrate_legacy_transcripts(&mut conn, &settings);
                settings
            } else {
                fallback
            }
        }
        Err(_) => fallback,
    }
}

pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let path = db_path(settings);
    let mut conn = open_connection(&path)?;
    ensure_schema(&conn)?;
    save_settings_to_conn(&mut conn, settings)
}

pub fn load_transcripts(settings: &Settings) -> Vec<Transcript> {
    let path = db_path(settings);
    let conn = match open_connection(&path) {
        Ok(conn) => conn,
        Err(_) => return Vec::new(),
    };

    if ensure_schema(&conn).is_err() {
        return Vec::new();
    }

    let mut stmt = match conn.prepare(
        "SELECT id, created_at, duration_ms, text, title, summary, tags, embedding, audio_path
     FROM transcripts
     ORDER BY created_at DESC",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };

    let rows = match stmt.query_map([], |row| {
        Ok(Transcript {
            id: row.get(0)?,
            created_at: row.get(1)?,
            duration_ms: row.get::<_, i64>(2)?.try_into().unwrap_or_default(),
            text: row.get(3)?,
            title: row.get(4)?,
            summary: row.get(5)?,
            tags: row
                .get::<_, Option<String>>(6)?
                .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
                .unwrap_or_default(),
            embedding: row
                .get::<_, Option<String>>(7)?
                .and_then(|raw| serde_json::from_str::<Vec<f32>>(&raw).ok()),
            audio_path: row.get::<_, Option<String>>(8)?,
        })
    }) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };

    let mut transcripts = Vec::new();
    for row in rows {
        if let Ok(transcript) = row {
            transcripts.push(transcript);
        }
    }

    transcripts
}

pub fn load_clips(settings: &Settings) -> Vec<Clip> {
    let path = db_path(settings);
    let conn = match open_connection(&path) {
        Ok(conn) => conn,
        Err(_) => return Vec::new(),
    };

    if ensure_schema(&conn).is_err() {
        return Vec::new();
    }

    let mut stmt = match conn.prepare(
        "SELECT id, created_at, title, text, transcript_id
     FROM clips
     ORDER BY created_at DESC",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };

    let rows = match stmt.query_map([], |row| {
        Ok(Clip {
            id: row.get(0)?,
            created_at: row.get(1)?,
            title: row.get(2)?,
            text: row.get(3)?,
            transcript_id: row.get(4)?,
        })
    }) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };

    let mut clips = Vec::new();
    for row in rows {
        if let Ok(clip) = row {
            clips.push(clip);
        }
    }

    clips
}

pub fn load_transcripts_with_retention(settings: &Settings) -> Vec<Transcript> {
    let transcripts = load_transcripts(settings);
    let retention_days = settings.storage.retention_days;
    if retention_days == 0 || transcripts.is_empty() {
        return transcripts;
    }

    let original_len = transcripts.len();
    let filtered = filter_transcripts_by_retention(transcripts.clone(), retention_days);
    if filtered.len() != original_len {
        let kept_ids = filtered
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>();
        let removed_paths = transcripts
            .iter()
            .filter(|item| !kept_ids.contains(item.id.as_str()))
            .filter_map(|item| item.audio_path.clone())
            .collect::<Vec<_>>();
        let _ = save_transcripts(settings, &filtered);
        for path in removed_paths {
            let _ = delete_audio_file(settings, &path);
        }
    }
    filtered
}

fn filter_transcripts_by_retention(
    transcripts: Vec<Transcript>,
    retention_days: u32,
) -> Vec<Transcript> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    let now_day = now_ms / MILLIS_PER_DAY;
    let retention_days = retention_days as i64;

    transcripts
        .into_iter()
        .filter(|transcript| {
            let created_day = transcript.created_at / MILLIS_PER_DAY;
            now_day.saturating_sub(created_day) < retention_days
        })
        .collect()
}

pub fn save_transcripts(settings: &Settings, transcripts: &[Transcript]) -> Result<(), String> {
    let path = db_path(settings);
    let mut conn = open_connection(&path)?;
    ensure_schema(&conn)?;
    save_transcripts_to_conn(&mut conn, settings, transcripts)
}

pub fn upsert_transcript(settings: &Settings, transcript: &Transcript) -> Result<(), String> {
    let path = db_path(settings);
    let conn = open_connection(&path)?;
    ensure_schema(&conn)?;

    let language = if settings.transcription.language.is_empty() {
        None
    } else {
        Some(settings.transcription.language.as_str())
    };

    let tags = encode_tags(&transcript.tags)?;
    let embedding = encode_embedding(&transcript.embedding)?;

    conn
    .execute(
      "INSERT INTO transcripts
        (id, created_at, duration_ms, text, language, tags, title, summary, embedding, audio_path, source)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ON CONFLICT(id) DO UPDATE SET
          created_at = excluded.created_at,
          duration_ms = excluded.duration_ms,
          text = excluded.text,
          language = excluded.language,
          tags = excluded.tags,
          title = excluded.title,
          summary = excluded.summary,
          embedding = excluded.embedding,
          audio_path = excluded.audio_path,
          source = excluded.source",
      params![
        transcript.id,
        transcript.created_at,
        transcript.duration_ms as i64,
        transcript.text,
        language,
        tags,
        transcript.title,
        transcript.summary,
        embedding,
        transcript.audio_path,
        TRANSCRIPT_SOURCE,
      ],
    )
    .map_err(|err| err.to_string())?;

    Ok(())
}

pub fn delete_transcript_row(settings: &Settings, id: &str) -> Result<(), String> {
    let path = db_path(settings);
    let conn = open_connection(&path)?;
    ensure_schema(&conn)?;
    conn.execute("DELETE FROM transcripts WHERE id = ?1", params![id])
        .map_err(|err| err.to_string())?;
    Ok(())
}

pub fn clear_transcripts_table(settings: &Settings) -> Result<(), String> {
    let path = db_path(settings);
    let conn = open_connection(&path)?;
    ensure_schema(&conn)?;
    conn.execute("DELETE FROM transcripts", [])
        .map_err(|err| err.to_string())?;
    Ok(())
}

pub fn insert_clip(settings: &Settings, clip: &Clip) -> Result<(), String> {
    let path = db_path(settings);
    let conn = open_connection(&path)?;
    ensure_schema(&conn)?;

    conn.execute(
        "INSERT INTO clips (id, created_at, title, text, transcript_id)
       VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            clip.id,
            clip.created_at,
            clip.title,
            clip.text,
            clip.transcript_id,
        ],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

pub fn delete_clip(settings: &Settings, id: &str) -> Result<(), String> {
    let path = db_path(settings);
    let conn = open_connection(&path)?;
    ensure_schema(&conn)?;
    conn.execute("DELETE FROM clips WHERE id = ?1", params![id])
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn expand_tilde_handles_windows_separator() {
        let original = std::env::var_os("HOME");
        std::env::set_var("HOME", "/tmp/whispr-test");

        let expanded = expand_tilde("~\\data");
        assert_eq!(expanded, PathBuf::from("/tmp/whispr-test").join("data"));

        if let Some(value) = original {
            std::env::set_var("HOME", value);
        } else {
            std::env::remove_var("HOME");
        }
    }

    #[test]
    fn upsert_and_delete_transcript_roundtrip() {
        let mut settings = Settings::default();
        let dir = std::env::temp_dir().join(format!("whispr-test-{}", Uuid::new_v4()));
        settings.storage.data_dir = dir.to_string_lossy().to_string();

        let transcript = Transcript {
            id: Uuid::new_v4().to_string(),
            created_at: 123,
            duration_ms: 456,
            text: "hello world".to_string(),
            title: Some("hello".to_string()),
            summary: None,
            tags: vec!["a".to_string(), "b".to_string()],
            audio_path: None,
            embedding: Some(vec![0.1, 0.2, 0.3]),
        };

        upsert_transcript(&settings, &transcript).expect("upsert");
        let loaded = load_transcripts(&settings);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, transcript.id);
        assert_eq!(loaded[0].text, transcript.text);
        assert_eq!(loaded[0].tags, transcript.tags);
        assert!(loaded[0].embedding.is_some());

        delete_transcript_row(&settings, &transcript.id).expect("delete");
        let loaded = load_transcripts(&settings);
        assert!(loaded.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
