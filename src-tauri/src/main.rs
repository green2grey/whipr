mod app_tray;
mod cli;
mod commands;
mod core;
mod hud;
mod overlay;
mod settings;
mod state;
mod tray;
mod types;

use std::sync::Mutex;

use tauri::Manager;

fn main() {
    // Disable whisper.cpp / ggml stderr spam by default.
    // Set `WHISPR_WHISPER_LOGS=1` to keep upstream logs (useful for debugging).
    if std::env::var_os("WHISPR_WHISPER_LOGS").is_none() {
        whisper_rs::install_logging_hooks();
    }

    let initial_action = cli::parse_cli_action(&std::env::args().collect::<Vec<_>>());

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(action) = cli::parse_cli_action(&argv) {
                cli::handle_action(app, action);
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(Mutex::new(state::AppState::load()))
        .setup(move |app| {
            let _ = overlay::write_state(false, None, Some(0.0));
            let _ = hud::ensure_recording_hud(app);
            if let Ok(guard) = app.state::<Mutex<state::AppState>>().lock() {
                let last_transcript_at_ms = guard.transcripts.first().map(|item| item.created_at);
                let _ =
                    tray::write_recents(&guard.settings, &guard.transcripts, last_transcript_at_ms);
            }
            let handle = app.handle();
            let state = app.state::<Mutex<state::AppState>>();
            let _ = app_tray::setup_tray(handle, state.inner());
            if let Some(action) = initial_action {
                cli::handle_action(handle, action);
                app_tray::maybe_hide_on_start(handle, state.inner(), Some(action));
            } else {
                app_tray::maybe_hide_on_start(handle, state.inner(), None);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // HUD is a utility window; hide instead of destroying it so it can reappear on the
            // next recording event.
            if window.label() == "recording_hud" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    let _ = window.hide();
                    api.prevent_close();
                }
                return;
            }
            app_tray::handle_window_event(window, event);
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::get_default_settings,
            commands::get_settings,
            commands::set_ui_active,
            commands::set_audio_input_device,
            commands::save_settings,
            commands::list_transcripts,
            commands::search_transcripts,
            commands::import_audio_files,
            commands::update_transcript,
            commands::delete_transcript,
            commands::clear_transcripts,
            commands::list_clips,
            commands::create_clip,
            commands::delete_clip,
            commands::get_runtime_info,
            commands::get_macos_permissions,
            commands::request_macos_accessibility_permission,
            commands::request_macos_input_monitoring_permission,
            commands::open_macos_permission_settings,
            commands::get_performance_info,
            commands::benchmark_transcription,
            commands::copy_text,
            commands::export_transcript,
            commands::check_for_updates,
            commands::get_storage_stats,
            commands::list_audio_devices,
            commands::toggle_recording,
            commands::get_recording_level,
            commands::get_recording_state,
            commands::paste_last_transcript,
            commands::list_models,
            commands::download_model,
            commands::delete_model,
            commands::activate_model,
            commands::cycle_model,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
