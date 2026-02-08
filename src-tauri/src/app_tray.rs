use std::sync::Mutex;

use crate::cli;
#[cfg(target_os = "windows")]
use crate::core::automation;
use crate::state::AppState;

#[cfg(target_os = "windows")]
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
#[cfg(target_os = "windows")]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
#[cfg(target_os = "windows")]
use tauri::Emitter;
#[cfg(target_os = "windows")]
use tauri::Manager;
use tauri::{AppHandle, Window, WindowEvent};

#[cfg(target_os = "windows")]
const TRAY_ID: &str = "main";

#[cfg(target_os = "windows")]
const MENU_TOGGLE_ID: &str = "tray_toggle";
#[cfg(target_os = "windows")]
const MENU_PASTE_ID: &str = "tray_paste_last";
#[cfg(target_os = "windows")]
const MENU_OPEN_ID: &str = "tray_open";
#[cfg(target_os = "windows")]
const MENU_SETTINGS_ID: &str = "tray_settings";
#[cfg(target_os = "windows")]
const MENU_QUIT_ID: &str = "tray_quit";
#[cfg(target_os = "windows")]
const MENU_RECENTS_ID: &str = "tray_recents";
#[cfg(target_os = "windows")]
const MENU_RECENT_PREFIX: &str = "tray_recent:";
#[cfg(target_os = "windows")]
const MENU_CLOSE_TO_TRAY_ID: &str = "tray_close_to_tray";
#[cfg(target_os = "windows")]
const MAX_RECENTS: usize = 8;
#[cfg(target_os = "windows")]
const PREVIEW_LEN: usize = 40;

#[cfg(target_os = "windows")]
pub fn setup_tray(app: &AppHandle, state: &Mutex<AppState>) -> Result<(), String> {
    let menu = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        build_menu(app, &guard)?
    };
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))
        .map_err(|err| err.to_string())?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| handle_menu_event(app, event))
        .on_tray_icon_event(|tray, event| handle_tray_event(tray, event))
        .build(app)
        .map_err(|err| err.to_string())?;

    refresh_tray(app, state);
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn setup_tray(_app: &AppHandle, _state: &Mutex<AppState>) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn refresh_tray(app: &AppHandle, state: &Mutex<AppState>) {
    let guard = match state.lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };

    let menu = match build_menu(app, &guard) {
        Ok(menu) => menu,
        Err(_) => return,
    };

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_menu(Some(menu));
        let tooltip = if guard.recording {
            Some("Whispr — Recording")
        } else {
            Some("Whispr")
        };
        let _ = tray.set_tooltip(tooltip);
    }
}

#[cfg(not(target_os = "windows"))]
pub fn refresh_tray(_app: &AppHandle, _state: &Mutex<AppState>) {}

