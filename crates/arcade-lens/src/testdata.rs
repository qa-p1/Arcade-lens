//! Test-only helpers for the data-file tests (FS-2): a private folder per test and the files
//! Lens 0.4.2 wrote (`tests/data/v0.4.2/`). Nothing here touches the real config folders.

use std::path::{Path, PathBuf};

/// An empty folder under the system temp folder, private to this test and process.
pub fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lens-schema-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A file Lens 0.4.2 wrote, by its path under `tests/data/v0.4.2/`.
pub fn fixture(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/v0.4.2").join(relative)
}

/// Copies fixture `relative` to `to` and returns the bytes.
pub fn install(relative: &str, to: &Path) -> Vec<u8> {
    let bytes = std::fs::read(fixture(relative)).unwrap();
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::write(to, &bytes).unwrap();
    bytes
}

/// The `<name>.unreadable-<seconds>` files next to `dir/name`.
pub fn set_aside(dir: &Path, name: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(&format!("{name}.unreadable-")))
        .collect();
    found.sort();
    found
}
