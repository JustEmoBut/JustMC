//! The `mods/` folder of a Fabric instance: what is in it, and adding to it.
//!
//! Deliberately thin. The folder is the source of truth, exactly like the
//! instance list — no index, no database. A mod is disabled by appending
//! `.disabled` to its file name, which is the convention Minecraft launchers
//! have shared for a decade and what the user's other tools expect.

use crate::download::{self, Job};
use crate::error::{Error, Result};
use crate::instance;
use serde::Serialize;
use std::path::PathBuf;
use tauri::AppHandle;

const DISABLED: &str = ".disabled";

#[derive(Serialize, Clone)]
pub struct ModFile {
    /// File name as it sits on disk, `.disabled` suffix included.
    pub file: String,
    /// Mod name from `fabric.mod.json`, or the file name when it has none.
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub size: u64,
    /// SHA-1 of the jar, which is what Modrinth matches files by.
    pub sha1: String,
}

/// A newer build Modrinth has for a jar already in the folder.
#[derive(Serialize, Clone)]
pub struct ModUpdate {
    /// The file that would be replaced.
    pub file: String,
    pub name: String,
    /// The version installed now, so the user sees what they are moving from.
    pub current_version: String,
    pub new_version: String,
    pub version_id: String,
    /// Release notes for the new build, markdown, often absent.
    pub changelog: Option<String>,
}

async fn mods_dir(id: &str) -> Result<PathBuf> {
    Ok(instance::get(id).await?.game_dir().join("mods"))
}

/// Reject anything that is not a plain file name: these come from the frontend
/// and are joined onto a path.
fn checked_name(file: &str) -> Result<&str> {
    let bad = file.is_empty()
        || file.contains(['/', '\\'])
        || file.contains("..")
        || std::path::Path::new(file).is_absolute();
    if bad {
        return Err(Error::msg("Invalid mod file name."));
    }
    Ok(file)
}

fn sha1_of(path: &std::path::Path) -> Option<String> {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(std::fs::read(path).ok()?);
    Some(hex::encode(hasher.finalize()))
}

/// Name and version out of the jar's `fabric.mod.json`, if it has one.
fn read_metadata(path: &std::path::Path) -> Option<(String, String)> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let entry = zip.by_name("fabric.mod.json").ok()?;
    let json: serde_json::Value = serde_json::from_reader(entry).ok()?;
    let name = json.get("name").and_then(|v| v.as_str())?.to_string();
    let version = json
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    Some((name, version))
}

/// Every jar in the instance's mods folder, enabled or not, by name.
pub async fn list(id: &str) -> Result<Vec<ModFile>> {
    let dir = mods_dir(id).await?;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new()); // no mods folder yet is not an error
    };

    let mut mods: Vec<ModFile> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let enabled = file.ends_with(".jar");
            if !enabled && !file.ends_with(".jar.disabled") {
                return None;
            }
            let (name, version) = read_metadata(&e.path())
                .unwrap_or_else(|| (file.trim_end_matches(DISABLED).to_string(), String::new()));
            Some(ModFile {
                file,
                name,
                version,
                enabled,
                size: e.metadata().map(|m| m.len()).unwrap_or(0),
                sha1: sha1_of(&e.path()).unwrap_or_default(),
            })
        })
        .collect();

    mods.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(mods)
}

/// Toggle a mod by renaming it. Returns the new file name.
pub async fn set_enabled(id: &str, file: &str, enabled: bool) -> Result<String> {
    let dir = mods_dir(id).await?;
    let from = dir.join(checked_name(file)?);
    let target = match (enabled, file.ends_with(DISABLED)) {
        (true, true) => file.trim_end_matches(DISABLED).to_string(),
        (false, false) => format!("{file}{DISABLED}"),
        // Already in the requested state; renaming onto itself would be a no-op
        // at best and an error on some filesystems.
        _ => return Ok(file.to_string()),
    };
    tokio::fs::rename(from, dir.join(&target)).await?;
    Ok(target)
}

pub async fn delete(id: &str, file: &str) -> Result<()> {
    let path = mods_dir(id).await?.join(checked_name(file)?);
    tokio::fs::remove_file(path).await?;
    Ok(())
}

/// Fetch a mod jar into the instance's mods folder.
pub async fn fetch(
    app: &AppHandle,
    id: &str,
    url: &str,
    file: &str,
    sha1: Option<String>,
    size: Option<u64>,
) -> Result<()> {
    let dir = mods_dir(id).await?;
    tokio::fs::create_dir_all(&dir).await?;
    let job = Job {
        url: url.to_string(),
        path: dir.join(checked_name(file)?),
        sha1,
        size,
    };
    download::run(app, "Mod", vec![job]).await
}

