//! Exporting an instance to a zip and importing one back.
//!
//! This is the only backup path the launcher has, and Delete removes worlds
//! permanently, so it exists mainly as the safety net next to that button.
//!
//! The archive holds the instance config plus its whole `.minecraft` folder.
//! Libraries, assets and the client jar are deliberately left out: they live in
//! the shared store, are identical for everyone, and re-downloading them is
//! cheaper than shipping half a gigabyte in every export.

use crate::error::{Error, Result};
use crate::instance::{self, Instance};
use crate::paths;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const CONFIG_NAME: &str = "instance.json";

/// Directories inside an instance that are never worth archiving.
fn is_skipped(relative: &Path) -> bool {
    let first = relative.components().next();
    // logs/ holds the unredacted game log, which contains the session token.
    matches!(first, Some(c) if c.as_os_str() == "natives" || c.as_os_str() == "logs")
}

/// Collect every file under `root`, as (absolute path, path relative to root).
pub(crate) fn collect(root: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|_| Error::msg("path escaped the instance directory"))?
                .to_path_buf();

            if is_skipped(&relative) {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else {
                // A crashed download leaves these behind; they are not content.
                if path.extension().is_some_and(|e| e == "part") {
                    continue;
                }
                out.push((path, relative));
            }
        }
    }
    Ok(out)
}

/// Write the instance to `exports/<id>.zip`, returning the archive path.
pub fn export(instance: &Instance) -> Result<PathBuf> {
    let dir = instance.dir();
    if !dir.join(CONFIG_NAME).exists() {
        return Err(Error::msg("Instance directory is missing its config."));
    }

    let exports = paths::exports();
    std::fs::create_dir_all(&exports)?;
    let dest = exports.join(format!("{}.zip", instance.id));

    zip_dir(&dir, &dest, "")?;
    Ok(dest)
}

/// Zip every file under `root` into `dest`, each entry prefixed with `prefix`
/// so an archive can unpack into a folder of its own. Shared with world
/// backups, which are the same operation on a smaller tree.
pub(crate) fn zip_dir(root: &Path, dest: &Path, prefix: &str) -> Result<()> {
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(dest)?);

    for (path, relative) in collect(root)? {
        // Zip entries always use forward slashes, whatever the host platform.
        let name = format!("{prefix}{}", relative.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"));
        zip.start_file(name, options)?;
        let mut file = std::fs::File::open(&path)?;
        std::io::copy(&mut file, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}

/// Copy an instance into a new one, worlds, configs and mods included.
///
/// Reuses the export filter, so the copy leaves out natives and logs for the
/// same reasons the archive does — both are rebuilt on the next launch.
pub fn duplicate(source: &Instance, name: &str) -> Result<Instance> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::msg("Instance name cannot be empty."));
    }
    let dir = source.dir();
    if !dir.join(CONFIG_NAME).exists() {
        return Err(Error::msg("Instance directory is missing its config."));
    }

    let mut copy = source.clone();
    copy.id = instance::unique_id(name);
    copy.name = name.to_string();
    copy.last_played = 0;
    copy.play_time = 0;
    // Natives were not copied, so the copy needs a verify pass before it runs.
    copy.installed = false;
    let dest = copy.dir();

    for (path, relative) in collect(&dir)? {
        if relative == Path::new(CONFIG_NAME) {
            continue;
        }
        let out = dest.join(&relative);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&path, &out)?;
    }

    std::fs::create_dir_all(copy.game_dir())?;
    // Written last so a half-copied directory is not a listable instance.
    std::fs::write(dest.join(CONFIG_NAME), serde_json::to_vec_pretty(&copy)?)?;
    Ok(copy)
}

/// The prefix inside the archive that contains `instance.json`.
///
/// People zip an instance folder in two shapes: with the config at the root, or
/// nested one level under the folder's own name. Accept both by locating the
/// shallowest `instance.json` and treating its directory as the root.
fn archive_root(names: &[String]) -> Option<String> {
    names
        .iter()
        .filter(|n| n.rsplit('/').next() == Some(CONFIG_NAME))
        .min_by_key(|n| n.matches('/').count())
        .map(|n| n.strip_suffix(CONFIG_NAME).unwrap_or("").to_string())
}

/// Extract an exported archive into a new instance.
pub fn import(archive: &Path) -> Result<Instance> {
    let file = std::fs::File::open(archive)
        .map_err(|e| Error::msg(format!("Cannot open {}: {e}", archive.display())))?;
    let mut zip = zip::ZipArchive::new(file)?;

    let names: Vec<String> = zip.file_names().map(str::to_string).collect();
    let root = archive_root(&names).ok_or_else(|| {
        Error::msg("That zip is not a JustLauncher instance (no instance.json inside).")
    })?;

    let mut config = String::new();
    zip.by_name(&format!("{root}{CONFIG_NAME}"))?
        .read_to_string(&mut config)?;
    let mut imported: Instance = serde_json::from_str(&config)
        .map_err(|e| Error::msg(format!("Instance config in the zip is invalid: {e}")))?;

    // The id is the folder name, so it has to be free on this machine; the
    // original may already exist here.
    imported.id = instance::unique_id(&imported.name);
    // Force a verify pass on first launch: the shared store on this machine may
    // not have the libraries and assets this instance needs.
    imported.installed = false;
    let dest = imported.dir();

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        // `enclosed_name` rejects absolute paths and `..`, so a crafted archive
        // cannot write outside the new instance directory.
        let Some(name) = entry.enclosed_name() else { continue };
        let Ok(relative) = name.strip_prefix(&root) else { continue };
        if entry.is_dir() || relative.as_os_str().is_empty() {
            continue;
        }

        let out = dest.join(relative);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut writer = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut writer)?;
    }

    // Written last so the config on disk carries the new id, not the old one.
    std::fs::create_dir_all(&dest)?;
    let mut config = std::fs::File::create(dest.join(CONFIG_NAME))?;
    config.write_all(&serde_json::to_vec_pretty(&imported)?)?;
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_root_accepts_both_zip_shapes() {
        assert_eq!(
            archive_root(&["instance.json".into(), ".minecraft/options.txt".into()]),
            Some(String::new())
        );
        assert_eq!(
            archive_root(&["my-pack/instance.json".into(), "my-pack/.minecraft/x".into()]),
            Some("my-pack/".into())
        );
        // The shallowest wins, so a mod that ships its own instance.json inside
        // the game folder cannot hijack the root.
        assert_eq!(
            archive_root(&[
                "pack/.minecraft/mods/weird/instance.json".into(),
                "pack/instance.json".into(),
            ]),
            Some("pack/".into())
        );
        assert_eq!(archive_root(&["readme.txt".into()]), None);
    }

    #[test]
    fn natives_and_partial_downloads_are_not_archived() {
        assert!(is_skipped(Path::new("natives/lwjgl.dll")));
        assert!(is_skipped(Path::new("logs/latest.log")));
        assert!(!is_skipped(Path::new(".minecraft/saves/world/level.dat")));
        assert!(!is_skipped(Path::new("instance.json")));
    }
}
