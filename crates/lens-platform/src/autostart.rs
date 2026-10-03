//! Start Lens at login, and register it with the desktop's app launcher.

use std::fs;
use std::path::{Path, PathBuf};

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

#[cfg(target_os = "linux")]
fn desktop_entry(exe: &Path, args: &str, name: &str, autostart: bool) -> String {
    let mut s = format!(
        "[Desktop Entry]\nType=Application\nName={name}\nComment=Select anything on screen and act on it\nExec=\"{}\" {args}\nIcon=zoom-select\nTerminal=false\nCategories=Utility;\n",
        exe.display()
    );
    if autostart {
        s.push_str("X-GNOME-Autostart-enabled=true\nNoDisplay=true\n");
    }
    s
}

fn autostart_path() -> Option<PathBuf> {
    let h = home()?;
    if cfg!(target_os = "linux") {
        let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| h.join(".config"));
        Some(base.join("autostart/arcade-lens.desktop"))
    } else if cfg!(target_os = "macos") {
        Some(h.join("Library/LaunchAgents/dev.arcade.lens.plist"))
    } else {
        None
    }
}

pub fn is_enabled() -> bool {
    #[cfg(windows)]
    {
        return std::process::Command::new("reg")
            .args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "ArcadeLens"])
            .output()
            .is_ok_and(|o| o.status.success());
    }
    #[allow(unreachable_code)]
    autostart_path().is_some_and(|p| p.exists())
}

pub fn enable(exe: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let status = std::process::Command::new("reg")
            .args([
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "ArcadeLens",
                "/t",
                "REG_SZ",
                "/d",
                &format!("\"{}\" start", exe.display()),
                "/f",
            ])
            .status()?;
        return if status.success() { Ok(()) } else { Err(std::io::Error::other("reg add failed")) };
    }
    #[allow(unreachable_code)]
    {
        let path = autostart_path().ok_or_else(|| std::io::Error::other("no home directory"))?;
        fs::create_dir_all(path.parent().unwrap())?;
        #[cfg(target_os = "linux")]
        fs::write(&path, desktop_entry(exe, "start", "Arcade Lens", true))?;
        #[cfg(target_os = "macos")]
        fs::write(
            &path,
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>Label</key><string>dev.arcade.lens</string>\n<key>ProgramArguments</key><array><string>{}</string><string>start</string></array>\n<key>RunAtLoad</key><true/>\n</dict></plist>\n",
                exe.display()
            ),
        )?;
        Ok(())
    }
}

pub fn disable() -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("reg").args(["delete", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "ArcadeLens", "/f"]).status()?;
        return Ok(());
    }
    #[allow(unreachable_code)]
    match autostart_path() {
        Some(p) if p.exists() => fs::remove_file(p),
        _ => Ok(()),
    }
}

/// Adds "Arcade Lens" and "Arcade Lens Settings" to the Linux app launcher.
pub fn install_launcher(exe: &Path) -> std::io::Result<Vec<PathBuf>> {
    #[cfg(target_os = "linux")]
    {
        let h = home().ok_or_else(|| std::io::Error::other("no home directory"))?;
        let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| h.join(".local/share"));
        let dir = base.join("applications");
        fs::create_dir_all(&dir)?;
        let a = dir.join("arcade-lens.desktop");
        let b = dir.join("arcade-lens-settings.desktop");
        fs::write(&a, desktop_entry(exe, "capture", "Arcade Lens", false))?;
        fs::write(&b, desktop_entry(exe, "settings", "Arcade Lens Settings", false))?;
        return Ok(vec![a, b]);
    }
    #[allow(unreachable_code)]
    {
        let _ = exe;
        Ok(vec![])
    }
}
