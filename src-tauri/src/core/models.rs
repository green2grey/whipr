use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use crate::core::storage::expand_tilde;
use crate::settings::Settings;
use crate::types::ModelInfo;

const MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

struct ModelDefinition {
    id: &'static str,
    label: &'static str,
    filename: &'static str,
}

const MODELS: [ModelDefinition; 3] = [
    ModelDefinition {
        id: "tiny.en",
        label: "Tiny (fast)",
        filename: "ggml-tiny.en.bin",
    },
    ModelDefinition {
        id: "small.en",
        label: "Small (balanced)",
        filename: "ggml-small.en.bin",
    },
    ModelDefinition {
        id: "medium.en",
        label: "Medium (accurate)",
        filename: "ggml-medium.en.bin",
    },
];

fn model_dir(settings: &Settings) -> PathBuf {
    expand_tilde(&settings.transcription.model_dir)
}

fn model_path(settings: &Settings, model: &ModelDefinition) -> PathBuf {
    model_dir(settings).join(model.filename)
}

fn find_model(model_id: &str) -> Result<&'static ModelDefinition, String> {
    MODELS
        .iter()
        .find(|model| model.id == model_id)
        .ok_or_else(|| format!("Unknown model id: {model_id}"))
}

pub fn list_models(settings: &Settings) -> Vec<ModelInfo> {
    MODELS
        .iter()
        .map(|model| {
            let installed = model_path(settings, model).exists();
            ModelInfo {
                id: model.id.to_string(),
                label: model.label.to_string(),
                installed,
                active: settings.transcription.model == model.id,
            }
        })
        .collect()
}

pub fn resolve_model_path(settings: &Settings, model_id: &str) -> Result<PathBuf, String> {
    let model = find_model(model_id)?;
    let path = model_path(settings, model);

    if !path.exists() {
        return Err(format!("Model not installed: {model_id}"));
    }

    Ok(path)
}

pub fn download_model_with_progress<F: FnMut(u64, u64)>(
    settings: &Settings,
    model_id: &str,
    mut on_progress: F,
) -> Result<(), String> {
    let model = find_model(model_id)?;
    let path = model_path(settings, model);

    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let url = format!("{MODEL_BASE_URL}/{}", model.filename);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30 * 60))
        .build()
        .map_err(|err| err.to_string())?;
    let mut response = client.get(url).send().map_err(|err| err.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Download failed with status {}", response.status()));
    }

    let total = response.content_length().unwrap_or(0);
    let tmp_path = path.with_extension("download");
    let mut file = fs::File::create(&tmp_path).map_err(|err| err.to_string())?;

    let mut downloaded: u64 = 0;
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = response.read(&mut buffer).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|err| err.to_string())?;
        downloaded = downloaded.saturating_add(read as u64);
        on_progress(downloaded, total);
    }

    file.flush().map_err(|err| err.to_string())?;
    fs::rename(&tmp_path, &path).map_err(|err| err.to_string())?;
    Ok(())
}

pub fn delete_model(settings: &Settings, model_id: &str) -> Result<(), String> {
    let model = find_model(model_id)?;
    let path = model_path(settings, model);

    if path.exists() {
        fs::remove_file(&path).map_err(|err| err.to_string())?;
    }

    Ok(())
}

pub fn activate_model(settings: &mut Settings, model_id: &str) -> Result<(), String> {
    let model = find_model(model_id)?;
    if !model_path(settings, model).exists() {
        return Err("Model not installed".to_string());
    }

    settings.transcription.model = model.id.to_string();
    Ok(())
}

pub fn cycle_model(settings: &mut Settings) -> Result<String, String> {
    let models = list_models(settings);
    let installed: Vec<ModelInfo> = models.into_iter().filter(|m| m.installed).collect();

    if installed.is_empty() {
        return Err("No installed models to cycle".to_string());
    }

    let current_index = installed
        .iter()
        .position(|model| model.id == settings.transcription.model)
        .unwrap_or(0);

    let next_index = (current_index + 1) % installed.len();
    let next_id = installed[next_index].id.clone();
    settings.transcription.model = next_id.clone();
    Ok(next_id)
}
