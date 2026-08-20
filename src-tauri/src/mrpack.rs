//! Importing a Modrinth modpack (`.mrpack`).
//!
//! The archive is a zip holding `modrinth.index.json` — a list of files to
//! fetch, with hashes — plus `overrides/` folders of configs the pack ships
//! itself. Nothing is bundled that Modrinth can serve, which is why importing
//! one is mostly downloading.
//!
//! Only the loaders the launcher can install are accepted; a Forge or NeoForge
//! pack is rejected here rather than failing at launch with an unreadable Java
//! error.

use crate::download::{self, Job};
use crate::error::{Error, Result};
use crate::instance::{self, Instance, Loader};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

const INDEX_NAME: &str = "modrinth.index.json";
/// Folders whose contents are copied into the game directory as-is. The
/// server-side one is deliberately absent: this is a client launcher.
const OVERRIDES: [&str; 2] = ["overrides/", "client-overrides/"];

#[derive(Deserialize)]
struct Index {
    name: String,
    #[serde(default)]
    files: Vec<IndexFile>,
    dependencies: HashMap<String, String>,
}

#[derive(Deserialize)]
struct IndexFile {
    path: String,
    #[serde(default)]
    hashes: HashMap<String, String>,
    #[serde(default)]
    env: Option<Env>,
    #[serde(default)]
    downloads: Vec<String>,
    #[serde(rename = "fileSize", default)]
    file_size: Option<u64>,
}

#[derive(Deserialize)]
struct Env {
    #[serde(default)]
    client: String,
}

impl IndexFile {
    /// A pack lists server-only mods too; `unsupported` on the client means
    /// "do not install here", not "optional".
    fn wanted(&self) -> bool {
        !matches!(self.env.as_ref().map(|e| e.client.as_str()), Some("unsupported"))
    }
}

/// Resolve an index path against the game directory, refusing anything that
/// would write outside it. The index is attacker-controlled data.
fn safe_join(root: &Path, relative: &str) -> Result<PathBuf> {
    let bad = relative.is_empty()
        || relative.contains("..")
        || relative.starts_with('/')
        || relative.starts_with('\\')
        || Path::new(relative).is_absolute();
    if bad {
        return Err(Error::msg(format!("Pack contains an unsafe path: {relative}")));
    }
    Ok(root.join(relative.replace('\\', "/")))
}

/// The Minecraft version and loader a pack asks for.
fn loader_of(deps: &HashMap<String, String>) -> Result<(String, Loader, String)> {
    let mc = deps
        .get("minecraft")
        .ok_or_else(|| Error::msg("Pack does not say which Minecraft version it is for."))?
        .clone();

    for unsupported in ["forge", "neoforge"] {
        if deps.contains_key(unsupported) {
            return Err(Error::msg(format!(
                "This is a {unsupported} pack. JustLauncher launches vanilla, Fabric and Quilt."
            )));
        }
    }
    // Quilt first: a pack that names both is a Quilt pack that also runs the
    // Fabric mods it lists.
    for (key, loader) in [("quilt-loader", Loader::Quilt), ("fabric-loader", Loader::Fabric)] {
        if let Some(v) = deps.get(key) {
            return Ok((mc, loader, v.clone()));
        }
    }
    Ok((mc, Loader::Vanilla, String::new()))
}

/// Extract the pack into a new instance and download everything it lists.
pub async fn import(app: &AppHandle, archive: &Path) -> Result<Instance> {
    let path = archive.to_path_buf();
    // Unzipping is blocking IO, and the index has to be parsed before an
    // instance can be created for it.
    let index = tokio::task::spawn_blocking({
        let path = path.clone();
        move || read_index(&path)
    })
    .await
    .map_err(|e| Error::msg(e.to_string()))??;

    let (mc_version, loader, loader_version) = loader_of(&index.dependencies)?;
    let mut inst = instance::create(&index.name, &mc_version, loader).await?;
    inst.loader_version = loader_version;
    inst.save().await?;

    // From here on a failure leaves a half-built instance, so remove it rather
    // than leaving something that looks playable and is not.
    match fill(app, &inst, index, &path).await {
        Ok(()) => Ok(inst),
        Err(e) => {
            let _ = instance::delete(&inst.id).await;
            Err(e)
        }
    }
}

