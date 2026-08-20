//! The content folders of an instance — `mods/`, `resourcepacks/`,
//! `shaderpacks/` — what is in them, and adding to them.
//!
//! Deliberately thin. The folder is the source of truth, exactly like the
//! instance list — no index, no database. A file is disabled by appending
//! `.disabled` to its name, which is the convention Minecraft launchers have
//! shared for a decade and what the user's other tools expect.
//!
//! The three folders differ in four things and nothing else: the directory
//! name, the file extension the game reads, what Modrinth calls the project
//! type, and which loaders tag a build. `Kind` carries all four so every
//! function below stays one implementation.

use crate::download::{self, Job};
use crate::error::{Error, Result};
use crate::instance::{self, Loader};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::AppHandle;

const DISABLED: &str = ".disabled";

/// Which content folder a call is about. The serde names are the folder names,
/// because they are also what the frontend sends.
#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Mods,
    Resourcepacks,
    Shaderpacks,
}

impl Kind {
    pub fn folder(self) -> &'static str {
        match self {
            Kind::Mods => "mods",
            Kind::Resourcepacks => "resourcepacks",
            Kind::Shaderpacks => "shaderpacks",
        }
    }

    /// The extension the game reads out of this folder. Anything else in there
    /// is not content and is not listed.
    fn extension(self) -> &'static str {
        match self {
            Kind::Mods => ".jar",
            _ => ".zip",
        }
    }

    /// Modrinth's `project_type` facet.
    pub fn project_type(self) -> &'static str {
        match self {
            Kind::Mods => "mod",
            Kind::Resourcepacks => "resourcepack",
            Kind::Shaderpacks => "shader",
        }
    }

    /// The loaders Modrinth tags a build of this kind with. Verified against
    /// the live API: a resource pack build is "minecraft", a shader is both
    /// "iris" and "optifine" — Iris reads OptiFine shaders, so neither can be
    /// dropped without hiding most of the catalogue.
    ///
    /// Mods are the instance's business rather than the folder's, so they come
    /// from `Loader::mod_loaders`: a Quilt instance runs Fabric mods too.
    pub fn loaders(self, loader: Loader) -> &'static [&'static str] {
        match self {
            Kind::Mods => loader.mod_loaders(),
            Kind::Resourcepacks => &["minecraft"],
            Kind::Shaderpacks => &["iris", "optifine"],
        }
    }

    /// Only mods declare dependencies worth chasing: a missing Fabric API is
    /// the usual reason a fresh install crashes. A pack's dependency is the
    /// loader itself, which cannot be installed into a pack folder.
    pub fn installs_dependencies(self) -> bool {
        self == Kind::Mods
    }
}

