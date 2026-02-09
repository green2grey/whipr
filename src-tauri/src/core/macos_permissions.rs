#[cfg(target_os = "macos")]
use core_foundation::{
    base::TCFType, boolean::CFBoolean, dictionary::CFDictionary, string::CFString,
};

#[cfg(target_os = "macos")]
use core_foundation::dictionary::CFDictionaryRef;

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;

    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

pub fn accessibility_enabled() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        AXIsProcessTrusted()
    }

    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn request_accessibility_prompt() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        // Passing `AXTrustedCheckOptionPrompt=true` triggers macOS to show the "grant Accessibility"
        // prompt. It will still return false until the user approves in System Settings.
        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let value = CFBoolean::true_value();
        let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
        AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef())
    }

    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn input_monitoring_enabled() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        CGPreflightListenEventAccess()
    }

    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn request_input_monitoring_prompt() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        // Triggers the macOS "Input Monitoring" prompt. Like Accessibility, this will return false
        // until the user approves.
        CGRequestListenEventAccess()
    }

    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn open_privacy_settings(permission: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let permission = permission.trim().to_lowercase();
        let url = match permission.as_str() {
            "accessibility" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            "input_monitoring" | "inputmonitoring" | "listen_event" | "listenevent" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
            }
            _ => {
                return Err(format!(
                    "Unknown permission '{permission}'. Expected 'accessibility' or 'input_monitoring'."
                ));
            }
        };

        let status = std::process::Command::new("open")
            .arg(url)
            .status()
            .map_err(|err| format!("Failed to open System Settings: {err}"))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("Failed to open System Settings (exit={status})."))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = permission;
        Err("System Settings permissions are only available on macOS.".to_string())
    }
}
