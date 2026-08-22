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

/// The name and last-played time out of `level.dat`.
///
/// Gzipped NBT, parsed properly now that `nbt` exists for `servers.dat`. The
/// file's own `LastPlayed` is what the game shows, and unlike the file's
/// modification time it survives a copy, a duplicate or a restored backup.
///
/// Anything unreadable is `None` on both counts, and the caller falls back to
/// the folder name and the file's mtime.
fn level_info(bytes: &[u8]) -> (Option<String>, Option<u64>) {
    use std::io::Read;
    let mut plain = Vec::new();
    if flate2::read::GzDecoder::new(bytes).read_to_end(&mut plain).is_err() {
        return (None, None);
    }
    let Ok((_, root)) = crate::nbt::read(&plain) else { return (None, None) };
    let Some(data) = root.get("Data") else { return (None, None) };

    let name = data
        .get("LevelName")
        .and_then(crate::nbt::Tag::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    // Milliseconds in the file, seconds everywhere in this app.
    let last_played = match data.get("LastPlayed") {
        Some(crate::nbt::Tag::Long(ms)) if *ms > 0 => Some(*ms as u64 / 1000),
        _ => None,
    };
    (name, last_played)
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
        let (name, played) = match std::fs::read(&level) {
            Ok(bytes) => level_info(&bytes),
            Err(_) => (None, None),
        };
        let last_played = played.unwrap_or_else(|| {
            level
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0)
        });

        out.push(World {
            name: name.unwrap_or_else(|| folder.clone()),
            size: tree_size(&path),
            last_played,
            folder,
        });
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
    use crate::nbt::Tag;

    /// A `level.dat` as the game writes one: gzipped NBT with the fields
    /// under `Data`.
    fn level_dat(name: &str, last_played: i64) -> Vec<u8> {
        use std::io::Write;
        let root = Tag::Compound(vec![(
            "Data".to_string(),
            Tag::Compound(vec![
                ("LevelName".to_string(), Tag::String(name.into())),
                ("LastPlayed".to_string(), Tag::Long(last_played)),
                ("GameType".to_string(), Tag::Int(0)),
            ]),
        )]);
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&crate::nbt::write("", &root)).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn reads_the_name_and_time_out_of_level_dat() {
        let (name, played) = level_info(&level_dat("Test World", 1_700_000_000_000));
        assert_eq!(name.as_deref(), Some("Test World"));
        // Milliseconds in the file, seconds out of it.
        assert_eq!(played, Some(1_700_000_000));
    }

    #[test]
    fn nothing_readable_falls_back_rather_than_failing() {
        // An empty name is no name; the caller uses the folder instead.
        assert_eq!(level_info(&level_dat("", 0)), (None, None));
        assert_eq!(level_info(b"not gzip at all"), (None, None));

        // Truncated at every length: never a panic, never a wrong answer.
        let bytes = level_dat("Test World", 1);
        for cut in 0..bytes.len() {
            assert_eq!(level_info(&bytes[..cut]).0, None, "truncated to {cut}");
        }
    }
}
