//! What every data file Lens reads back shares (FS-2, D15): a `schema` number, a file made by a newer
//! Arcade Lens that is read as far as possible and never written over, and a file that can't be parsed
//! that is moved aside instead of failing the start or being replaced by the next save.
//!
//! A file with no `schema` field is version 1, the shape every release so far wrote. [`lens_core::schema`]
//! holds the number and the rule that it is written only above 1.

use std::io;
use std::path::{Path, PathBuf};

use lens_core::schema::CURRENT;
use serde::de::DeserializeOwned;

/// The words a newer file's log line and refusal use.
pub const NEWER: &str = "made by a newer Arcade Lens";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Json,
    Toml,
}

/// The `schema` a JSON document declares; none (or not a number) is 1.
pub fn json_schema(value: &serde_json::Value) -> u32 {
    value.get("schema").and_then(serde_json::Value::as_u64).map_or(1, |n| u32::try_from(n).unwrap_or(u32::MAX))
}

/// The `schema` a TOML document declares; none (or not a number) is 1.
pub fn toml_schema(table: &toml::Table) -> u32 {
    table.get("schema").and_then(toml::Value::as_integer).map_or(1, |n| u32::try_from(n.max(1)).unwrap_or(u32::MAX))
}

/// Moves a file Lens can't read to `<name>.unreadable-<unix seconds>` next to it (`-1`, `-2`... if that name
/// is taken) so no save can destroy it. Returns where it went.
pub fn set_aside(path: &Path) -> Option<PathBuf> {
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let name = path.file_name()?.to_string_lossy().into_owned();
    let mut target = path.with_file_name(format!("{name}.unreadable-{seconds}"));
    let mut n = 1;
    while target.exists() {
        target = path.with_file_name(format!("{name}.unreadable-{seconds}-{n}"));
        n += 1;
    }
    std::fs::rename(path, &target).ok()?;
    Some(target)
}

fn unreadable(path: &Path, why: &str) {
    match set_aside(path) {
        Some(kept) => eprintln!("arcade-lens: can't read {} ({why}); kept it as {} and started with defaults", path.display(), kept.display()),
        None => eprintln!("arcade-lens: can't read {} ({why}); using defaults", path.display()),
    }
}

fn parse_json(bytes: &[u8]) -> Result<serde_json::Value, String> {
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

fn parse_toml(bytes: &[u8]) -> Result<toml::Table, String> {
    std::str::from_utf8(bytes).map_err(|e| e.to_string())?.parse::<toml::Table>().map_err(|e| e.to_string())
}

/// Reads `path` into whatever `map` makes of the parsed document and the schema it declares. `Ok(None)` means
/// "use the defaults": the file is missing, or can't be used. A file that can't be parsed, or that declares
/// schema 1 and doesn't fit `map`, is moved aside. One made by a newer Lens is logged and, if `map` can't
/// use it, left where it is; it is never moved or written. `Err` is only for a file that exists but can't be read.
fn load<D, T>(
    path: &Path,
    parse: fn(&[u8]) -> Result<D, String>,
    schema: fn(&D) -> u32,
    map: impl FnOnce(D, u32) -> Result<T, String>,
) -> io::Result<Option<T>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let doc = match parse(&bytes) {
        Ok(doc) => doc,
        Err(why) => {
            unreadable(path, &why);
            return Ok(None);
        }
    };
    let found = schema(&doc);
    let newer = found > CURRENT;
    if newer {
        eprintln!("arcade-lens: {} was {NEWER} (schema {found}, this build reads {CURRENT}); reading what I can and leaving the file as it is", path.display());
    }
    match map(doc, found) {
        Ok(value) => Ok(Some(value)),
        Err(why) if newer => {
            eprintln!("arcade-lens: couldn't read all of {}: {why}; using defaults without writing", path.display());
            Ok(None)
        }
        Err(why) => {
            unreadable(path, &why);
            Ok(None)
        }
    }
}

/// [`load`] for a JSON file, with `map` doing the reading of the document.
pub fn load_json_with<T>(path: &Path, map: impl FnOnce(serde_json::Value, u32) -> Result<T, String>) -> io::Result<Option<T>> {
    load(path, parse_json, json_schema, map)
}

/// A JSON file that is one serde type.
pub fn load_json<T: DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    load_json_with(path, |value, _| serde_json::from_value(value).map_err(|e| e.to_string()))
}

/// A TOML file that is one serde type.
pub fn load_toml<T: DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    load(path, parse_toml, toml_schema, |table, _| table.try_into().map_err(|e: toml::de::Error| e.to_string()))
}

/// To call before writing `path`. A file made by a newer Lens is refused (`Err`, nothing changes); a file
/// that can't be parsed is moved aside first so the write can't destroy it; anything else is clear to write.
pub fn guard_write(path: &Path, kind: Kind) -> io::Result<()> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let found = match kind {
        Kind::Json => parse_json(&bytes).map(|doc| json_schema(&doc)),
        Kind::Toml => parse_toml(&bytes).map(|doc| toml_schema(&doc)),
    };
    match found {
        Ok(found) if found > CURRENT => {
            let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            Err(io::Error::other(format!("{name} was {NEWER} (schema {found}); this Arcade Lens did not change it")))
        }
        Ok(_) => Ok(()),
        Err(why) => {
            unreadable(path, &why);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::{set_aside as aside, temp};

    #[test]
    fn no_schema_field_is_version_one() {
        assert_eq!(json_schema(&serde_json::json!({})), 1);
        assert_eq!(json_schema(&serde_json::json!([1])), 1);
        assert_eq!(json_schema(&serde_json::json!({"schema": "2"})), 1);
        assert_eq!(json_schema(&serde_json::json!({"schema": 3})), 3);
        assert_eq!(toml_schema(&"a = 1".parse().unwrap()), 1);
        assert_eq!(toml_schema(&"schema = 4".parse().unwrap()), 4);
        assert_eq!(toml_schema(&"schema = -4".parse().unwrap()), 1);
    }

    #[test]
    fn a_name_that_is_taken_gets_a_counter() {
        let dir = temp("aside-counter");
        let file = dir.join("data.json");
        let mut kept = Vec::new();
        for n in 0..3u8 {
            std::fs::write(&file, [n]).unwrap();
            kept.push(set_aside(&file).unwrap());
        }
        assert_eq!(aside(&dir, "data.json").len(), 3);
        for (n, path) in kept.iter().enumerate() {
            assert_eq!(std::fs::read(path).unwrap(), [n as u8]);
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_missing_file_is_not_an_error_and_writes_freely() {
        let dir = temp("missing");
        assert!(load_json::<serde_json::Value>(&dir.join("x.json")).unwrap().is_none());
        guard_write(&dir.join("x.json"), Kind::Json).unwrap();
        std::fs::remove_dir_all(dir).ok();
    }
}