#[derive(Serialize, Clone)]
pub struct ModFile {
    /// File name as it sits on disk, `.disabled` suffix included.
    pub file: String,
    /// Name from the archive's own metadata, or the file name when it has none.
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub size: u64,
    /// SHA-1 of the jar, which is what Modrinth matches files by.
    pub sha1: String,
    /// The icon `fabric.mod.json` names, inlined as a `data:` URI. Read from
    /// the jar itself so an installed mod needs no network to show artwork.
    pub icon: Option<String>,
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

async fn folder(id: &str, kind: Kind) -> Result<PathBuf> {
    Ok(instance::get(id).await?.game_dir().join(kind.folder()))
}

/// Reject anything that is not a plain file name: these come from the frontend
/// and are joined onto a path.
pub(crate) fn checked_name(file: &str) -> Result<&str> {
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

/// Icons live inside the jar and only ever reach the frontend as a `data:`
/// URI, so a whole-file encode is all that is needed -- no streaming, no crate.
fn base64(bytes: &[u8]) -> String {
    const SET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(SET[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A mod icon is bigger than a favicon but never a texture pack; anything past
/// this is not artwork we want to push across IPC for every row.
const MAX_ICON: u64 = 512 * 1024;

/// `icon` is a path inside the jar, or a map of size -> path on mods that ship
/// several. Any of them renders fine, so take the first.
fn icon_path(json: &serde_json::Value) -> Option<&str> {
    match json.get("icon")? {
        serde_json::Value::String(s) => Some(s),
        serde_json::Value::Object(map) => map.values().find_map(|v| v.as_str()),
        _ => None,
    }
}

/// Name, version and icon out of an archive, however its format states them.
fn read_metadata(path: &std::path::Path, kind: Kind) -> Option<(String, String, Option<String>)> {
    match kind {
        Kind::Mods => read_mod_metadata(path),
        _ => read_pack_metadata(path),
    }
}

/// A resource pack states its name nowhere — the file name is the name — but
/// it does carry a description and the `pack.png` every launcher shows. A
/// shader pack usually has neither, and falls back to the file name.
fn read_pack_metadata(path: &std::path::Path) -> Option<(String, String, Option<String>)> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let icon = read_icon(&mut zip, "pack.png");
    // `description` is a plain string on most packs and a JSON text component
    // on a few; only the simple form is worth reading for a one-line caption.
    let description = zip
        .by_name("pack.mcmeta")
        .ok()
        .and_then(|e| serde_json::from_reader::<_, serde_json::Value>(e).ok())
        .and_then(|json| Some(json.pointer("/pack/description")?.as_str()?.to_string()))
        .unwrap_or_default();
    let name = path.file_name()?.to_string_lossy();
    let name = name.trim_end_matches(DISABLED).trim_end_matches(".zip").to_string();
    Some((name, description, icon))
}

/// Name, version and icon out of the jar's `fabric.mod.json`, if it has one.
fn read_mod_metadata(path: &std::path::Path) -> Option<(String, String, Option<String>)> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let json: serde_json::Value = serde_json::from_reader(zip.by_name("fabric.mod.json").ok()?).ok()?;
    let name = json.get("name").and_then(|v| v.as_str())?.to_string();
    let version = json
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let icon = icon_path(&json).and_then(|p| read_icon(&mut zip, p));
    Some((name, version, icon))
}

fn read_icon<R: std::io::Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<String> {
    use std::io::Read;
    let mut entry = zip.by_name(name).ok()?;
    if entry.size() > MAX_ICON {
        return None;
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes).ok()?;
    let mime = if name.to_lowercase().ends_with(".svg") { "image/svg+xml" } else { "image/png" };
    Some(format!("data:{mime};base64,{}", base64(&bytes)))
}

/// Name, version, icon, SHA-1: everything `list` reads out of one archive.
type JarInfo = (String, String, Option<String>, String);

/// Reading an archive means hashing all of it and unzipping an icon out of it,
/// and `list` runs on every tab open, install and toggle. Cache on the identity
/// the filesystem already tracks -- path, size, modified time -- so a file that
/// was replaced or edited is re-read and one that was not never is.
///
/// ponytail: unbounded, one entry per file seen this session; add eviction if a
/// user ever holds enough instances for that to matter.
fn cached_jar(
    path: &std::path::Path,
    size: u64,
    mtime: Option<std::time::SystemTime>,
    kind: Kind,
) -> JarInfo {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<(PathBuf, u64, Option<std::time::SystemTime>), JarInfo>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = (path.to_path_buf(), size, mtime);

    if let Some(hit) = cache.lock().unwrap().get(&key) {
        return hit.clone();
    }
    let file = path.file_name().unwrap_or_default().to_string_lossy();
    let (name, version, icon) = read_metadata(path, kind)
        .unwrap_or_else(|| (file.trim_end_matches(DISABLED).to_string(), String::new(), None));
    let info = (name, version, icon, sha1_of(path).unwrap_or_default());
    cache.lock().unwrap().insert(key, info.clone());
    info
}

/// Everything in one of the instance's content folders, enabled or not.
///
/// ponytail: archives only. Minecraft also reads a resource pack unpacked into
/// a directory; listing those means a recursive delete behind the trash button,
/// so they stay invisible until someone asks.
pub async fn list(id: &str, kind: Kind) -> Result<Vec<ModFile>> {
    let dir = folder(id, kind).await?;
    let extension = kind.extension();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new()); // the folder not existing yet is not an error
    };

    let mut mods: Vec<ModFile> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let enabled = file.ends_with(extension);
            if !enabled && !file.ends_with(&format!("{extension}{DISABLED}")) {
                return None;
            }
            let meta = e.metadata().ok();
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let (name, version, icon, sha1) =
                cached_jar(&e.path(), size, meta.and_then(|m| m.modified().ok()), kind);
            Some(ModFile { file, name, version, icon, enabled, size, sha1 })
        })
        .collect();

    mods.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(mods)
}

/// Toggle a file by renaming it. Returns the new file name.
pub async fn set_enabled(id: &str, kind: Kind, file: &str, enabled: bool) -> Result<String> {
    let dir = folder(id, kind).await?;
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

pub async fn delete(id: &str, kind: Kind, file: &str) -> Result<()> {
    let path = folder(id, kind).await?.join(checked_name(file)?);
    tokio::fs::remove_file(path).await?;
    Ok(())
}

/// Fetch one file into the instance's folder for `kind`.
pub async fn fetch(
    app: &AppHandle,
    id: &str,
    kind: Kind,
    url: &str,
    file: &str,
    sha1: Option<String>,
    size: Option<u64>,
) -> Result<()> {
    let dir = folder(id, kind).await?;
    tokio::fs::create_dir_all(&dir).await?;
    let job = Job {
        url: url.to_string(),
        path: dir.join(checked_name(file)?),
        sha1,
        size,
    };
    download::run(app, "Mod", vec![job]).await
}

/// Copy a file the user dropped on the window into the folder for `kind`.
pub async fn add_file(id: &str, kind: Kind, source: &std::path::Path) -> Result<String> {
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| Error::msg("That is not a file."))?;
    let extension = kind.extension();
    if !name.to_lowercase().ends_with(extension) {
        return Err(Error::msg(format!(
            "Only {extension} files belong in the {} folder.",
            kind.folder()
        )));
    }
    let dir = folder(id, kind).await?;
    tokio::fs::create_dir_all(&dir).await?;
    tokio::fs::copy(source, dir.join(checked_name(&name)?)).await?;
    Ok(name)
}

