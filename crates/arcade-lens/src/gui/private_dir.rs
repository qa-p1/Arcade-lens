//! The one private folder Lens uses to hand files to other processes: the
//! Wayland window hand-over and the images given to KDE Connect.

use std::path::{Path, PathBuf};

/// The folder, created if needed and verified private to this user.
pub fn private_dir() -> Result<PathBuf, String> {
    private_dir_under(std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).as_deref(), &std::env::temp_dir())
}

/// [`private_dir`] with the roots passed in (for tests).
///
/// On Unix: `<runtime>/arcade-lens` when `runtime` is a folder owned by this
/// user, else `<temp>/arcade-lens-<uid>`. It is created with mode 0700 and
/// then checked: a real folder (not a symlink), owned by this user, with no
/// group or other access. Anything else is refused, never written into.
pub fn private_dir_under(runtime: Option<&Path>, temp: &Path) -> Result<PathBuf, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        // SAFETY: getuid has no preconditions and cannot fail.
        let uid = unsafe { libc::getuid() };
        let runtime = runtime.filter(|r| std::fs::metadata(r).is_ok_and(|m| m.is_dir() && m.uid() == uid));
        let dir = match runtime {
            Some(r) => r.join("arcade-lens"),
            None => temp.join(format!("arcade-lens-{uid}")),
        };
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("{}: {e}", dir.display())),
        }
        let m = std::fs::symlink_metadata(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        if !m.file_type().is_dir() {
            return Err(format!("{} is not a real folder", dir.display()));
        }
        if m.uid() != uid {
            return Err(format!("{} belongs to another user", dir.display()));
        }
        if m.mode() & 0o077 != 0 {
            return Err(format!("{} is not private (mode {:o}); remove it or fix it with chmod 700", dir.display(), m.mode() & 0o777));
        }
        Ok(dir)
    }
    #[cfg(not(unix))]
    {
        // The temporary folder is already per user on Windows.
        let _ = runtime;
        let dir = temp.join("arcade-lens");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(dir)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lens-private-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn mode(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn the_folder_is_created_private_and_per_user() {
        let temp = root("create");
        let dir = private_dir_under(None, &temp).unwrap();
        assert_eq!(mode(&dir), 0o700);
        let uid = unsafe { libc::getuid() };
        assert_eq!(dir.file_name().unwrap().to_string_lossy(), format!("arcade-lens-{uid}"));
        assert_eq!(private_dir_under(None, &temp).unwrap(), dir, "opening it again works");
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn the_runtime_folder_is_preferred_when_it_is_ours() {
        let temp = root("rt-temp");
        let runtime = root("rt-run");
        let dir = private_dir_under(Some(&runtime), &temp).unwrap();
        assert_eq!(dir, runtime.join("arcade-lens"));
        assert_eq!(mode(&dir), 0o700);
        // A runtime folder that does not exist is ignored.
        let dir = private_dir_under(Some(&runtime.join("missing")), &temp).unwrap();
        assert!(dir.starts_with(&temp));
        let _ = std::fs::remove_dir_all(&temp);
        let _ = std::fs::remove_dir_all(&runtime);
    }

    #[test]
    fn a_folder_open_to_group_or_other_is_refused() {
        for bits in [0o750, 0o705, 0o755, 0o770] {
            let temp = root("open");
            let dir = private_dir_under(None, &temp).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(bits)).unwrap();
            let err = private_dir_under(None, &temp).unwrap_err();
            assert!(err.contains("private"), "{bits:o}: {err}");
            let _ = std::fs::remove_dir_all(&temp);
        }
    }

    #[test]
    fn a_symlink_in_place_of_the_folder_is_refused() {
        let temp = root("link");
        let elsewhere = root("link-target");
        std::fs::set_permissions(&elsewhere, std::fs::Permissions::from_mode(0o700)).unwrap();
        let uid = unsafe { libc::getuid() };
        symlink(&elsewhere, temp.join(format!("arcade-lens-{uid}"))).unwrap();
        let err = private_dir_under(None, &temp).unwrap_err();
        assert!(err.contains("not a real folder"), "{err}");
        let _ = std::fs::remove_dir_all(&temp);
        let _ = std::fs::remove_dir_all(&elsewhere);
    }
}
