//! Receipt-backed Startup shortcuts take precedence over the portable Run key.
//! The filesystem policy also runs in tests on Linux; only the OS calls below
//! are Windows-only.

use std::fs;
use std::io;
use std::path::Path;

use arcade_link::receipt::{Receipt, Store};

const ID: &str = arcade_link::ids::LENS;

pub(super) fn enabled(store: &Store, run_enabled: impl FnOnce() -> bool) -> bool {
    match store.read(ID) {
        Ok(Some(receipt)) => receipt.integration.autostart.is_some_and(|p| p.is_absolute() && p.exists()),
        Ok(None) => run_enabled(),
        // A damaged receipt must not silently select the portable backend.
        Err(_) => false,
    }
}

fn remove_file(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// OS operations are injected so receipt selection, migration and rollback
/// are verified without touching the host registry or real Startup folder.
pub(super) fn set(
    store: &Store,
    startup: Option<&Path>,
    exe: &Path,
    on: bool,
    write_shortcut: impl FnOnce(&Path, &Path) -> io::Result<()>,
    set_run: impl FnOnce(bool, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let Some(receipt) = store.read(ID)? else {
        // Clean up a leftover installer shortcut before using the Run key.
        if let Some(path) = startup {
            remove_file(path)?;
        }
        return set_run(on, exe);
    };
    receipt.validate_native_paths()?;
    // Read the original JSON as well: the Windows installer records extra
    // fields (such as desktopShortcut) which must survive an in-app toggle.
    let mut document: serde_json::Value = serde_json::from_slice(&fs::read(store.path(ID)?)?).map_err(io::Error::other)?;
    let path = receipt.integration.autostart.as_deref().or(startup);
    if on && path.is_none() {
        return Err(io::Error::other("the Windows Startup folder is unavailable"));
    }
    let backup = match path.map(fs::read).transpose() {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    // Delete the historical Run value before creating a shortcut, so the
    // installed and portable mechanisms never both launch Lens at login.
    set_run(false, exe)?;
    let change = || {
        if let Some(path) = path {
            if on {
                write_shortcut(path, &receipt.path)?;
            } else {
                remove_file(path)?;
            }
        }
        document["integration"]["autostart"] = if on { serde_json::to_value(path).map_err(io::Error::other)? } else { serde_json::Value::Null };
        document["updatedAt"] = arcade_link::manifest::now_rfc3339().into();
        let bytes = serde_json::to_vec_pretty(&document).map_err(io::Error::other)?;
        Receipt::from_json(std::str::from_utf8(&bytes).map_err(io::Error::other)?)?.validate_native_paths()?;
        arcade_link::paths::write_atomic(&store.path(ID)?, &bytes, true)
    };
    if let Err(error) = change() {
        if let Some(path) = path {
            let restore = match backup {
                Some(bytes) => arcade_link::paths::write_atomic(path, &bytes, false),
                None => remove_file(path),
            };
            if let Err(restore) = restore {
                return Err(io::Error::other(format!("{error}; restoring the login shortcut failed: {restore}")));
            }
        }
        return Err(error);
    }
    Ok(())
}

fn shortcut_script(path: &Path, executable: &Path) -> String {
    let quote = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', "''"));
    format!(
        "$ErrorActionPreference='Stop'; $s=(New-Object -ComObject WScript.Shell).CreateShortcut({}); $s.TargetPath={}; $s.Arguments='--background'; $s.WorkingDirectory={}; $s.Save()",
        quote(path),
        quote(executable),
        quote(executable.parent().unwrap_or(executable)),
    )
}

#[cfg(windows)]
pub(super) fn set_current(exe: &Path, on: bool) -> io::Result<()> {
    let environment = arcade_link::install::Environment::discover()?;
    let startup = std::env::var_os("APPDATA").map(std::path::PathBuf::from).map(|p| p.join("Microsoft/Windows/Start Menu/Programs/Startup/Arcade Lens.lnk"));
    set(&environment.receipts(), startup.as_deref(), exe, on, write_shortcut, set_run)
}

#[cfg(windows)]
fn write_shortcut(path: &Path, executable: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let output =
        std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &shortcut_script(path, executable)]).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("could not create the Windows login shortcut: {}", String::from_utf8_lossy(&output.stderr).trim())))
    }
}

