use std::env;
use std::path::Path;

use crate::types::RuntimeInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionType {
    Wayland,
    X11,
    Macos,
    Windows,
    Unknown,
}

impl SessionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionType::Wayland => "wayland",
            SessionType::X11 => "x11",
            SessionType::Macos => "macos",
            SessionType::Windows => "windows",
            SessionType::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HelperAvailability {
    pub wl_copy: bool,
    pub wl_paste: bool,
    pub wtype: bool,
    // True if the ydotool binary exists (regardless of daemon/socket availability).
    pub ydotool_bin: bool,
    // True if ydotool is likely usable (binary exists and daemon/socket is present).
    pub ydotool: bool,
    pub xdotool: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteMethod {
    X11CtrlV,
    WaylandWtype,
    WaylandYdotool,
    ClipboardOnly,
    Unavailable,
}

impl PasteMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            PasteMethod::X11CtrlV => "x11_ctrl_v",
            PasteMethod::WaylandWtype => "wayland_wtype",
            PasteMethod::WaylandYdotool => "wayland_ydotool",
            PasteMethod::ClipboardOnly => "clipboard_only",
            PasteMethod::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PasteResolution {
    pub method: PasteMethod,
    pub missing_helpers: Vec<String>,
}

pub fn detect_session_type() -> SessionType {
    if cfg!(target_os = "macos") {
        return SessionType::Macos;
    }

    if cfg!(target_os = "windows") {
        return SessionType::Windows;
    }

    if let Ok(value) = env::var("XDG_SESSION_TYPE") {
        match value.to_lowercase().as_str() {
            "wayland" => return SessionType::Wayland,
            "x11" => return SessionType::X11,
            _ => {}
        }
    }

    if env::var_os("WAYLAND_DISPLAY").is_some() {
        return SessionType::Wayland;
    }

    if env::var_os("DISPLAY").is_some() {
        return SessionType::X11;
    }

    SessionType::Unknown
}

pub fn detect_helpers() -> HelperAvailability {
    let ydotool_bin = command_exists("ydotool");
    HelperAvailability {
        wl_copy: command_exists("wl-copy"),
        wl_paste: command_exists("wl-paste"),
        wtype: command_exists("wtype"),
        ydotool_bin,
        ydotool: ydotool_bin && ydotool_socket_available(),
        xdotool: command_exists("xdotool"),
    }
}

#[cfg(target_os = "linux")]
fn ydotool_socket_available() -> bool {
    use std::fs;
    use std::os::unix::fs::FileTypeExt;
    use std::path::PathBuf;

    fn is_socket(path: &Path) -> bool {
        fs::metadata(path)
            .map(|meta| meta.file_type().is_socket())
            .unwrap_or(false)
    }

    // ydotool reads YDOTOOL_SOCKET if provided.
    if let Ok(socket) = env::var("YDOTOOL_SOCKET") {
        let socket = socket.trim();
        if !socket.is_empty() && is_socket(Path::new(socket)) {
            return true;
        }
    }

    // Common defaults vary by distro/package.
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(runtime_dir) = env::var("XDG_RUNTIME_DIR") {
        let base = PathBuf::from(runtime_dir);
        candidates.push(base.join("ydotool_socket"));
        candidates.push(base.join("ydotoold").join("socket"));
    }
    candidates.push(PathBuf::from("/tmp/.ydotool_socket"));

    candidates.iter().any(|path| is_socket(path))
}

#[cfg(not(target_os = "linux"))]
fn ydotool_socket_available() -> bool {
    // ydotool is only meaningful on Linux/Wayland; other platforms don't need daemon probing.
    true
}

pub fn resolve_paste_method(
    requested: &str,
    session: SessionType,
    helpers: &HelperAvailability,
) -> PasteResolution {
    let normalized = requested.trim().to_lowercase();
    let request = if normalized.is_empty() {
        "auto"
    } else {
        normalized.as_str()
    };

    let request = if request == "x11_ctrl_v" && session == SessionType::Wayland {
        "auto"
    } else {
        request
    };

    match request {
        "auto" => resolve_auto(session, helpers),
        "x11_ctrl_v" => PasteResolution {
            method: PasteMethod::X11CtrlV,
            missing_helpers: Vec::new(),
        },
        "wayland_wtype" => resolve_wayland_specific(session, helpers, PasteMethod::WaylandWtype),
        "wayland_ydotool" => {
            resolve_wayland_specific(session, helpers, PasteMethod::WaylandYdotool)
        }
        "clipboard_only" => resolve_clipboard_only(session, helpers),
        _ => resolve_auto(session, helpers),
    }
}

pub fn runtime_info(
    paste_method: &str,
    use_clipboard: bool,
    preserve_clipboard: bool,
) -> RuntimeInfo {
    let session = detect_session_type();
    let helpers = detect_helpers();
    let mut resolution = if use_clipboard {
        resolve_paste_method(paste_method, session, &helpers)
    } else {
        resolve_no_clipboard(paste_method, session, &helpers)
    };

    // Preserving clipboard on Wayland requires wl-paste (in addition to wl-copy).
    if preserve_clipboard && session == SessionType::Wayland && !helpers.wl_paste {
        if !resolution.missing_helpers.iter().any(|h| h == "wl-paste") {
            resolution.missing_helpers.push("wl-paste".to_string());
        }
    }

    RuntimeInfo {
        session_type: session.as_str().to_string(),
        hotkeys_supported: matches!(
            session,
            SessionType::X11 | SessionType::Windows | SessionType::Macos
        ),
        paste_method: resolution.method.as_str().to_string(),
        missing_helpers: resolution.missing_helpers,
    }
}

fn resolve_no_clipboard(
    requested: &str,
    session: SessionType,
    helpers: &HelperAvailability,
) -> PasteResolution {
    match session {
        SessionType::Wayland => resolve_wayland_no_clipboard(requested, helpers),
        SessionType::X11 => PasteResolution {
            method: PasteMethod::X11CtrlV,
            missing_helpers: Vec::new(),
        },
        SessionType::Macos => PasteResolution {
            method: PasteMethod::X11CtrlV,
            missing_helpers: Vec::new(),
        },
        SessionType::Windows => PasteResolution {
            method: PasteMethod::X11CtrlV,
            missing_helpers: Vec::new(),
        },
        SessionType::Unknown => PasteResolution {
            method: PasteMethod::Unavailable,
            missing_helpers: vec!["display".to_string()],
        },
    }
}

fn resolve_wayland_no_clipboard(requested: &str, helpers: &HelperAvailability) -> PasteResolution {
    let normalized = requested.trim().to_lowercase();
    let request = if normalized.is_empty() {
        "auto"
    } else {
        normalized.as_str()
    };

    match request {
        "wayland_wtype" => {
            if helpers.wtype {
                PasteResolution {
                    method: PasteMethod::WaylandWtype,
                    missing_helpers: Vec::new(),
                }
            } else {
                PasteResolution {
                    method: PasteMethod::Unavailable,
                    missing_helpers: vec!["wtype".to_string()],
                }
            }
        }
        "wayland_ydotool" => {
            if helpers.ydotool {
                PasteResolution {
                    method: PasteMethod::WaylandYdotool,
                    missing_helpers: Vec::new(),
                }
            } else {
                PasteResolution {
                    method: PasteMethod::Unavailable,
                    missing_helpers: vec![if helpers.ydotool_bin {
                        "ydotoold".to_string()
                    } else {
                        "ydotool".to_string()
                    }],
                }
            }
        }
        "clipboard_only" => PasteResolution {
            method: PasteMethod::Unavailable,
            missing_helpers: vec!["wtype".to_string(), "ydotool".to_string()],
        },
        "x11_ctrl_v" | "auto" | _ => {
            if helpers.wtype {
                PasteResolution {
                    method: PasteMethod::WaylandWtype,
                    missing_helpers: Vec::new(),
                }
            } else if helpers.ydotool {
                PasteResolution {
                    method: PasteMethod::WaylandYdotool,
                    missing_helpers: Vec::new(),
                }
            } else {
                PasteResolution {
                    method: PasteMethod::Unavailable,
                    missing_helpers: vec![
                        "wtype".to_string(),
                        if helpers.ydotool_bin {
                            "ydotoold".to_string()
                        } else {
                            "ydotool".to_string()
                        },
                    ],
                }
            }
        }
    }
}

fn resolve_auto(session: SessionType, helpers: &HelperAvailability) -> PasteResolution {
    match session {
        SessionType::Wayland => resolve_wayland_auto(helpers),
        SessionType::X11 => PasteResolution {
            method: PasteMethod::X11CtrlV,
            missing_helpers: Vec::new(),
        },
        SessionType::Macos => PasteResolution {
            method: PasteMethod::X11CtrlV,
            missing_helpers: Vec::new(),
        },
        SessionType::Windows => PasteResolution {
            method: PasteMethod::X11CtrlV,
            missing_helpers: Vec::new(),
        },
        SessionType::Unknown => PasteResolution {
            method: PasteMethod::Unavailable,
            missing_helpers: vec!["display".to_string()],
        },
    }
}

fn resolve_wayland_auto(helpers: &HelperAvailability) -> PasteResolution {
    if !helpers.wl_copy {
        return PasteResolution {
            method: PasteMethod::Unavailable,
            missing_helpers: vec!["wl-copy".to_string()],
        };
    }

    if helpers.wtype {
        return PasteResolution {
            method: PasteMethod::WaylandWtype,
            missing_helpers: Vec::new(),
        };
    }

    if helpers.ydotool {
        return PasteResolution {
            method: PasteMethod::WaylandYdotool,
            missing_helpers: Vec::new(),
        };
    }

    PasteResolution {
        method: PasteMethod::ClipboardOnly,
        missing_helpers: vec![
            "wtype".to_string(),
            if helpers.ydotool_bin {
                "ydotoold".to_string()
            } else {
                "ydotool".to_string()
            },
        ],
    }
}

fn resolve_wayland_specific(
    session: SessionType,
    helpers: &HelperAvailability,
    method: PasteMethod,
) -> PasteResolution {
    if session != SessionType::Wayland {
        return PasteResolution {
            method: PasteMethod::Unavailable,
            missing_helpers: vec!["wayland-session".to_string()],
        };
    }

    let mut missing = Vec::new();
    if !helpers.wl_copy {
        missing.push("wl-copy".to_string());
    }

    match method {
        PasteMethod::WaylandWtype => {
            if !helpers.wtype {
                missing.push("wtype".to_string());
            }
        }
        PasteMethod::WaylandYdotool => {
            if !helpers.ydotool {
                missing.push(if helpers.ydotool_bin {
                    "ydotoold".to_string()
                } else {
                    "ydotool".to_string()
                });
            }
        }
        _ => {}
    }

    if missing.is_empty() {
        PasteResolution {
            method,
            missing_helpers: Vec::new(),
        }
    } else {
        PasteResolution {
            method: PasteMethod::Unavailable,
            missing_helpers: missing,
        }
    }
}

fn resolve_clipboard_only(session: SessionType, helpers: &HelperAvailability) -> PasteResolution {
    match session {
        SessionType::Wayland => {
            if helpers.wl_copy {
                PasteResolution {
                    method: PasteMethod::ClipboardOnly,
                    missing_helpers: Vec::new(),
                }
            } else {
                PasteResolution {
                    method: PasteMethod::Unavailable,
                    missing_helpers: vec!["wl-copy".to_string()],
                }
            }
        }
        SessionType::X11 => PasteResolution {
            method: PasteMethod::ClipboardOnly,
            missing_helpers: Vec::new(),
        },
        SessionType::Macos => PasteResolution {
            method: PasteMethod::ClipboardOnly,
            missing_helpers: Vec::new(),
        },
        SessionType::Windows => PasteResolution {
            method: PasteMethod::ClipboardOnly,
            missing_helpers: Vec::new(),
        },
        SessionType::Unknown => PasteResolution {
            method: PasteMethod::Unavailable,
            missing_helpers: vec!["display".to_string()],
        },
    }
}

fn command_exists(name: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&paths).any(|path| {
        let candidate = path.join(name);
        if candidate.is_file() {
            return is_executable(&candidate);
        }
        false
    })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_wayland_prefers_wtype() {
        let helpers = HelperAvailability {
            wl_copy: true,
            wl_paste: true,
            wtype: true,
            ydotool_bin: true,
            ydotool: true,
            xdotool: false,
        };
        let resolution = resolve_paste_method("auto", SessionType::Wayland, &helpers);
        assert_eq!(resolution.method, PasteMethod::WaylandWtype);
    }

    #[test]
    fn auto_wayland_falls_back_to_clipboard_only() {
        let helpers = HelperAvailability {
            wl_copy: true,
            wl_paste: true,
            wtype: false,
            ydotool_bin: false,
            ydotool: false,
            xdotool: false,
        };
        let resolution = resolve_paste_method("auto", SessionType::Wayland, &helpers);
        assert_eq!(resolution.method, PasteMethod::ClipboardOnly);
        assert!(resolution.missing_helpers.contains(&"wtype".to_string()));
    }

    #[test]
    fn wayland_wtype_requires_helpers() {
        let helpers = HelperAvailability {
            wl_copy: true,
            wl_paste: true,
            wtype: false,
            ydotool_bin: false,
            ydotool: false,
            xdotool: false,
        };
        let resolution = resolve_paste_method("wayland_wtype", SessionType::Wayland, &helpers);
        assert_eq!(resolution.method, PasteMethod::Unavailable);
        assert!(resolution.missing_helpers.contains(&"wtype".to_string()));
    }

    #[test]
    fn x11_ctrl_v_on_wayland_uses_auto() {
        let helpers = HelperAvailability {
            wl_copy: true,
            wl_paste: true,
            wtype: true,
            ydotool_bin: false,
            ydotool: false,
            xdotool: false,
        };
        let resolution = resolve_paste_method("x11_ctrl_v", SessionType::Wayland, &helpers);
        assert_eq!(resolution.method, PasteMethod::WaylandWtype);
    }

    #[test]
    fn windows_auto_uses_ctrl_v() {
        let helpers = HelperAvailability {
            wl_copy: false,
            wl_paste: false,
            wtype: false,
            ydotool_bin: false,
            ydotool: false,
            xdotool: false,
        };
        let resolution = resolve_paste_method("auto", SessionType::Windows, &helpers);
        assert_eq!(resolution.method, PasteMethod::X11CtrlV);
    }

    #[test]
    fn windows_clipboard_only_allowed() {
        let helpers = HelperAvailability {
            wl_copy: false,
            wl_paste: false,
            wtype: false,
            ydotool_bin: false,
            ydotool: false,
            xdotool: false,
        };
        let resolution = resolve_paste_method("clipboard_only", SessionType::Windows, &helpers);
        assert_eq!(resolution.method, PasteMethod::ClipboardOnly);
    }
}
