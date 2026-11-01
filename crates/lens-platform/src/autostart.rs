//! Start Lens at login, and register it with the desktop's app launcher.

use std::fs;
use std::path::{Path, PathBuf};

#[cfg(any(windows, test))]
mod windows_login;

/// Lens's OS identifier (STANDARD §10): desktop file, icon name, window
/// class, login entry and LaunchAgent label. Same as `link::OS_ID`.
pub const OS_ID: &str = "dev.arcade.Lens";
/// Sets the Windows AppUserModelID, so the taskbar groups Lens's windows
/// under its Start menu shortcut. Call before any window or tray icon.
pub fn init_app_identity() {
    #[cfg(windows)]
    {
        let id: Vec<u16> = OS_ID.encode_utf16().chain(Some(0)).collect();
        // SAFETY: a live, NUL-terminated UTF-16 string for the Win32 call.
        unsafe { windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(id.as_ptr()) };
    }
}

/// The launcher comment: Lens's tagline. Same as `settings_view::TAGLINE`.
pub const TAGLINE: &str = "Select anything on screen and act on it.";

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
    let exe = current_exe()?;
    #[cfg(target_os = "linux")]
    {
        let appdir = std::env::var_os("APPDIR").map(PathBuf::from);
        let image = std::env::var_os("APPIMAGE").map(PathBuf::from);
        if let Some(image) = appimage_for_exe(&exe, appdir.as_deref(), image.as_deref()) {
            return Ok(image);
        }
    }
    Ok(exe)
}

#[cfg(target_os = "linux")]
fn appimage_for_exe(exe: &Path, appdir: Option<&Path>, image: Option<&Path>) -> Option<PathBuf> {
    // A launcher may itself be an AppImage and pass its environment to Lens.
    // Only its own mounted executable should relaunch via APPIMAGE.
    let appdir = appdir?.canonicalize().ok()?;
    let exe = exe.canonicalize().ok()?;
    let image = image?;
    (exe.starts_with(appdir) && image.is_file()).then(|| image.to_path_buf())
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
    let path = exe.display().to_string();
    let quoted = path.replace('\\', "\\\\").replace('"', "\\\"").replace('`', "\\`").replace('$', "\\$").replace('%', "%%");
    // Then Desktop Entry string escaping: a raw newline would end the key.
    let quoted = quoted.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t").replace('\r', "\\r");
    // GLib looks the program up before expanding "%%"; `env` takes it as an argument.
    let program = if path.contains('%') { format!("env \"{quoted}\"") } else { format!("\"{quoted}\"") };
    if args.is_empty() {
        program
    } else {
        format!("{program} {args}")
    }
}

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home().map(|h| h.join(".local/share")))
}

fn autostart_path() -> Option<PathBuf> {
    let environment = arcade_link::install::Environment::discover().ok()?;
    if cfg!(target_os = "linux") {
        Some(environment.config.join(format!("autostart/{OS_ID}.desktop")))
    } else if cfg!(target_os = "macos") {
        Some(environment.home.join(format!("Library/LaunchAgents/{OS_ID}.plist")))
    } else {
        None
    }
}

/// The login entry Lens wrote before 0.4.
fn legacy_autostart_path() -> Option<PathBuf> {
    let environment = arcade_link::install::Environment::discover().ok()?;
    if cfg!(target_os = "linux") {
        Some(environment.config.join("autostart/arcade-lens.desktop"))
    } else if cfg!(target_os = "macos") {
        Some(environment.home.join("Library/LaunchAgents/dev.arcade.lens.plist"))
    } else {
        None
    }
}

/// Whether `old` is a different file from `new`. On a case-insensitive
/// volume (macOS by default) `dev.arcade.lens.plist` *is*
/// `dev.arcade.Lens.plist`, and removing the "old" one would remove the new.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn distinct_file(old: &Path, new: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(old), fs::metadata(new)) {
        (Ok(a), Ok(b)) => (a.dev(), a.ino()) != (b.dev(), b.ino()),
        _ => true,
    }
}

/// Removes a pre-0.4 file Lens wrote, when it is Lens's (`marker`) and not
/// the same file as its replacement.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn remove_legacy(old: &Path, new: &Path, marker: &str) -> std::io::Result<()> {
    let ours = fs::read_to_string(old).is_ok_and(|c| c.contains(marker));
    if ours && distinct_file(old, new) {
        match fs::remove_file(old) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn legacy_login_marker() -> &'static str {
    if cfg!(target_os = "macos") {
        "<string>dev.arcade.lens</string>"
    } else {
        "X-Arcade-Id=arcade.lens"
    }
}

