use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

use crate::commands;
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliAction {
    Toggle,
    PasteLast,
    Show,
    ShowSettings,
    Quit,
}

pub fn parse_cli_action(args: &[String]) -> Option<CliAction> {
    args.iter().find_map(|arg| action_from_arg(arg))
}

pub fn handle_action(app: &AppHandle, action: CliAction) {
    match action {
        CliAction::Toggle => {
            let state = app.state::<Mutex<AppState>>();
            let _ = commands::toggle_recording_with_state_and_emit(app, state.inner());
        }
        CliAction::PasteLast => {
            let state = app.state::<Mutex<AppState>>();
            let _ = commands::paste_last_transcript_with_state(state.inner());
        }
        CliAction::Show => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        CliAction::ShowSettings => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            let _ = app.emit("open-settings", true);
        }
        CliAction::Quit => {
            app.exit(0);
        }
    }
}

fn action_from_arg(arg: &str) -> Option<CliAction> {
    match arg {
        "--toggle" | "toggle" => Some(CliAction::Toggle),
        "--paste-last" | "paste-last" | "--paste_last" | "paste_last" => Some(CliAction::PasteLast),
        "--show" | "show" | "--open" | "open" | "--focus" | "focus" => Some(CliAction::Show),
        "--show-settings" | "show-settings" | "--settings" | "settings" => {
            Some(CliAction::ShowSettings)
        }
        "--quit" | "quit" | "--exit" | "exit" => Some(CliAction::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_action_toggle() {
        let args = vec!["whispr".to_string(), "--toggle".to_string()];
        assert_eq!(parse_cli_action(&args), Some(CliAction::Toggle));
    }

    #[test]
    fn parse_cli_action_paste_last() {
        let args = vec!["whispr".to_string(), "--paste-last".to_string()];
        assert_eq!(parse_cli_action(&args), Some(CliAction::PasteLast));
    }

    #[test]
    fn parse_cli_action_show() {
        let args = vec!["whispr".to_string(), "--show".to_string()];
        assert_eq!(parse_cli_action(&args), Some(CliAction::Show));
    }
}
