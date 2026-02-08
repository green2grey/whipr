use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use arboard::Clipboard;
use enigo::{Enigo, Key, KeyboardControllable};

use crate::core::runtime::{self, HelperAvailability, PasteMethod, SessionType};

/// Default delay before restoring the previous clipboard contents after a clipboard-based paste.
///
/// Why 90ms: most applications consume the clipboard synchronously when handling the paste
/// shortcut (Ctrl+V / Cmd+V) or within a couple of event-loop ticks. Empirically, ~90ms avoids
/// restoring too early on typical systems while still feeling instantaneous.
///
/// When to increase: if you see intermittent paste failures (wrong/empty contents), especially on
/// slower machines, under high CPU load, or via remote desktop / VMs, increase the delay (e.g.
/// 150-300ms or more).
const DEFAULT_CLIPBOARD_RESTORE_DELAY_MS: u64 = 90;

// Avoid letting a bad config value introduce multi-second UI stalls.
const MAX_CLIPBOARD_RESTORE_DELAY_MS: u64 = 2_000;

fn validated_clipboard_restore_delay_ms(ms: u64) -> u64 {
    if ms == 0 {
        return DEFAULT_CLIPBOARD_RESTORE_DELAY_MS;
    }
    ms.min(MAX_CLIPBOARD_RESTORE_DELAY_MS)
}

#[derive(Debug)]
enum WaylandPasteHelper {
    Wtype,
    Ydotool,
}

pub fn paste_text(
    text: &str,
    delay_ms: u32,
    clipboard_restore_delay_ms: u64,
    use_clipboard: bool,
    preserve_clipboard: bool,
    paste_method: &str,
    focus_window_id: Option<&str>,
) -> Result<(), String> {
    let session = runtime::detect_session_type();
    let helpers = runtime::detect_helpers();

    if !use_clipboard {
        maybe_focus_window(session, &helpers, focus_window_id);
        return paste_without_clipboard(text, delay_ms, paste_method, session, &helpers);
    }

    // If preserving clipboard but we can't read it, fall back to "type" injection instead of
    // clobbering it. This keeps the user promise ("don't clobber my clipboard") even if helpers
    // are missing or the clipboard contains non-text data.
    let previous_clipboard = if preserve_clipboard {
        match capture_clipboard_text_for_restore(session, &helpers) {
            Ok(previous) => Some(previous),
            Err(_) => {
                maybe_focus_window(session, &helpers, focus_window_id);
                return paste_without_clipboard(text, delay_ms, paste_method, session, &helpers);
            }
        }
    } else {
        None
    };

    let resolution = runtime::resolve_paste_method(paste_method, session, &helpers);
    if !matches!(
        resolution.method,
        PasteMethod::ClipboardOnly | PasteMethod::Unavailable
    ) {
        maybe_focus_window(session, &helpers, focus_window_id);
    }

    if preserve_clipboard && matches!(resolution.method, PasteMethod::ClipboardOnly) {
        return Err(
            "Preserve clipboard is not compatible with 'Clipboard only' paste method.".to_string(),
        );
    }

    let clipboard_restore_delay_ms =
        validated_clipboard_restore_delay_ms(clipboard_restore_delay_ms);

    match resolution.method {
        PasteMethod::X11CtrlV => paste_x11(
            text,
            delay_ms,
            &previous_clipboard,
            clipboard_restore_delay_ms,
        ),
        PasteMethod::WaylandWtype => paste_wayland(
            text,
            delay_ms,
            &previous_clipboard,
            clipboard_restore_delay_ms,
            &helpers,
            WaylandPasteHelper::Wtype,
        ),
        PasteMethod::WaylandYdotool => paste_wayland(
            text,
            delay_ms,
            &previous_clipboard,
            clipboard_restore_delay_ms,
            &helpers,
            WaylandPasteHelper::Ydotool,
        ),
        PasteMethod::ClipboardOnly => {
            paste_clipboard_only(text, &previous_clipboard, session, &helpers)
        }
        PasteMethod::Unavailable => {
            let detail = if resolution.missing_helpers.is_empty() {
                "Paste method unavailable".to_string()
            } else {
                format!("Missing helpers: {}", resolution.missing_helpers.join(", "))
            };
            if use_clipboard && !preserve_clipboard {
                if paste_clipboard_only(text, &previous_clipboard, session, &helpers).is_ok() {
                    return Ok(());
                }
            }
            Err(detail)
        }
    }
}