/// Portable copies keep their login entry, but never gain an install receipt.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn set_entry(store: &arcade_link::receipt::Store, path: &Path, contents: Option<&[u8]>) -> std::io::Result<()> {
    let receipt = store.read(arcade_link::ids::LENS)?;
    let mut document = None;
    if let Some(receipt) = &receipt {
        receipt.validate_native_paths()?;
        document = Some(serde_json::from_slice::<serde_json::Value>(&fs::read(store.path(arcade_link::ids::LENS)?)?).map_err(std::io::Error::other)?);
    }
    if let Some(contents) = contents {
        write_if_changed(path, contents)?;
    } else {
        match fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    if let Some(mut document) = document {
        document["integration"]["autostart"] = serde_json::to_value(contents.map(|_| path)).map_err(std::io::Error::other)?;
        document["updatedAt"] = arcade_link::manifest::now_rfc3339().into();
        let bytes = serde_json::to_vec_pretty(&document).map_err(std::io::Error::other)?;
        arcade_link::paths::write_atomic(&store.path(arcade_link::ids::LENS)?, &bytes, true)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn login_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Arcade Lens\nComment=Start Arcade Lens in the background\nExec={}\nIcon={OS_ID}\nTerminal=false\nNoDisplay=true\nX-Arcade-Id=arcade.lens\nX-GNOME-Autostart-enabled=true\n",
        exec(exe, "--background")
    )
}

pub fn is_enabled() -> bool {
    #[cfg(windows)]
    {
        return arcade_link::install::Environment::discover().is_ok_and(|e| windows_login::enabled(&e.receipts(), windows_login::run_enabled));
    }
    // An entry switched off in the desktop's startup settings won't run.
    // The pre-0.4 entry still starts Lens until it is replaced.
    #[cfg(target_os = "linux")]
    return [autostart_path(), legacy_autostart_path()].into_iter().flatten().any(|p| arcade_link::install::autostart_enabled(&p));
    #[allow(unreachable_code)]
    [autostart_path(), legacy_autostart_path()].into_iter().flatten().any(|p| p.exists())
}

/// Starts `exe` in the background at login. Rewrites an existing entry, so
/// it also repoints the login item after Lens moves.
pub fn enable(exe: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        return windows_login::set_current(exe, true);
    }
    #[allow(unreachable_code)]
    {
        let path = autostart_path().ok_or_else(|| std::io::Error::other("no home directory"))?;
        #[cfg(target_os = "linux")]
        set_entry(&arcade_link::receipt::Store::discover(), &path, Some(login_entry(exe).as_bytes()))?;
        #[cfg(target_os = "macos")]
        set_entry(
            &arcade_link::receipt::Store::discover(),
            &path,
            Some(format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>Label</key><string>{OS_ID}</string>\n<key>ProgramArguments</key><array><string>{}</string><string>--background</string></array>\n<key>RunAtLoad</key><true/>\n</dict></plist>\n",
                exe.display()
            )
            .as_bytes()),
        )?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if let Some(old) = legacy_autostart_path() {
            remove_legacy(&old, &path, legacy_login_marker())?;
        }
        let _ = (path, exe);
        Ok(())
    }
}

#[cfg_attr(windows, allow(clippy::needless_return))]
pub fn disable() -> std::io::Result<()> {
    #[cfg(windows)]
    {
        return windows_login::set_current(&current_exe()?, false);
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    match autostart_path() {
        Some(p) => {
            if let Some(old) = legacy_autostart_path() {
                remove_legacy(&old, &p, legacy_login_marker())?;
            }
            set_entry(&arcade_link::receipt::Store::discover(), &p, None)
        }
        _ => Ok(()),
    }
}

/// Lens's launcher: the tagline, keywords, window class and the actions in
/// the tray's order and words.
#[cfg(target_os = "linux")]
fn launcher_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Arcade Lens\nGenericName=Screen Lens\nComment={TAGLINE}\nExec={}\nIcon={OS_ID}\nTerminal=false\nCategories=Utility;Graphics;\nKeywords=screenshot;capture;ocr;text;qr;color;pin;annotate;lens;\nStartupNotify=false\nStartupWMClass={OS_ID}\nX-Arcade-Id=arcade.lens\nActions=Capture;Settings;Quit;\n\n[Desktop Action Capture]\nName=Capture\nExec={}\n\n[Desktop Action Settings]\nName=Settings\nExec={}\n\n[Desktop Action Quit]\nName=Quit Arcade Lens\nExec={}\n",
        exec(exe, ""),
        exec(exe, "--capture"),
        exec(exe, "--settings"),
        exec(exe, "--quit"),
    )
}

