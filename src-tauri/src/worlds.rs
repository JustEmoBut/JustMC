//! The saves folder of an instance: what is in it, backing one up, deleting one.
//!
//! Deliberately narrow. The launcher does not create, rename or edit worlds —
//! the game does that far better. What it has that the game does not is a
//! backup button next to a delete button, which is the whole point: Delete
//! here is permanent and `.minecraft/saves` is the only thing in an instance a
//! user cannot re-download.
//!
//! The folder is the database, as everywhere else: `list` reads the directory
//! and nothing caches it.

use crate::error::{Error, Result};
use crate::instance;
use crate::mods::checked_name;
use crate::{pack, paths};
use serde::Serialize;
use std::path::PathBuf;

/// Every save directory holds one; it is what tells a world apart from a
/// stray folder someone dropped in `saves`.
const LEVEL_DAT: &str = "level.dat";

#[derive(Serialize)]
pub struct World {
    /// The directory name, which is also the handle every command takes.
    pub folder: String,
    /// What the world calls itself in game, when `level.dat` says.
    pub name: String,
    /// Bytes on disk, summed over the tree — the number that decides whether
    /// a backup is worth waiting for.
    pub size: u64,
    /// Unix seconds, from `level.dat`: the game rewrites it on every save.
    pub last_played: u64,
}

async fn saves(id: &str) -> Result<PathBuf> {
    Ok(instance::get(id).await?.game_dir().join("saves"))
}

/// The world's in-game name out of `level.dat`.
///
/// `level.dat` is gzipped NBT, and the one string wanted here sits in a
/// predictable shape: the tag name, then a big-endian u16 length, then UTF-8.
/// Scanning for it beats carrying an NBT parser for a single field.
///
/// ponytail: byte scan, not a parser. A world whose name does not survive this
/// falls back to the folder name, which is what it was created from anyway.
fn level_name(bytes: &[u8]) -> Option<String> {
    let at = bytes.windows(9).position(|w| w == b"LevelName")? + 9;
    let len = u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]) as usize;
    let text = bytes.get(at + 2..at + 2 + len)?;
    let name = String::from_utf8_lossy(text).trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Total bytes and newest modification time under a directory.
fn tree_size(dir: &std::path::Path) -> u64 {
    pack::collect(dir)
        .map(|files| files.iter().filter_map(|(p, _)| p.metadata().ok()).map(|m| m.len()).sum())
        .unwrap_or(0)
}

/// Every world of an instance, most recently played first.
pub async fn list(id: &str) -> Result<Vec<World>> {
    let dir = saves(id).await?;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new()); // no saves folder yet is not an error
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let level = path.join(LEVEL_DAT);
        if !level.is_file() {
            continue;
        }
        let folder = entry.file_name().to_string_lossy().into_owned();
        let last_played = level
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let name = std::fs::read(&level)
            .ok()
            .and_then(|bytes| {
                use std::io::Read;
                let mut out = Vec::new();
                flate2::read::GzDecoder::new(&bytes[..]).read_to_end(&mut out).ok()?;
                level_name(&out)
            })
            .unwrap_or_else(|| folder.clone());

        out.push(World { name, size: tree_size(&path), last_played, folder });
    }
    out.sort_by(|a, b| b.last_played.cmp(&a.last_played));
    Ok(out)
}

/// The directory of one world, with the name checked before it is joined.
async fn world_dir(id: &str, folder: &str) -> Result<PathBuf> {
    let dir = saves(id).await?.join(checked_name(folder)?);
    if !dir.join(LEVEL_DAT).is_file() {
        return Err(Error::msg("That is not a world folder."));
    }
    Ok(dir)
}

/// Zip a world into `exports/`, returning the archive path.
///
/// Backups go where exported instances go, so there is one folder a user has
/// to know about, and the entries are prefixed with the world folder so
/// unpacking straight into `saves` restores it.
pub async fn backup(id: &str, folder: &str) -> Result<PathBuf> {
    let dir = world_dir(id, folder).await?;
    let exports = paths::exports();
    std::fs::create_dir_all(&exports)?;

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dest = exports.join(format!("{id}-{folder}-{stamp}.zip"));
    pack::zip_dir(&dir, &dest, &format!("{folder}/"))?;
    Ok(dest)
}

/// Delete a world, permanently. The UI asks first; this does not.
pub async fn delete(id: &str, folder: &str) -> Result<()> {
    let dir = world_dir(id, folder).await?;
    tokio::fs::remove_dir_all(dir).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fragment of real `level.dat` shape: the tag name, a big-endian
    /// length, then the text.
    fn nbt(name: &str) -> Vec<u8> {
        let mut out = b"\x00\x08Data\x08".to_vec();
        out.extend_from_slice(b"LevelName");
        out.extend_from_slice(&(name.len() as u16).to_be_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(b"\x0a\x09GameRules");
        out
    }

    #[test]
    fn reads_the_name_out_of_level_dat() {
        assert_eq!(level_name(&nbt("Test World")).as_deref(), Some("Test World"));
        assert_eq!(level_name(&nbt("Ev")).as_deref(), Some("Ev"));
        // A folder with no name tag, or an empty one, falls back to the folder.
        assert_eq!(level_name(b"nothing here"), None);
        assert_eq!(level_name(&nbt("")), None);
    }

    #[test]
    fn a_truncated_name_does_not_panic() {
        let mut bytes = nbt("Test World");
        bytes.truncate(bytes.len() - 14);
        assert_eq!(level_name(&bytes), None);
    }
}