pub fn copy_text(text: &str) -> Result<(), String> {
    let session = runtime::detect_session_type();
    let helpers = runtime::detect_helpers();
    paste_clipboard_only(text, &None, session, &helpers)
}

pub fn capture_focus_window() -> Option<String> {
    let session = runtime::detect_session_type();
    if session != SessionType::X11 {
        return None;
    }
    let helpers = runtime::detect_helpers();
    if !helpers.xdotool {
        return None;
    }

    let output = Command::new("xdotool")
        .arg("getwindowfocus")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
}

fn maybe_focus_window(
    session: SessionType,
    helpers: &HelperAvailability,
    focus_window_id: Option<&str>,
) {
    if session != SessionType::X11 || !helpers.xdotool {
        return;
    }
    if let Some(window_id) = focus_window_id {
        let _ = focus_x11(window_id);
    }
}

fn focus_x11(window_id: &str) -> Result<(), String> {
    let status = Command::new("xdotool")
        .args(["windowactivate", "--sync", window_id])
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("xdotool failed with status {status}"))
    }
}

fn paste_without_clipboard(
    text: &str,
    delay_ms: u32,
    paste_method: &str,
    session: SessionType,
    helpers: &HelperAvailability,
) -> Result<(), String> {
    match session {
        SessionType::X11 | SessionType::Windows | SessionType::Macos => type_x11(text, delay_ms),
        SessionType::Wayland => {
            let helper = resolve_wayland_type_helper(paste_method, helpers)?;
            type_wayland(text, delay_ms, helper)
        }
        SessionType::Unknown => Err("No display session detected".to_string()),
    }
}

fn resolve_wayland_type_helper(
    paste_method: &str,
    helpers: &HelperAvailability,
) -> Result<WaylandPasteHelper, String> {
    let normalized = paste_method.trim().to_lowercase();
    let request = if normalized.is_empty() {
        "auto"
    } else {
        normalized.as_str()
    };

    match request {
        "wayland_wtype" => helpers
            .wtype
            .then_some(WaylandPasteHelper::Wtype)
            .ok_or_else(|| "Missing helpers: wtype".to_string()),
        "wayland_ydotool" => helpers
            .ydotool
            .then_some(WaylandPasteHelper::Ydotool)
            .ok_or_else(|| {
                if helpers.ydotool_bin {
                    "Missing helpers: ydotoold".to_string()
                } else {
                    "Missing helpers: ydotool".to_string()
                }
            }),
        "clipboard_only" => {
            Err("Clipboard-only paste is disabled when copy-to-clipboard is off".to_string())
        }
        "x11_ctrl_v" | "auto" | _ => {
            if helpers.wtype {
                Ok(WaylandPasteHelper::Wtype)
            } else if helpers.ydotool {
                Ok(WaylandPasteHelper::Ydotool)
            } else {
                let ydotool_missing = if helpers.ydotool_bin {
                    "ydotoold"
                } else {
                    "ydotool"
                };
                Err(format!("Missing helpers: wtype, {ydotool_missing}"))
            }
        }
    }
}

fn type_x11(text: &str, delay_ms: u32) -> Result<(), String> {
    if delay_ms > 0 {
        thread::sleep(Duration::from_millis(delay_ms as u64));
    }

    let mut enigo = Enigo::new();
    enigo.key_sequence(text);
    Ok(())
}

fn type_wayland(text: &str, delay_ms: u32, helper: WaylandPasteHelper) -> Result<(), String> {
    if delay_ms > 0 {
        thread::sleep(Duration::from_millis(delay_ms as u64));
    }

    match helper {
        WaylandPasteHelper::Wtype => send_wtype_text(text),
        WaylandPasteHelper::Ydotool => send_ydotool_text(text),
    }
}

