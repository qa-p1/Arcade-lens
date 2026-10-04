//! Windows: embeds the app icon and version information in the executable,
//! so Explorer, the taskbar and the installer show them.

use std::path::PathBuf;

fn main() {
    let icon = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../packaging/icons/arcade-lens.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts = version.split(['.', '-', '+']).map(|p| p.parse::<u16>().unwrap_or(0));
    let [major, minor, patch] = [parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0)];
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Arcade"
      VALUE "FileDescription", "Arcade Lens"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "arcade-lens"
      VALUE "OriginalFilename", "arcade-lens.exe"
      VALUE "ProductName", "Arcade Lens"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "\\\\"),
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("arcade-lens.rc");
    std::fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE).manifest_optional().unwrap();
}