/// The Modrinth project ids of the jars already in this instance's folder, so
/// the browser can mark them installed. Jars from anywhere else are simply not
/// in the answer.
pub async fn installed_projects(id: &str, kind: Kind) -> Result<Vec<String>> {
    let hashes: Vec<String> = list(id, kind)
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
pub async fn remove_other_versions(id: &str, kind: Kind, project: &str, keep: &str) -> Result<()> {
    let installed = list(id, kind).await?;
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
            delete(id, kind, &m.file).await?;
        }
    }
    Ok(())
}

/// Which installed jars have a newer build on Modrinth. Jars Modrinth does not
/// recognise -- hand-built, or from anywhere else -- are simply absent from the
/// answer, so they never nag.
pub async fn check_updates(id: &str, kind: Kind) -> Result<Vec<ModUpdate>> {
    let instance = instance::get(id).await?;
    let installed = list(id, kind).await?;
    let hashes: Vec<String> = installed
        .iter()
        .map(|m| m.sha1.clone())
        .filter(|h| !h.is_empty())
        .collect();
    let newest =
        crate::modrinth::updates(&hashes, &instance.mc_version, kind.loaders(instance.loader))
            .await?;

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
pub async fn update_to(
    app: &AppHandle,
    id: &str,
    kind: Kind,
    file: &str,
    version_id: &str,
) -> Result<String> {
    let version = crate::modrinth::version(version_id).await?;
    let jar = version.jar().ok_or_else(|| Error::msg("That version has no jar."))?;
    let name = if file.ends_with(DISABLED) {
        format!("{}{DISABLED}", jar.filename)
    } else {
        jar.filename.clone()
    };
    fetch(app, id, kind, &jar.url, &name, jar.hashes.sha1.clone(), Some(jar.size)).await?;
    if name != file {
        delete(id, kind, file).await?;
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::{Kind, Loader, base64, checked_name, icon_path};

    #[test]
    fn each_folder_knows_its_extension_and_modrinth_names() {
        // The serde name is the folder name, which is also what the frontend
        // sends; drift here silently reads the wrong directory.
        assert_eq!(serde_json::to_string(&Kind::Shaderpacks).unwrap(), "\"shaderpacks\"");
        assert_eq!(Kind::Shaderpacks.folder(), "shaderpacks");
        assert_eq!(Kind::Mods.extension(), ".jar");
        assert_eq!(Kind::Resourcepacks.extension(), ".zip");
        assert_eq!(Kind::Resourcepacks.project_type(), "resourcepack");
        assert_eq!(Kind::Shaderpacks.project_type(), "shader");
        assert_eq!(Kind::Resourcepacks.loaders(Loader::Vanilla), ["minecraft"]);
        // Quilt runs Fabric mods; asking for "quilt" alone hides most of them.
        assert_eq!(Kind::Mods.loaders(Loader::Quilt), ["quilt", "fabric"]);
        assert_eq!(Kind::Mods.loaders(Loader::Fabric), ["fabric"]);
        assert!(Kind::Mods.installs_dependencies());
        assert!(!Kind::Shaderpacks.installs_dependencies());
    }

    #[test]
    fn file_names_from_the_frontend_cannot_escape_the_mods_folder() {
        assert!(checked_name("sodium.jar").is_ok());
        assert!(checked_name("../../instance.json").is_err());
        assert!(checked_name("sub/mod.jar").is_err());
        assert!(checked_name("sub\\mod.jar").is_err());
        assert!(checked_name("").is_err());
    }

    /// The cache is keyed on size and mtime, so replacing a jar must not serve
    /// the old jar's hash -- that is what "already installed" is matched by.
    #[test]
    fn a_rewritten_jar_is_read_again() {
        let path = std::env::temp_dir().join("justlauncher_cache_test.jar");
        std::fs::write(&path, b"one").unwrap();
        let first =
            super::cached_jar(&path, 3, std::fs::metadata(&path).unwrap().modified().ok(), Kind::Mods);

        std::fs::write(&path, b"two different bytes").unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        let second = super::cached_jar(&path, meta.len(), meta.modified().ok(), Kind::Mods);
        std::fs::remove_file(&path).ok();

        assert_ne!(first.3, second.3, "hash must follow the file, not the path");
    }

    #[test]
    fn base64_pads_every_tail_length() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // The PNG magic bytes, which is what every mod icon actually starts with.
        assert_eq!(base64(&[0x89, b'P', b'N', b'G']), "iVBORw==");
    }

    #[test]
    fn icon_is_read_from_a_string_or_a_size_map() {
        let one = serde_json::json!({ "icon": "assets/sodium/icon.png" });
        assert_eq!(icon_path(&one), Some("assets/sodium/icon.png"));
        let many = serde_json::json!({ "icon": { "128": "icon128.png" } });
        assert_eq!(icon_path(&many), Some("icon128.png"));
        assert_eq!(icon_path(&serde_json::json!({})), None);
    }
}