fn paste_x11(
    text: &str,
    delay_ms: u32,
    previous_clipboard: &Option<String>,
    clipboard_restore_delay_ms: u64,
) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|err| err.to_string())?;

    clipboard
        .set_text(text.to_string())
        .map_err(|err| err.to_string())?;

    if delay_ms > 0 {
        thread::sleep(Duration::from_millis(delay_ms as u64));
    }

    let mut enigo = Enigo::new();
    let modifier = paste_modifier_key();
    enigo.key_down(modifier);
    enigo.key_click(Key::Layout('v'));
    enigo.key_up(modifier);

    if let Some(previous_text) = previous_clipboard.as_deref() {
        // Give the target app a moment to consume the clipboard on paste before restoring.
        thread::sleep(Duration::from_millis(clipboard_restore_delay_ms));
        if let Err(err) = clipboard.set_text(previous_text.to_string()) {
            return Err(format!(
                "Failed to restore clipboard (previous_clipboard restore path) after {clipboard_restore_delay_ms}ms: {err}"
            ));
        }
    }

    Ok(())
}

fn paste_wayland(
    text: &str,
    delay_ms: u32,
    previous_clipboard: &Option<String>,
    clipboard_restore_delay_ms: u64,
    helpers: &HelperAvailability,
    helper: WaylandPasteHelper,
) -> Result<(), String> {
    if !helpers.wl_copy {
        return Err("wl-copy is required for Wayland clipboard support".to_string());
    }

    wl_copy_text(text)?;

    if delay_ms > 0 {
        thread::sleep(Duration::from_millis(delay_ms as u64));
    }

    match helper {
        WaylandPasteHelper::Wtype => send_wtype_paste()?,
        WaylandPasteHelper::Ydotool => send_ydotool_paste()?,
    }

    if let Some(previous_text) = previous_clipboard.as_deref() {
        // Give the target app a moment to consume the clipboard on paste before restoring.
        thread::sleep(Duration::from_millis(clipboard_restore_delay_ms));
        if let Err(err) = wl_copy_text(previous_text) {
            // Best-effort restore; failures are intermittent on some Wayland setups.
            // Avoid logging clipboard contents; length is usually enough for debugging.
            log::debug!(
                "clipboard restore failed (wl_copy_text, previous_clipboard_len={}): {:?}",
                previous_text.len(),
                err
            );
        }
    }

    Ok(())
}

fn paste_clipboard_only(
    text: &str,
    previous_clipboard: &Option<String>,
    session: SessionType,
    helpers: &HelperAvailability,
) -> Result<(), String> {
    let is_wayland = matches!(session, SessionType::Wayland);

    // Prefer wl-copy on Wayland (it's more reliable than arboard in many setups),
    // but fall back to arboard if wl-copy fails.
    if is_wayland && helpers.wl_copy {
        match wl_copy_text(text) {
            Ok(()) => {
                return Ok(());
            }
            Err(wl_err) => {
                // Fall through to arboard. If arboard succeeds, prefer not failing just
                // because wl-copy did.
                let arboard_result = set_clipboard_text(text);
                if arboard_result.is_ok() {
                    if let Some(previous_text) = previous_clipboard.as_deref() {
                        // Best-effort restore; prefer wl-copy when available.
                        let _ = wl_copy_text(previous_text)
                            .or_else(|_| set_clipboard_text(previous_text));
                    }
                    return Ok(());
                }

                // If both fail, return the wl-copy error (more actionable on Wayland).
                return Err(wl_err);
            }
        }
    }

    // Non-Wayland (or no wl-copy): use arboard.
    let arboard_result = set_clipboard_text(text);
    if arboard_result.is_ok() {
        if let Some(previous_text) = previous_clipboard.as_deref() {
            let _ = set_clipboard_text(previous_text);
        }
        return Ok(());
    }

    if is_wayland && !helpers.wl_copy {
        return Err("wl-copy is required for Wayland clipboard support".to_string());
    }

    if session == SessionType::Unknown {
        return Err("No display session detected".to_string());
    }

    arboard_result
}

fn set_clipboard_text(text: &str) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|err| err.to_string())?;
    clipboard
        .set_text(text.to_string())
        .map_err(|err| err.to_string())
}