#[cfg(target_os = "windows")]
pub fn handle_window_event(window: &Window, event: &WindowEvent) {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        let close_to_tray = {
            let app = window.app_handle();
            let state = app.state::<Mutex<AppState>>();
            let mutex = state.inner();
            mutex
                .lock()
                .ok()
                .map(|g| g.settings.app.close_to_tray)
                .unwrap_or(false)
        };
        if close_to_tray {
            let _ = window.hide();
            api.prevent_close();
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn handle_window_event(_window: &Window, _event: &WindowEvent) {}

#[cfg(target_os = "windows")]
pub fn maybe_hide_on_start(
    app: &AppHandle,
    state: &Mutex<AppState>,
    action: Option<cli::CliAction>,
) {
    let hide = {
        let guard = match state.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        guard.settings.app.start_in_tray
    };

    if !hide {
        return;
    }

    if matches!(
        action,
        Some(cli::CliAction::Show | cli::CliAction::ShowSettings)
    ) {
        return;
    }

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

#[cfg(not(target_os = "windows"))]
pub fn maybe_hide_on_start(
    _app: &AppHandle,
    _state: &Mutex<AppState>,
    _action: Option<cli::CliAction>,
) {
}

#[cfg(target_os = "windows")]
fn handle_tray_event(tray: &tauri::tray::TrayIcon, event: TrayIconEvent) {
    let app = tray.app_handle();
    match event {
        TrayIconEvent::Click {
            button,
            button_state,
            ..
        } => {
            if button == MouseButton::Left && button_state == MouseButtonState::Up {
                cli::handle_action(app, cli::CliAction::Toggle);
                let state = app.state::<Mutex<AppState>>();
                refresh_tray(app, state.inner());
            }
        }
        TrayIconEvent::DoubleClick { button, .. } => {
            if button == MouseButton::Left {
                cli::handle_action(app, cli::CliAction::Show);
            }
        }
        _ => {}
    }
}

#[cfg(target_os = "windows")]
fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let id = event.id().as_ref();
    match id {
        MENU_TOGGLE_ID => cli::handle_action(app, cli::CliAction::Toggle),
        MENU_PASTE_ID => cli::handle_action(app, cli::CliAction::PasteLast),
        MENU_OPEN_ID => cli::handle_action(app, cli::CliAction::Show),
        MENU_SETTINGS_ID => cli::handle_action(app, cli::CliAction::ShowSettings),
        MENU_QUIT_ID => cli::handle_action(app, cli::CliAction::Quit),
        MENU_CLOSE_TO_TRAY_ID => {
            let state = app.state::<Mutex<AppState>>();
            let mutex = state.inner();
            let updated = {
                let mut guard = match mutex.lock() {
                    Ok(guard) => guard,
                    Err(_) => return,
                };
                guard.settings.app.close_to_tray = !guard.settings.app.close_to_tray;
                guard.settings.clone()
            };
            let _ = crate::core::storage::save_settings(&updated);
            let _ = app.emit("settings-updated", updated);
        }
        id if id.starts_with(MENU_RECENT_PREFIX) => {
            let Some(transcript_id) = id.strip_prefix(MENU_RECENT_PREFIX) else {
                return;
            };
            let state = app.state::<Mutex<AppState>>();
            let mutex = state.inner();
            if let Ok(guard) = mutex.lock() {
                if let Some(transcript) = guard
                    .transcripts
                    .iter()
                    .find(|item| item.id == transcript_id)
                {
                    let _ = automation::copy_text(&transcript.text);
                }
            }
        }
        _ => {}
    }

    let state = app.state::<Mutex<AppState>>();
    refresh_tray(app, state.inner());
}

#[cfg(target_os = "windows")]
fn build_menu(app: &AppHandle, state: &AppState) -> Result<Menu<tauri::Wry>, String> {
    let menu = Menu::new(app).map_err(|err| err.to_string())?;
    let record_accel = normalize_accelerator(&state.settings.hotkeys.record_toggle);
    let paste_accel = normalize_accelerator(&state.settings.hotkeys.paste_last);
    let open_accel = normalize_accelerator(&state.settings.hotkeys.open_app);

    let toggle_text = if state.recording {
        "Stop Recording"
    } else {
        "Start Recording"
    };
    let toggle_item = MenuItem::with_id(
        app,
        MENU_TOGGLE_ID,
        toggle_text,
        true,
        record_accel.as_deref(),
    )
    .map_err(|err| err.to_string())?;

    let paste_item = MenuItem::with_id(
        app,
        MENU_PASTE_ID,
        "Paste Last Transcript",
        !state.transcripts.is_empty(),
        paste_accel.as_deref(),
    )
    .map_err(|err| err.to_string())?;

    let recents = build_recents_submenu(app, state).map_err(|err| err.to_string())?;

    menu.append(&toggle_item).map_err(|err| err.to_string())?;
    menu.append(&paste_item).map_err(|err| err.to_string())?;
    menu.append(&recents).map_err(|err| err.to_string())?;
    menu.append(&PredefinedMenuItem::separator(app).map_err(|err| err.to_string())?)
        .map_err(|err| err.to_string())?;

    let settings_item = MenuItem::with_id(app, MENU_SETTINGS_ID, "Settings", true, None::<&str>)
        .map_err(|err| err.to_string())?;
    let open_item = MenuItem::with_id(app, MENU_OPEN_ID, "Open App", true, open_accel.as_deref())
        .map_err(|err| err.to_string())?;
    let close_to_tray_item = tauri::menu::CheckMenuItem::with_id(
        app,
        MENU_CLOSE_TO_TRAY_ID,
        "Close to Tray",
        true,
        state.settings.app.close_to_tray,
        None::<&str>,
    )
    .map_err(|err| err.to_string())?;

    menu.append(&settings_item).map_err(|err| err.to_string())?;
    menu.append(&open_item).map_err(|err| err.to_string())?;
    menu.append(&close_to_tray_item)
        .map_err(|err| err.to_string())?;

    menu.append(&PredefinedMenuItem::separator(app).map_err(|err| err.to_string())?)
        .map_err(|err| err.to_string())?;

    let quit_item = MenuItem::with_id(app, MENU_QUIT_ID, "Quit", true, None::<&str>)
        .map_err(|err| err.to_string())?;
    menu.append(&quit_item).map_err(|err| err.to_string())?;

    Ok(menu)
}

#[cfg(target_os = "windows")]
fn normalize_accelerator(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut normalized = trimmed.to_string();
    for (from, to) in [
        ("CommandOrControl", "Ctrl"),
        ("CmdOrCtrl", "Ctrl"),
        ("Cmd", "Ctrl"),
    ] {
        normalized = normalized.replace(from, to);
    }

    Some(normalized)
}

#[cfg(target_os = "windows")]
fn build_recents_submenu(
    app: &AppHandle,
    state: &AppState,
) -> Result<Submenu<tauri::Wry>, tauri::Error> {
    let submenu = Submenu::with_id(app, MENU_RECENTS_ID, "Recent Transcriptions", true)?;
    let transcripts = state.transcripts.iter().take(MAX_RECENTS);
    let mut added = false;

    for transcript in transcripts {
        let preview = preview_text(&transcript.text);
        let item = MenuItem::with_id(
            app,
            format!("{MENU_RECENT_PREFIX}{}", transcript.id),
            preview,
            true,
            None::<&str>,
        )?;
        submenu.append(&item)?;
        added = true;
    }

    if !added {
        let empty = MenuItem::new(app, "No transcripts yet", false, None::<&str>)?;
        submenu.append(&empty)?;
    }

    Ok(submenu)
}

#[cfg(target_os = "windows")]
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
