use tauri::{App, Manager};

const HUD_LABEL: &str = "recording_hud";

/// Create the recording HUD window at startup so it can receive events even when hidden.
pub fn ensure_recording_hud(app: &App) -> Result<(), String> {
    if app.get_webview_window(HUD_LABEL).is_some() {
        return Ok(());
    }

    // Load the same frontend bundle, but mount a dedicated HUD UI via the query flag.
    let url = tauri::WebviewUrl::App("index.html?hud=1".into());

    tauri::WebviewWindowBuilder::new(app, HUD_LABEL, url)
        .title("Whipr")
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .closable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .visible(false)
        // Initial size/position are refined by the HUD window itself using `screen.avail*`.
        .inner_size(412.0, 64.0)
        .build()
        .map(|_| ())
        .map_err(|err| err.to_string())
}
