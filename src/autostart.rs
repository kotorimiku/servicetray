use std::{env, fs, path::PathBuf};

use color_eyre::eyre::{Result, eyre};

#[cfg(target_os = "windows")]
pub fn set_autostart(enabled: bool) -> Result<()> {
    // Create/remove a shortcut in the user's Startup folder without using the registry.
    // Use PowerShell via CreateProcess with CREATE_NO_WINDOW to avoid showing a console window.
    use std::os::windows::process::CommandExt;

    let appdata = env::var_os("APPDATA").ok_or_else(|| eyre!("APPDATA not set"))?;
    let mut startup_dir = PathBuf::from(appdata);
    startup_dir.push("Microsoft");
    startup_dir.push("Windows");
    startup_dir.push("Start Menu");
    startup_dir.push("Programs");
    startup_dir.push("Startup");

    fs::create_dir_all(&startup_dir)?;

    let exe = env::current_exe()?;
    let exe_display = exe.display();

    let shortcut_path = startup_dir.join("servicetray.lnk");

    if enabled {
        // Use PowerShell COM object to create .lnk, but create process with no window.
        let script = format!(
            r#"$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.WorkingDirectory='{}';$s.Save()"#,
            shortcut_path.display(),
            exe_display,
            exe.parent()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "".to_string())
        );

        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let status = std::process::Command::new("powershell")
            .creation_flags(CREATE_NO_WINDOW)
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-Command")
            .arg(script)
            .status()?;

        if !status.success() {
            return Err(eyre!("failed to create shortcut in startup folder"));
        }
    } else {
        if shortcut_path.exists() {
            let _ = fs::remove_file(&shortcut_path);
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub fn set_autostart(enabled: bool) -> Result<()> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| eyre!("HOME not set"))?;
    let autostart_dir = home.join(".config").join("autostart");
    fs::create_dir_all(&autostart_dir)?;

    let exe = env::current_exe()?;
    let desktop_path = autostart_dir.join("servicetray.desktop");

    if enabled {
        let contents = format!(
            "[Desktop Entry]\nType=Application\nName=Service Tray\nExec={} \nX-GNOME-Autostart-enabled=true\nNoDisplay=false\n",
            exe.display()
        );
        fs::write(&desktop_path, contents)?;
    } else {
        if desktop_path.exists() {
            let _ = fs::remove_file(desktop_path);
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
pub fn set_autostart(enabled: bool) -> Result<()> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| eyre!("HOME not set"))?;
    let launch_agents = home.join("Library").join("LaunchAgents");
    fs::create_dir_all(&launch_agents)?;

    let exe = env::current_exe()?;
    let plist_path = launch_agents.join("com.servicetray.startup.plist");

    if enabled {
        let contents = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.servicetray.startup</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>"#,
            exe.display()
        );
        fs::write(&plist_path, contents)?;
        // Try to load it immediately (best-effort)
        let _ = std::process::Command::new("launchctl")
            .arg("load")
            .arg(&plist_path)
            .status();
    } else {
        // Try unloading first
        let _ = std::process::Command::new("launchctl")
            .arg("unload")
            .arg(&plist_path)
            .status();
        if plist_path.exists() {
            let _ = fs::remove_file(plist_path);
        }
    }

    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub fn set_autostart(_enabled: bool) -> Result<()> {
    // Unsupported platform: no-op
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_autostart() {
        let res = set_autostart(true);
        assert!(res.is_ok());
        let res_off = set_autostart(false);
        assert!(res_off.is_ok());
    }
}