#[cfg(windows)]
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
#[cfg(windows)]
const RUN_NAME: &str = "ArcadeLens";

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
pub(super) fn run_enabled() -> bool {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let (key, name) = (wide(RUN_KEY), wide(RUN_NAME));
    let mut size = 0;
    // SAFETY: NUL-terminated strings; querying the size without a data buffer.
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), std::ptr::null_mut(), &mut size) == ERROR_SUCCESS
    }
}

#[cfg(windows)]
fn set_run(on: bool, exe: &Path) -> io::Result<()> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, HKEY_CURRENT_USER};
    if on {
        let output = std::process::Command::new("reg")
            .args(["add", &format!("HKCU\\{RUN_KEY}"), "/v", RUN_NAME, "/t", "REG_SZ", "/d", &format!("\"{}\" --background", exe.display()), "/f"])
            .output()?;
        return if output.status.success() { Ok(()) } else { Err(io::Error::other("reg add failed")) };
    }
    let (key, name) = (wide(RUN_KEY), wide(RUN_NAME));
    // SAFETY: NUL-terminated strings; only Lens's historical value is deleted.
    let result = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
    match result {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(()),
        error => Err(io::Error::from_raw_os_error(error as i32)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcade_link::receipt::{InstallMethod, Integration, ManagedBy};
    use std::cell::Cell;
    use std::path::PathBuf;

    struct Fixture {
        root: PathBuf,
        store: Store,
        startup: PathBuf,
        receipt: Receipt,
        run: Cell<bool>,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("lens-windows-login-{}", arcade_link::endpoint::new_token().unwrap()));
            fs::create_dir_all(&root).unwrap();
            let store = Store::new(&arcade_link::Locations::under(&root.join("arcade")));
            let startup = root.join("Startup/Arcade Lens.lnk");
            let receipt = Receipt {
                schema: 1,
                id: ID.into(),
                version: "0.2.0".into(),
                channel: "stable".into(),
                method: InstallMethod::WindowsInstaller,
                managed_by: ManagedBy::Tools,
                path: root.join("Arcade Lens/arcade-lens.exe"),
                integration: Integration::default(),
                previous: None,
                installed_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
            };
            Self { root, store, startup, receipt, run: Cell::new(false) }
        }

        fn set(&self, on: bool) -> io::Result<()> {
            set(
                &self.store,
                Some(&self.startup),
                &self.root.join("portable.exe"),
                on,
                |path, exe| {
                    assert!(!self.run.get(), "Run value must be removed before creating the shortcut");
                    fs::create_dir_all(path.parent().unwrap())?;
                    fs::write(path, exe.to_string_lossy().as_bytes())
                },
                |on, _| {
                    self.run.set(on);
                    Ok(())
                },
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn receipt_controls_status_even_with_a_legacy_run_value() {
        let mut f = Fixture::new();
        f.run.set(true);
        assert!(enabled(&f.store, || f.run.get()));
        f.store.write(&f.receipt).unwrap();
        assert!(!enabled(&f.store, || panic!("receipt must take precedence over Run")));
        f.receipt.integration.autostart = Some(f.startup.clone());
        f.store.write(&f.receipt).unwrap();
        assert!(!enabled(&f.store, || true), "a missing recorded shortcut is disabled");
        fs::create_dir_all(f.startup.parent().unwrap()).unwrap();
        fs::write(&f.startup, b"shortcut").unwrap();
        assert!(enabled(&f.store, || false));
        let environment = arcade_link::install::Environment::under(&f.root.join("arcade"));
        let status = arcade_link::install::integration_status(&environment, ID, "").unwrap();
        assert!(status.entries.iter().any(|entry| entry.path == f.startup && entry.exists));
    }

    #[test]
    fn toggle_round_trip_keeps_receipt_and_shortcut_in_sync() {
        let f = Fixture::new();
        f.store.write(&f.receipt).unwrap();
        let mut document = serde_json::to_value(&f.receipt).unwrap();
        document["integration"]["desktopShortcut"] = f.root.join("Desktop/Arcade Lens.lnk").to_string_lossy().as_ref().into();
        fs::write(f.store.path(ID).unwrap(), serde_json::to_vec(&document).unwrap()).unwrap();
        f.run.set(true);
        f.set(true).unwrap();
        assert!(!f.run.get());
        assert_eq!(fs::read_to_string(&f.startup).unwrap(), f.receipt.path.to_string_lossy());
        let installed = f.store.read(ID).unwrap().unwrap();
        assert_eq!(installed.integration.autostart, Some(f.startup.clone()));
        assert_eq!(installed.managed_by, ManagedBy::Tools);
        assert_eq!(installed.installed_at, f.receipt.installed_at);
        assert!(installed.updated_at > f.receipt.updated_at);
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(f.store.path(ID).unwrap()).unwrap()).unwrap();
        assert_eq!(saved["integration"]["desktopShortcut"], document["integration"]["desktopShortcut"]);
        f.set(false).unwrap();
        assert!(!f.startup.exists());
        assert!(!f.run.get());
        assert!(f.store.read(ID).unwrap().unwrap().integration.autostart.is_none());
        assert!(!enabled(&f.store, || true));
        f.set(true).unwrap();
        assert!(f.startup.is_file());
        assert!(enabled(&f.store, || false));
        f.set(false).unwrap();
        f.set(false).unwrap();
    }

    #[test]
    fn recorded_shortcut_is_used_without_appdata() {
        let mut f = Fixture::new();
        let recorded = f.root.join("Redirected Startup/Lens.lnk");
        f.receipt.integration.autostart = Some(recorded.clone());
        f.store.write(&f.receipt).unwrap();
        f.set(true).unwrap();
        assert!(recorded.is_file());
        assert!(!f.startup.exists());
        set(&f.store, None, &f.receipt.path, false, |_, _| panic!("disable must not create a shortcut"), |_, _| Ok(())).unwrap();
        assert!(!recorded.exists());
        assert!(f.store.read(ID).unwrap().unwrap().integration.autostart.is_none());
    }

    #[test]
    fn portable_toggle_removes_orphan_shortcut_and_never_creates_receipt() {
        let f = Fixture::new();
        fs::create_dir_all(f.startup.parent().unwrap()).unwrap();
        fs::write(&f.startup, b"leftover shortcut").unwrap();
        f.set(true).unwrap();
        assert!(f.run.get());
        assert!(!f.startup.exists());
        assert!(f.store.read(ID).unwrap().is_none());
        f.set(false).unwrap();
        assert!(!f.run.get());
    }

    #[test]
    fn malformed_receipt_never_falls_back_to_run_key() {
        let f = Fixture::new();
        f.store.write(&f.receipt).unwrap();
        fs::write(f.store.path(ID).unwrap(), b"not a receipt").unwrap();
        f.run.set(true);
        assert!(!enabled(&f.store, || panic!("invalid receipts must not select Run")));
        assert!(f.set(true).is_err());
        assert!(f.run.get(), "failed receipt read must not mutate integration");
        assert!(!f.startup.exists());
    }

    #[test]
    fn failed_shortcut_write_restores_previous_file_and_receipt() {
        let mut f = Fixture::new();
        f.receipt.integration.autostart = Some(f.startup.clone());
        f.store.write(&f.receipt).unwrap();
        fs::create_dir_all(f.startup.parent().unwrap()).unwrap();
        fs::write(&f.startup, b"original shortcut").unwrap();
        let result = set(
            &f.store,
            None,
            &f.receipt.path,
            true,
            |path, _| {
                fs::write(path, b"partial shortcut")?;
                Err(io::Error::other("shortcut creation failed"))
            },
            |_, _| Ok(()),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&f.startup).unwrap(), b"original shortcut");
        assert_eq!(f.store.read(ID).unwrap(), Some(f.receipt.clone()));
    }

    #[test]
    fn registry_failure_cannot_create_a_second_login_entry() {
        let f = Fixture::new();
        f.store.write(&f.receipt).unwrap();
        assert!(set(
            &f.store,
            Some(&f.startup),
            &f.receipt.path,
            true,
            |_, _| panic!("registry cleanup failed"),
            |_, _| Err(io::Error::other("access denied"))
        )
        .is_err());
        assert!(!f.startup.exists());
        assert_eq!(f.store.read(ID).unwrap(), Some(f.receipt.clone()));
    }

    #[test]
    fn powershell_shortcut_paths_are_literal_and_start_in_background() {
        let script = shortcut_script(Path::new("C:/User's Startup/Lens.lnk"), Path::new("C:/User's Apps/arcade-lens.exe"));
        assert!(script.contains("CreateShortcut('C:/User''s Startup/Lens.lnk')"));
        assert!(script.contains("$s.TargetPath='C:/User''s Apps/arcade-lens.exe'"));
        assert!(script.contains("$s.WorkingDirectory='C:/User''s Apps'"));
        assert!(script.contains("$s.Arguments='--background'"));
    }
}
