import { invoke } from '@tauri-apps/api/core';

export type Settings = {
  audio: {
    input_device_id: string;
    sample_rate_hz: number;
    channels: number;
    input_gain_db: number;
    noise_gate_enabled: boolean;
    noise_gate_threshold: number;
    vad_enabled: boolean;
    vad_threshold: number;
    vad_silence_ms: number;
    vad_resume_ms: number;
  };
  hotkeys: {
    record_toggle: string;
    paste_last: string;
    open_app: string;
  };
  transcription: {
    model: string;
    model_dir: string;
    threads: number;
    language: string;
    custom_vocab: string;
    use_gpu: boolean;
  };
  automation: {
    auto_paste_enabled: boolean;
    paste_delay_ms: number;
    copy_to_clipboard: boolean;
    preserve_clipboard: boolean;
    paste_method: string;
  };
  storage: {
    data_dir: string;
    keep_audio: boolean;
    retention_days: number;
  };
  app: {
    launch_on_login: boolean;
    start_in_tray: boolean;
    close_to_tray: boolean;
  };
  ui: {
    list_compact: boolean;
    onboarding_seen: boolean;
    live_preview_enabled: boolean;
    recording_hud_enabled: boolean;
  };
};

export type Transcript = {
  id: string;
  created_at: number;
  duration_ms: number;
  text: string;
  title: string | null;
  summary: string | null;
  tags: string[];
  audio_path?: string | null;
};

export type TranscriptUpdate = {
  text?: string;
  title?: string | null;
  summary?: string | null;
  tags?: string[];
};

export type Clip = {
  id: string;
  created_at: number;
  title: string;
  text: string;
  transcript_id?: string | null;
};

export type ModelInfo = {
  id: string;
  label: string;
  installed: boolean;
  active: boolean;
};

export type ToggleResult = {
  recording: boolean;
  transcript: Transcript | null;
};

export type RuntimeInfo = {
  session_type: string;
  hotkeys_supported: boolean;
  paste_method: string;
  missing_helpers: string[];
};

export type MacosPermissions = {
  accessibility: boolean;
  input_monitoring: boolean;
};

export type StorageStats = {
  data_bytes: number;
  model_bytes: number;
  db_bytes: number;
  transcript_count: number;
};

export type PerformanceInfo = {
  gpu_supported: boolean;
  gpu_enabled: boolean;
  thread_count: number;
  gpu_error?: string | null;
  gpu_name?: string | null;
};

export type BenchmarkResult = {
  audio_seconds: number;
  duration_ms: number;
  realtime_factor: number;
  text_length: number;
};

export type ImportFailure = {
  path: string;
  error: string;
};

export type ImportResult = {
  transcripts: Transcript[];
  failures: ImportFailure[];
};

export type UpdateInfo = {
  current_version: string;
  latest_version: string;
  url: string;
};

export type AudioDevice = {
  id: string;
  name: string;
  is_default: boolean;
};

export const getDefaultSettings = () => invoke<Settings>('get_default_settings');
export const getSettings = () => invoke<Settings>('get_settings');
export const setUiActive = (active: boolean) => invoke<boolean>('set_ui_active', { active });
export const saveSettings = (settings: Settings) => invoke<Settings>('save_settings', { settings });
export const setAudioInputDevice = (inputDeviceId: string) =>
  // Tauri v2 maps Rust snake_case params (e.g. `input_device_id`) to camelCase JS keys.
  invoke<Settings>('set_audio_input_device', { inputDeviceId });

export const listTranscripts = () => invoke<Transcript[]>('list_transcripts');
export const searchTranscripts = (query: string, limit?: number) =>
  invoke<Transcript[]>('search_transcripts', { query, limit });
export const updateTranscript = (id: string, update: TranscriptUpdate) =>
  invoke<Transcript>('update_transcript', { id, update });
export const deleteTranscript = (id: string) => invoke<boolean>('delete_transcript', { id });
export const exportTranscript = (path: string, text: string) =>
  invoke<boolean>('export_transcript', { path, text });
export const clearTranscripts = () => invoke<boolean>('clear_transcripts');
export const importAudioFiles = (paths: string[]) =>
  invoke<ImportResult>('import_audio_files', { paths });
export const getRuntimeInfo = () => invoke<RuntimeInfo>('get_runtime_info');
export const getMacosPermissions = () => invoke<MacosPermissions>('get_macos_permissions');
export const requestMacosAccessibilityPermission = () =>
  invoke<MacosPermissions>('request_macos_accessibility_permission');
export const requestMacosInputMonitoringPermission = () =>
  invoke<MacosPermissions>('request_macos_input_monitoring_permission');
export const openMacosPermissionSettings = (permission: 'accessibility' | 'input_monitoring') =>
  invoke<boolean>('open_macos_permission_settings', { permission });
export const getPerformanceInfo = () => invoke<PerformanceInfo>('get_performance_info');
export const benchmarkTranscription = (path: string) =>
  invoke<BenchmarkResult>('benchmark_transcription', { path });
export const listAudioDevices = () => invoke<AudioDevice[]>('list_audio_devices');
export const toggleRecording = () => invoke<ToggleResult>('toggle_recording');
export const getRecordingLevel = () => invoke<number | null>('get_recording_level');
export const getRecordingState = () =>
  invoke<{ recording: boolean; started_at_ms: number | null; hud_enabled: boolean }>('get_recording_state');
export const pasteLastTranscript = () => invoke<boolean>('paste_last_transcript');
export const copyText = (text: string) => invoke<boolean>('copy_text', { text });
export const checkForUpdates = () => invoke<UpdateInfo | null>('check_for_updates');
export const getStorageStats = () => invoke<StorageStats>('get_storage_stats');
export const listClips = () => invoke<Clip[]>('list_clips');
export const createClip = (title: string, text: string, transcriptId?: string | null) =>
  invoke<Clip>('create_clip', { title, text, transcriptId });
export const deleteClip = (id: string) => invoke<boolean>('delete_clip', { id });

export const listModels = () => invoke<ModelInfo[]>('list_models');
export const downloadModel = (modelId: string) =>
  invoke<ModelInfo[]>('download_model', { modelId });
export const deleteModel = (modelId: string) =>
  invoke<ModelInfo[]>('delete_model', { modelId });
export const activateModel = (modelId: string) =>
  invoke<ModelInfo[]>('activate_model', { modelId });
export const cycleModel = () => invoke<ModelInfo[]>('cycle_model');