async fn fill(app: &AppHandle, inst: &Instance, index: Index, archive: &Path) -> Result<()> {
    let game_dir = inst.game_dir();

    let mut jobs = Vec::new();
    for file in index.files.iter().filter(|f| f.wanted()) {
        let url = file
            .downloads
            .first()
            .ok_or_else(|| Error::msg(format!("Pack lists {} with no download.", file.path)))?;
        // The pack chooses both the URL and where its bytes land, so require
        // the transport that at least authenticates the host.
        if !url.starts_with("https://") {
            return Err(Error::msg(format!("Pack wants to download {url} over plain HTTP.")));
        }
        jobs.push(Job {
            url: url.clone(),
            path: safe_join(&game_dir, &file.path)?,
            sha1: file.hashes.get("sha1").cloned(),
            size: file.file_size,
        });
    }

    let archive = archive.to_path_buf();
    let dest = game_dir.clone();
    tokio::task::spawn_blocking(move || extract_overrides(&archive, &dest))
        .await
        .map_err(|e| Error::msg(e.to_string()))??;

    download::run(app, "Pack files", jobs).await
}

/// Read `modrinth.index.json` out of the archive.
fn read_index(archive: &Path) -> Result<Index> {
    let file = std::fs::File::open(archive)
        .map_err(|e| Error::msg(format!("Cannot open {}: {e}", archive.display())))?;
    let mut zip = zip::ZipArchive::new(file)?;
    let mut text = String::new();
    zip.by_name(INDEX_NAME)
        .map_err(|_| Error::msg("That file is not a Modrinth pack (no modrinth.index.json)."))?
        .read_to_string(&mut text)?;
    serde_json::from_str(&text).map_err(|e| Error::msg(format!("Pack index is invalid: {e}")))
}

