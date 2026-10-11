//! Arcade Lens app-owned Link runtime and installation policy.
//!
//! The shared installer and receipt APIs stay behind this module so callers
//! do not have to depend on Link's install types directly.

pub mod hub;
pub mod installer;
pub mod installer_ui;
pub mod portable_settings;
pub mod trayhost;

pub use installer::{method, Options};
