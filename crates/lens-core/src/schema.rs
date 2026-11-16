//! File format versions of Lens's data files (FS-2, D15).
//!
//! Every release so far wrote version 1, which has no `schema` field. A field is written only once
//! it is above 1, so a v1 file stays exactly what 0.4.x wrote and reads. A file whose number is above
//! [`CURRENT`] was made by a newer Arcade Lens: it is read as far as possible and never written over.

/// The newest file format this build reads and writes.
pub const CURRENT: u32 = 1;

/// `skip_serializing_if` for a `schema` field: version 1 has no field.
pub fn is_v1(schema: &u32) -> bool {
    *schema <= 1
}

/// `default` for a `schema` field: no field means 1.
pub fn v1() -> u32 {
    1
}
