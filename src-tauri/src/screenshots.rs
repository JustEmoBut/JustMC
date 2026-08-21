//! `.minecraft/screenshots`, which is the one folder in an instance a user
//! wants to *look* at rather than manage.
//!
//! The images are served straight to the webview over Tauri's `asset:`
//! protocol instead of being pushed across IPC: a screenshot is a couple of
//! megabytes and a grid of them would be a hundred, base64-encoded, per open.
//! The protocol's scope is empty in the config on purpose — nothing is
//! readable until `list` allows the folder it just read, and the folder is
//! under whatever `JUSTLAUNCHER_HOME` points at, which no static glob can
//! describe.

use crate::error::{Error, Result};
use crate::instance;
use crate::mods::checked_name;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// What the game writes. Anything else in the folder is not ours to show.
const EXTENSION: &str = ".png";

#[derive(Serialize)]
pub struct Screenshot {
    pub file: String,
    /// Absolute path, for `convertFileSrc` on the frontend.
    pub path: String,
    pub size: u64,
    /// Unix seconds; the game names files by date but the mtime is exact.
    pub taken: u64,
}

async fn folder(id: &str) -> Result<PathBuf> {
    Ok(instance::get(id).await?.game_dir().join("screenshots"))
}

/// Every screenshot of an instance, newest first, with the folder allowed on
/// the asset protocol so the webview may render what was just listed.
pub async fn list(app: &AppHandle, id: &str) -> Result<Vec<Screenshot>> {
    let dir = folder(id).await?;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new()); // nothing taken yet is not an error
    };
    app.asset_protocol_scope()
        .allow_directory(&dir, false)
        .map_err(|e| Error::msg(format!("Could not show the screenshots folder: {e}")))?;

    let mut out: Vec<Screenshot> = entries
        .flatten()
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            if !file.to_lowercase().ends_with(EXTENSION) {
                return None;
            }
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let taken = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            Some(Screenshot {
                path: entry.path().to_string_lossy().into_owned(),
                file,
                size: meta.len(),
                taken,
            })
        })
        .collect();

    out.sort_by(|a, b| b.taken.cmp(&a.taken));
    Ok(out)
}

/// Delete one. The name crosses IPC, so it is checked before it is joined.
pub async fn delete(id: &str, file: &str) -> Result<()> {
    if !file.to_lowercase().ends_with(EXTENSION) {
        return Err(Error::msg("Not a screenshot."));
    }
    let path = folder(id).await?.join(checked_name(file)?);
    tokio::fs::remove_file(path).await?;
    Ok(())
}
