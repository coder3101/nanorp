//! Locations for persistent app data (SQLite DB, avatars, attachments),
//! rooted at the platform config dir (e.g. `~/.config/nanorp` on Linux).

use anyhow::{Context, Result};
use std::path::PathBuf;

const APP_NAME: &str = "nanorp";
const DB_FILENAME: &str = "nanorp.db";
const AVATARS_DIR: &str = "avatars";
const ATTACHMENTS_DIR: &str = "attachments";

/// Returns the base config directory for the application.
/// Uses `dirs::config_dir()` which resolves to:
///   - Linux:   ~/.config/nanorp/
///   - macOS:   ~/Library/Application Support/nanorp/
///   - Windows: C:\Users\<user>\AppData\Roaming\nanorp\
pub fn config_dir() -> Result<PathBuf> {
    let base = dirs::config_dir().context("Could not determine config directory")?;
    Ok(base.join(APP_NAME))
}

/// Returns the full path to the SQLite database file.
pub fn db_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(DB_FILENAME))
}

/// Returns the path to the avatars directory.
pub fn avatars_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join(AVATARS_DIR))
}

/// Returns the path to a specific avatar file by character ID.
/// Avatar files are stored as `{id}.webp`.
pub fn avatar_path(id: &str) -> Result<PathBuf> {
    Ok(avatars_dir()?.join(format!("{}.webp", id)))
}

/// Returns the path to an avatar file with an explicit extension
/// (e.g. "png", "jpeg", "webp", "gif"). Filename is `{id}.{ext}`.
pub fn avatar_path_ext(id: &str, ext: &str) -> Result<PathBuf> {
    Ok(avatars_dir()?.join(format!("{}.{}", id, ext)))
}

/// Returns the path to the attachments directory.
pub fn attachments_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join(ATTACHMENTS_DIR))
}

/// Returns the path to a specific attachment file.
/// Filename should include extension (e.g. "{uuid}.png").
pub fn attachment_path(filename: &str) -> Result<PathBuf> {
    Ok(attachments_dir()?.join(filename))
}

/// Creates all required directories if they don't exist.
/// Call this once at application startup.
pub fn ensure_dirs() -> Result<()> {
    let dirs_to_create = [config_dir()?, avatars_dir()?, attachments_dir()?];
    for dir in &dirs_to_create {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create directory: {}", dir.display()))?;
    }
    Ok(())
}