/// The launcher entry, when Lens is in the Linux applications menu.
pub fn launcher_path() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        data_home().map(|d| d.join(format!("applications/{OS_ID}.desktop")))
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
            write_if_changed(&data.join(format!("icons/hicolor/{size}x{size}/apps/{OS_ID}.png")), png)?;
        }
        let path = data.join(format!("applications/{OS_ID}.desktop"));
        write_if_changed(&path, launcher_entry(exe).as_bytes())?;
        // Before 0.4: arcade-lens.desktop with an arcade-lens icon, and
        // earlier still a second, Settings-only entry.
        for old in ["arcade-lens", "arcade-lens-settings"] {
            remove_legacy(&data.join(format!("applications/{old}.desktop")), &path, "Name=Arcade Lens")?;
        }
        for (size, _) in icons {
            let _ = fs::remove_file(data.join(format!("icons/hicolor/{size}x{size}/apps/arcade-lens.png")));
        }
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
    fn appimage_is_used_only_for_its_own_mounted_executable() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/lens-appimage-test-{}", std::process::id()));
        let mounted = dir.join("mount/usr/bin/arcade-lens");
        let unrelated = dir.join("mount-other/arcade-lens");
        let image = dir.join("ArcadeLens.AppImage");
        fs::create_dir_all(mounted.parent().unwrap()).unwrap();
        fs::create_dir_all(unrelated.parent().unwrap()).unwrap();
        fs::write(&mounted, b"mounted Lens").unwrap();
        fs::write(&unrelated, b"standalone Lens").unwrap();
        fs::write(&image, b"AppImage").unwrap();
        let appdir = dir.join("mount");
        assert_eq!(appimage_for_exe(&mounted, Some(&appdir), Some(&image)), Some(image.clone()));
        assert_eq!(appimage_for_exe(&unrelated, Some(&appdir), Some(&image)), None);
        assert_eq!(appimage_for_exe(&unrelated, None, Some(&image)), None);
        assert_eq!(appimage_for_exe(&mounted, Some(&appdir), None), None);
        let alias = dir.join("mount-alias");
        std::os::unix::fs::symlink("mount", &alias).unwrap();
        assert_eq!(appimage_for_exe(&mounted, Some(&alias), Some(&image)), Some(image.clone()));
        fs::remove_file(&image).unwrap();
        assert_eq!(appimage_for_exe(&mounted, Some(&appdir), Some(&image)), None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn launcher_uses_the_os_id_and_tray_words() {
        let entry = launcher_entry(Path::new("/opt/lens/arcade-lens"));
        for line in [
            "Icon=dev.arcade.Lens",
            "StartupWMClass=dev.arcade.Lens",
            "Comment=Select anything on screen and act on it.",
            "Actions=Capture;Settings;Quit;",
            "Name=Capture",
            "Name=Settings",
            "Name=Quit Arcade Lens",
            "Exec=\"/opt/lens/arcade-lens\" --settings",
        ] {
            assert!(entry.lines().any(|l| l == line), "{line} missing from\n{entry}");
        }
    }

    #[test]
    fn legacy_files_go_only_when_ours_and_not_the_replacement() {
        let dir = std::env::temp_dir().join(format!("lens-legacy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (old, new) = (dir.join("arcade-lens.desktop"), dir.join("dev.arcade.Lens.desktop"));
        fs::write(&new, "Name=Arcade Lens").unwrap();
        // Someone else's file with the old name stays.
        fs::write(&old, "Name=Other").unwrap();
        remove_legacy(&old, &new, "Name=Arcade Lens").unwrap();
        assert!(old.exists());
        // The same file under another name (a case-insensitive volume, here a
        // hard link) is the replacement and stays.
        fs::remove_file(&old).unwrap();
        fs::hard_link(&new, &old).unwrap();
        remove_legacy(&old, &new, "Name=Arcade Lens").unwrap();
        assert!(old.exists() && new.exists());
        // Lens's own old file goes.
        fs::remove_file(&old).unwrap();
        fs::write(&old, "Name=Arcade Lens").unwrap();
        remove_legacy(&old, &new, "Name=Arcade Lens").unwrap();
        assert!(!old.exists() && new.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn exec_quotes_paths() {
        assert_eq!(exec(Path::new("/opt/Arcade Lens/arcade-lens"), "--background"), "\"/opt/Arcade Lens/arcade-lens\" --background");
        // Exec quoting, then Desktop Entry string escaping (both layers).
        assert_eq!(exec(Path::new("/a/$b\"c"), ""), "\"/a/\\\\$b\\\\\"c\"");
        assert_eq!(exec(Path::new("/home/50% off/Lens.AppImage"), ""), "env \"/home/50%% off/Lens.AppImage\"");
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod login_receipt_tests {
    use super::*;
    use arcade_link::install::{self, Environment, UninstallOptions};
    use arcade_link::receipt::{InstallMethod, Integration, ManagedBy, Receipt};
    use std::fs;

    struct Fixture {
        environment: Environment,
        path: PathBuf,
        receipt: Receipt,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("login-receipt-{}", arcade_link::endpoint::new_token().unwrap()));
            let environment = Environment::under(&root);
            let path =
                if cfg!(target_os = "macos") { root.join("Library/LaunchAgents/app.plist") } else { environment.config.join("autostart/arcade-lens.desktop") };
            let receipt = Receipt {
                schema: 1,
                id: arcade_link::ids::LENS.into(),
                version: "0.2.0".into(),
                channel: "nightly".into(),
                method: InstallMethod::Appimage,
                managed_by: ManagedBy::Tools,
                path: environment.applications.join("app.AppImage"),
                integration: Integration { cli: Some(environment.bin.join("app")), ..Default::default() },
                previous: None,
                installed_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
            };
            Self { environment, path, receipt }
        }

        fn set(&self, enabled: bool) -> Result<(), std::io::Error> {
            set_entry(
                &self.environment.receipts(),
                &self.path,
                enabled.then_some(b"[Desktop Entry]\nType=Application\nX-Arcade-Id=arcade.lens\nExec=app\n".as_slice()),
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if self.environment.home.exists() {
                fs::remove_dir_all(&self.environment.home).unwrap();
            }
        }
    }

    #[test]
    fn login_toggle_preserves_receipt_metadata_and_uninstall_removes_the_entry() {
        let fixture = Fixture::new();
        let store = fixture.environment.receipts();
        store.write(&fixture.receipt).unwrap();
        let file = store.path(arcade_link::ids::LENS).unwrap();
        let mut document = serde_json::to_value(&fixture.receipt).unwrap();
        document["futureField"] = serde_json::json!({"keep": true});
        document["integration"]["futureEntry"] = serde_json::json!("keep");
        fs::write(&file, serde_json::to_vec(&document).unwrap()).unwrap();
        fixture.set(true).unwrap();
        assert!(fixture.path.is_file());
        let enabled = store.read(arcade_link::ids::LENS).unwrap().unwrap();
        let mut expected = fixture.receipt.clone();
        expected.integration.autostart = Some(fixture.path.clone());
        expected.updated_at = enabled.updated_at.clone();
        assert_eq!(enabled, expected);
        assert!(enabled.updated_at > fixture.receipt.updated_at);
        fixture.set(false).unwrap();
        fixture.set(false).unwrap();
        assert!(!fixture.path.exists());
        assert!(store.read(arcade_link::ids::LENS).unwrap().unwrap().integration.autostart.is_none());
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
        assert_eq!(saved["futureField"], document["futureField"]);
        assert_eq!(saved["integration"]["futureEntry"], document["integration"]["futureEntry"]);
        fixture.set(true).unwrap();
        assert!(install::uninstall(&fixture.environment, arcade_link::ids::LENS, UninstallOptions { remove_data: false, data_folders: &[] }).unwrap());
        assert!(!fixture.path.exists());
        assert!(store.read(arcade_link::ids::LENS).unwrap().is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn generated_login_entry_carries_ownership_and_quotes_the_executable() {
        let entry = login_entry(Path::new("/home/user/My Apps/Lens.AppImage"));
        assert!(entry.contains("\nX-Arcade-Id=arcade.lens\n"));
        assert!(entry.contains("Exec=\"/home/user/My Apps/Lens.AppImage\" --background\n"));
    }

    #[test]
    fn portable_login_toggle_never_creates_an_install_receipt() {
        let fixture = Fixture::new();
        fixture.set(true).unwrap();
        assert!(fixture.path.is_file());
        assert!(fixture.environment.receipts().read(arcade_link::ids::LENS).unwrap().is_none());
        fixture.set(false).unwrap();
        assert!(!fixture.path.exists());
    }

    #[test]
    fn damaged_receipt_does_not_mutate_login_entries() {
        let fixture = Fixture::new();
        let store = fixture.environment.receipts();
        store.write(&fixture.receipt).unwrap();
        fixture.set(true).unwrap();
        let original = fs::read(&fixture.path).unwrap();
        fs::write(store.path(arcade_link::ids::LENS).unwrap(), b"invalid receipt").unwrap();
        assert!(fixture.set(false).is_err());
        assert_eq!(fs::read(&fixture.path).unwrap(), original);
    }
}