/// Copy the pack's own config files into the game directory.
fn extract_overrides(archive: &Path, game_dir: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        // `enclosed_name` rejects absolute paths and `..`, so a crafted archive
        // cannot write outside the instance.
        let Some(name) = entry.enclosed_name() else { continue };
        let name = name.to_string_lossy().replace('\\', "/");
        let Some(relative) = OVERRIDES.iter().find_map(|p| name.strip_prefix(p)) else {
            continue;
        };
        if entry.is_dir() || relative.is_empty() {
            continue;
        }
        let out = game_dir.join(relative);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::io::copy(&mut entry, &mut std::fs::File::create(&out)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deps(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn fabric_and_vanilla_packs_are_accepted() {
        let (mc, loader, version) =
            loader_of(&deps(&[("minecraft", "1.20.1"), ("fabric-loader", "0.16.0")])).unwrap();
        assert_eq!(mc, "1.20.1");
        assert_eq!(loader, Loader::Fabric);
        assert_eq!(version, "0.16.0");

        let (_, loader, _) = loader_of(&deps(&[("minecraft", "1.20.1")])).unwrap();
        assert_eq!(loader, Loader::Vanilla);
    }

    #[test]
    fn quilt_packs_are_accepted_and_win_over_a_fabric_entry() {
        let (_, loader, version) =
            loader_of(&deps(&[("minecraft", "1.21.1"), ("quilt-loader", "0.24.0")])).unwrap();
        assert_eq!(loader, Loader::Quilt);
        assert_eq!(version, "0.24.0");

        // Quilt runs Fabric mods, so a pack naming both is a Quilt pack.
        let (_, loader, _) = loader_of(&deps(&[
            ("minecraft", "1.21.1"),
            ("quilt-loader", "0.24.0"),
            ("fabric-loader", "0.16.0"),
        ]))
        .unwrap();
        assert_eq!(loader, Loader::Quilt);
    }

    #[test]
    fn other_loaders_are_refused_before_anything_is_created() {
        for l in ["forge", "neoforge"] {
            assert!(loader_of(&deps(&[("minecraft", "1.20.1"), (l, "1.0")])).is_err(), "{l}");
        }
        assert!(loader_of(&deps(&[("fabric-loader", "0.16.0")])).is_err());
    }

    #[test]
    fn paths_cannot_escape_the_game_directory() {
        let root = Path::new("/game");
        assert!(safe_join(root, "mods/sodium.jar").is_ok());
        assert!(safe_join(root, "../../evil").is_err());
        assert!(safe_join(root, "/etc/passwd").is_err());
        assert!(safe_join(root, "").is_err());
    }

    /// Verbatim from a published pack (Fabulously Optimized 14.0.0-beta.6),
    /// trimmed to two files: the shape of the real thing, not a guess at it.
    const REAL_INDEX: &str = r#"{
      "formatVersion": 1,
      "game": "minecraft",
      "versionId": "14.0.0-beta.6",
      "name": "Fabulously Optimized",
      "files": [
        {
          "path": "mods/BetterGrassify-1.8.7+fabric.26.2.jar",
          "hashes": {
            "sha1": "0f4a890d07402280686a518579fa9fa02309e315",
            "sha512": "7b70e796cea2ee57a6022108092517b6aa39d5a19b8da1de26685ae53b8fbb4717cd91ee1d232c54efa8e210ebe9b760d3dab79bf9a5fdb5ae75d97b461a1fab"
          },
          "env": { "client": "required", "server": "required" },
          "downloads": [
            "https://cdn.modrinth.com/data/m5T5xmUy/versions/r4yqxYQl/BetterGrassify-1.8.7%2Bfabric.26.2.jar"
          ],
          "fileSize": 88858
        },
        {
          "path": "resourcepacks/Fabulously-Optimized-Reborn.zip",
          "hashes": { "sha1": "b0b2d6b3fc4581fb1097e807eeaec09bca7dac4c" },
          "env": { "client": "required", "server": "unsupported" },
          "downloads": ["https://cdn.modrinth.com/data/x/versions/y/pack.zip"],
          "fileSize": 3463625
        }
      ],
      "dependencies": { "fabric-loader": "0.19.3", "minecraft": "26.2" }
    }"#;

    #[test]
    fn a_real_pack_index_parses() {
        let index: Index = serde_json::from_str(REAL_INDEX).unwrap();
        assert_eq!(index.name, "Fabulously Optimized");
        assert_eq!(index.files.len(), 2);

        let (mc, loader, version) = loader_of(&index.dependencies).unwrap();
        assert_eq!(mc, "26.2");
        assert_eq!(loader, Loader::Fabric);
        assert_eq!(version, "0.19.3");

        let jar = &index.files[0];
        assert_eq!(jar.file_size, Some(88858));
        assert_eq!(jar.hashes.get("sha1").unwrap(), "0f4a890d07402280686a518579fa9fa02309e315");
        // Modrinth percent-encodes the `+` in a file name; the URL is used as
        // given and the path is not, so the two must not be confused.
        assert!(jar.downloads[0].contains("%2B"));
        assert!(safe_join(Path::new("/game"), &jar.path)
            .unwrap()
            .ends_with("mods/BetterGrassify-1.8.7+fabric.26.2.jar"));

        // A pack ships more than mods, and `server: unsupported` is not a
        // reason to skip a client-side file.
        assert!(index.files[1].path.starts_with("resourcepacks/"));
        assert!(index.files.iter().all(|f| f.wanted()));
    }

    #[test]
    fn server_only_files_are_skipped() {
        let file = |client: Option<&str>| IndexFile {
            path: "mods/x.jar".into(),
            hashes: HashMap::new(),
            env: client.map(|c| Env { client: c.into() }),
            downloads: vec![],
            file_size: None,
        };
        assert!(file(Some("required")).wanted());
        assert!(file(Some("optional")).wanted());
        assert!(!file(Some("unsupported")).wanted());
        // No env block at all means the file is simply for everyone.
        assert!(file(None).wanted());
    }
}
