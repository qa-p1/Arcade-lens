//! Start Lens at login, and register it with the desktop's app launcher.

use std::fs;
use std::path::{Path, PathBuf};

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

/// Our executable. After an upgrade or rebuild Linux reports the replaced
/// file as `… (deleted)`; new processes should run the new binary.
pub fn current_exe() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(exe.to_str().and_then(|s| s.strip_suffix(" (deleted)")).map(PathBuf::from).unwrap_or(exe))
}

/// What to run to start Lens later: at login, from the menu, or a restart.
/// Inside an AppImage that is the AppImage file, since our executable only
/// exists while the image is mounted.
pub fn launch_path() -> std::io::Result<PathBuf> {
    if cfg!(target_os = "linux") {
        if let Some(image) = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file()) {
            return Ok(image);
        }
    }
    current_exe()
}

/// Writes `contents` unless the file already holds exactly that.
#[cfg_attr(windows, allow(dead_code))]
fn write_if_changed(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if fs::read(path).is_ok_and(|c| c == contents) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, contents)
}

/// `exe` quoted for a desktop entry's `Exec` key.
#[cfg(target_os = "linux")]
fn exec(exe: &Path, args: &str) -> String {
    let quoted = exe.display().to_string().replace('\\', "\\\\").replace('"', "\\\"").replace('`', "\\`").replace('$', "\\$").replace('%', "%%");
    if args.is_empty() {
        format!("\"{quoted}\"")
    } else {
        format!("\"{quoted}\" {args}")
    }
}

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home().map(|h| h.join(".local/share")))
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

/// Starts `exe` in the background at login. Rewrites an existing entry, so
/// it also repoints the login item after Lens moves.
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
                &format!("\"{}\" --background", exe.display()),
                "/f",
            ])
            .status()?;
        return if status.success() { Ok(()) } else { Err(std::io::Error::other("reg add failed")) };
    }
    #[allow(unreachable_code)]
    {
        let path = autostart_path().ok_or_else(|| std::io::Error::other("no home directory"))?;
        #[cfg(target_os = "linux")]
        write_if_changed(
            &path,
            format!(
                "[Desktop Entry]\nType=Application\nName=Arcade Lens\nComment=Start Arcade Lens in the background\nExec={}\nIcon=arcade-lens\nTerminal=false\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n",
                exec(exe, "--background")
            )
            .as_bytes(),
        )?;
        #[cfg(target_os = "macos")]
        write_if_changed(
            &path,
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>Label</key><string>dev.arcade.lens</string>\n<key>ProgramArguments</key><array><string>{}</string><string>--background</string></array>\n<key>RunAtLoad</key><true/>\n</dict></plist>\n",
                exe.display()
            )
            .as_bytes(),
        )?;
        let _ = (path, exe);
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

/// The launcher entry, when Lens is in the Linux applications menu.
pub fn launcher_path() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        data_home().map(|d| d.join("applications/arcade-lens.desktop"))
    } else {
        None
    }
}

pub fn launcher_installed() -> bool {
    launcher_path().is_some_and(|p| p.exists())
}

/// Adds Arcade Lens to the Linux applications menu: a launcher entry (with
/// Capture and Quit actions) and the app icon, `icons` being `(size, PNG)`.
/// Unchanged files are left alone, so this is cheap to repeat at every start.
pub fn install_launcher(exe: &Path, icons: &[(u32, Vec<u8>)]) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let data = data_home().ok_or_else(|| std::io::Error::other("no home directory"))?;
        for (size, png) in icons {
            write_if_changed(&data.join(format!("icons/hicolor/{size}x{size}/apps/arcade-lens.png")), png)?;
        }
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=Arcade Lens\nGenericName=Screen Lens\nComment=Select anything on screen and act on it\nExec={}\nIcon=arcade-lens\nTerminal=false\nCategories=Utility;Graphics;\nKeywords=screenshot;capture;ocr;text;qr;color;pin;annotate;lens;\nStartupNotify=false\nStartupWMClass=arcade-lens-settings\nActions=capture;quit;\n\n[Desktop Action capture]\nName=Capture Screen\nExec={}\n\n[Desktop Action quit]\nName=Quit Arcade Lens\nExec={}\n",
            exec(exe, ""),
            exec(exe, "--capture"),
            exec(exe, "--quit"),
        );
        write_if_changed(&data.join("applications/arcade-lens.desktop"), entry.as_bytes())?;
        // Earlier versions added a second, Settings-only entry.
        let _ = fs::remove_file(data.join("applications/arcade-lens-settings.desktop"));
        return Ok(());
    }
    #[allow(unreachable_code)]
    {
        let _ = (exe, icons);
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn exec_quotes_paths() {
        assert_eq!(exec(Path::new("/opt/Arcade Lens/arcade-lens"), "--background"), "\"/opt/Arcade Lens/arcade-lens\" --background");
        assert_eq!(exec(Path::new("/a/$b\"c"), ""), "\"/a/\\$b\\\"c\"");
    }
}
