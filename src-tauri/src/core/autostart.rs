use std::fs;
use std::path::PathBuf;

pub fn apply_launch_on_login(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        return apply_linux(enabled);
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = enabled;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn apply_linux(enabled: bool) -> Result<(), String> {
    let config_dir = if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(dir)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config")
    } else {
        return Err("Unable to resolve config directory for autostart".to_string());
    };

    let autostart_dir = config_dir.join("autostart");
    let desktop_path = autostart_dir.join("whispr.desktop");

    if !enabled {
        if desktop_path.exists() {
            fs::remove_file(&desktop_path).map_err(|err| err.to_string())?;
        }
        return Ok(());
    }

    fs::create_dir_all(&autostart_dir).map_err(|err| err.to_string())?;
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let exec = exe.to_string_lossy();
    let contents = format!(
    "[Desktop Entry]\nType=Application\nName=Whispr\nExec=\"{exec}\"\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n"
  );
    fs::write(&desktop_path, contents).map_err(|err| err.to_string())?;
    Ok(())
}