fn capture_clipboard_text_for_restore(
    session: SessionType,
    helpers: &HelperAvailability,
) -> Result<String, String> {
    // Only preserves text clipboard contents.
    if matches!(session, SessionType::Wayland) {
        if !helpers.wl_paste {
            return Err("wl-paste is required to preserve the clipboard on Wayland".to_string());
        }
        return wl_paste_text();
    }

    let mut clipboard = Clipboard::new().map_err(|err| err.to_string())?;
    clipboard.get_text().map_err(|err| err.to_string())
}

fn wl_copy_text(text: &str) -> Result<(), String> {
    let mut child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "Failed to open wl-copy stdin".to_string())?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|err| err.to_string())?;
    }

    let status = child.wait().map_err(|err| err.to_string())?;
    if !status.success() {
        return Err(format!("wl-copy failed with status {status}"));
    }

    Ok(())
}

fn wl_paste_text() -> Result<String, String> {
    let output = Command::new("wl-paste")
        .output()
        .map_err(|err| err.to_string())?;

    if !output.status.success() {
        return Err(format!("wl-paste failed with status {}", output.status));
    }

    String::from_utf8(output.stdout).map_err(|err| err.to_string())
}

fn send_wtype_paste() -> Result<(), String> {
    let status = Command::new("wtype")
        .args(["-M", "ctrl", "-k", "v", "-m", "ctrl"])
        .status()
        .map_err(|err| err.to_string())?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("wtype failed with status {status}"))
    }
}

fn send_wtype_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }

    let status = Command::new("wtype")
        .arg("--")
        .arg(text)
        .status()
        .map_err(|err| err.to_string())?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("wtype failed with status {status}"))
    }
}

fn send_ydotool_paste() -> Result<(), String> {
    let status = Command::new("ydotool")
        .args(["key", "29:1", "47:1", "47:0", "29:0"])
        .status()
        .map_err(|err| err.to_string())?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("ydotool failed with status {status}"))
    }
}

fn send_ydotool_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }

    let status = Command::new("ydotool")
        .args(["type", "--", text])
        .status()
        .map_err(|err| err.to_string())?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("ydotool failed with status {status}"))
    }
}

fn paste_modifier_key() -> Key {
    #[cfg(target_os = "macos")]
    {
        Key::Meta
    }

    #[cfg(not(target_os = "macos"))]
    {
        Key::Control
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::runtime::HelperAvailability;

    #[test]
    fn wayland_helper_prefers_wtype() {
        let helpers = HelperAvailability {
            wl_copy: true,
            wl_paste: true,
            wtype: true,
            ydotool_bin: true,
            ydotool: true,
            xdotool: false,
        };
        let helper = resolve_wayland_type_helper("auto", &helpers).expect("helper");
        assert!(matches!(helper, WaylandPasteHelper::Wtype));
    }

    #[test]
    fn wayland_helper_falls_back_to_ydotool() {
        let helpers = HelperAvailability {
            wl_copy: true,
            wl_paste: true,
            wtype: false,
            ydotool_bin: true,
            ydotool: true,
            xdotool: false,
        };
        let helper = resolve_wayland_type_helper("auto", &helpers).expect("helper");
        assert!(matches!(helper, WaylandPasteHelper::Ydotool));
    }

    #[test]
    fn wayland_helper_errors_when_missing() {
        let helpers = HelperAvailability {
            wl_copy: true,
            wl_paste: true,
            wtype: false,
            ydotool_bin: false,
            ydotool: false,
            xdotool: false,
        };
        let err = resolve_wayland_type_helper("auto", &helpers).unwrap_err();
        assert!(err.contains("wtype"));
    }

    #[test]
    fn wayland_helper_respects_specific_request() {
        let helpers = HelperAvailability {
            wl_copy: true,
            wl_paste: true,
            wtype: true,
            ydotool_bin: true,
            ydotool: true,
            xdotool: false,
        };
        let helper = resolve_wayland_type_helper("wayland_ydotool", &helpers).expect("helper");
        assert!(matches!(helper, WaylandPasteHelper::Ydotool));
    }
}