/// Copy a jar the user dropped on the window into the mods folder.
pub async fn add_file(id: &str, source: &std::path::Path) -> Result<String> {
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| Error::msg("That is not a file."))?;
    if !name.to_lowercase().ends_with(".jar") {
        return Err(Error::msg("Only .jar files belong in the mods folder."));
    }
    let dir = mods_dir(id).await?;
    tokio::fs::create_dir_all(&dir).await?;
    tokio::fs::copy(source, dir.join(checked_name(&name)?)).await?;
    Ok(name)
}

/// The Modrinth project ids of the jars already in this instance's folder, so
/// the browser can mark them installed. Jars from anywhere else are simply not
/// in the answer.
pub async fn installed_projects(id: &str) -> Result<Vec<String>> {
    let hashes: Vec<String> = list(id)
        .await?
        .into_iter()
        .map(|m| m.sha1)
        .filter(|h| !h.is_empty())
        .collect();
    let known = crate::modrinth::version_files(&hashes).await?;
    let mut projects: Vec<String> = known.into_values().map(|v| v.project_id).collect();
    projects.sort();
    projects.dedup();
    Ok(projects)
}

/// Delete jars belonging to `project`, except `keep`. Installing a second
/// build of a mod that is already there leaves two jars in the folder, and
/// Fabric refuses to start with duplicates -- so the old one goes before the
/// new one lands.
pub async fn remove_other_versions(id: &str, project: &str, keep: &str) -> Result<()> {
    let installed = list(id).await?;
    let hashes: Vec<String> = installed
        .iter()
        .map(|m| m.sha1.clone())
        .filter(|h| !h.is_empty())
        .collect();
    let known = crate::modrinth::version_files(&hashes).await?;

    for m in installed {
        let same_project = known.get(&m.sha1).is_some_and(|v| v.project_id == project);
        // Compare against the enabled name: a disabled jar of the same mod is
        // still a duplicate once the new one is enabled.
        if same_project && m.file.trim_end_matches(DISABLED) != keep {
            delete(id, &m.file).await?;
        }
    }
    Ok(())
}

/// Which installed jars have a newer build on Modrinth. Jars Modrinth does not
/// recognise -- hand-built, or from anywhere else -- are simply absent from the
/// answer, so they never nag.
pub async fn check_updates(id: &str) -> Result<Vec<ModUpdate>> {
    let instance = instance::get(id).await?;
    let installed = list(id).await?;
    let hashes: Vec<String> = installed
        .iter()
        .map(|m| m.sha1.clone())
        .filter(|h| !h.is_empty())
        .collect();
    let newest = crate::modrinth::updates(&hashes, &instance.mc_version).await?;

    Ok(installed
        .into_iter()
        .filter_map(|m| {
            let version = newest.get(&m.sha1)?;
            // The API answers with the newest build even when that is the file
            // we sent; only a different jar counts as an update.
            let same = version
                .jar()
                .is_some_and(|f| f.hashes.sha1.as_deref() == Some(m.sha1.as_str()));
            (!same).then(|| ModUpdate {
                file: m.file,
                name: m.name,
                current_version: m.version,
                new_version: version.version_number.clone(),
                version_id: version.id.clone(),
                changelog: version.changelog.clone(),
            })
        })
        .collect())
}

/// Replace one jar with a specific Modrinth version, keeping it disabled if it
/// was disabled.
pub async fn update_to(app: &AppHandle, id: &str, file: &str, version_id: &str) -> Result<String> {
    let version = crate::modrinth::version(version_id).await?;
    let jar = version.jar().ok_or_else(|| Error::msg("That version has no jar."))?;
    let name = if file.ends_with(DISABLED) {
        format!("{}{DISABLED}", jar.filename)
    } else {
        jar.filename.clone()
    };
    fetch(app, id, &jar.url, &name, jar.hashes.sha1.clone(), Some(jar.size)).await?;
    if name != file {
        delete(id, file).await?;
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::checked_name;

    #[test]
    fn file_names_from_the_frontend_cannot_escape_the_mods_folder() {
        assert!(checked_name("sodium.jar").is_ok());
        assert!(checked_name("../../instance.json").is_err());
        assert!(checked_name("sub/mod.jar").is_err());
        assert!(checked_name("sub\\mod.jar").is_err());
        assert!(checked_name("").is_err());
    }
}
