//! Importing a Modrinth modpack (`.mrpack`) and exporting one back.
//!
//! The archive is a zip holding `modrinth.index.json` — a list of files to
//! fetch, with hashes — plus `overrides/` folders of configs the pack ships
//! itself. Nothing is bundled that Modrinth can serve, which is why importing
//! one is mostly downloading, and why exporting walks the game folder asking
//! which jars Modrinth knows and ships only the rest as overrides.
//!
//! Only the loaders the launcher can install are accepted; a Forge or NeoForge
//! pack is rejected here rather than failing at launch with an unreadable Java
//! error.

use crate::download::{self, Job};
use crate::error::{Error, Result};
use crate::instance::{self, Instance, Loader, PackSource};
use crate::paths;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
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

    // Quilt first: a pack that names both is a Quilt pack that also runs the
    // Fabric mods it lists. NeoForge before Forge for the same reason -- a
    // pack that names both is a NeoForge pack.
    for (key, loader) in [
        ("quilt-loader", Loader::Quilt),
        ("fabric-loader", Loader::Fabric),
        ("neoforge", Loader::NeoForge),
        ("forge", Loader::Forge),
    ] {
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
    let inst = instance::create(&index.name, &mc_version, loader, &loader_version).await?;

    // From here on a failure leaves a half-built instance, so remove it rather
    // than leaving something that looks playable and is not.
    let files = match fill(app, &inst, index, &path).await {
        Ok(files) => files,
        Err(e) => {
            let _ = instance::delete(&inst.id).await;
            return Err(e);
        }
    };

    // Which Modrinth build this archive is, asked by its hash, so a picked
    // file is recognised exactly like a browsed one. Best effort: a pack
    // Modrinth does not host, or no connection, only means no update offers.
    let mut inst = inst;
    if let Some(version) = identify(&path).await {
        inst.pack = Some(PackSource {
            project: version.project_id,
            version_id: version.id,
            version_number: version.version_number,
            files,
        });
        inst.save().await?;
    }
    Ok(inst)
}

async fn identify(archive: &Path) -> Option<crate::modrinth::Version> {
    use sha1::{Digest, Sha1};
    let sha1 = hex::encode(Sha1::digest(tokio::fs::read(archive).await.ok()?));
    crate::modrinth::version_files(std::slice::from_ref(&sha1)).await.ok()?.remove(&sha1)
}

/// Download a pack build's `.mrpack` to a temp file. The caller removes it.
pub async fn fetch(app: &AppHandle, version: &crate::modrinth::Version) -> Result<PathBuf> {
    let file = version.jar().ok_or_else(|| Error::msg("That pack build has no file."))?;
    let archive = std::env::temp_dir().join(format!("justlauncher-{}.mrpack", version.id));
    // A leftover from an interrupted install would be accepted on its size
    // alone, so the download starts from an empty slot.
    let _ = tokio::fs::remove_file(&archive).await;
    download::run(
        app,
        "Pack",
        vec![Job {
            url: file.url.clone(),
            path: archive.clone(),
            sha1: file.hashes.sha1.clone(),
            size: Some(file.size),
        }],
    )
    .await?;
    Ok(archive)
}

/// The newest release of the pack an instance came from, when it is not the
/// one installed. Modrinth lists versions newest first.
pub async fn newer(inst: &Instance) -> Result<Option<crate::modrinth::Version>> {
    let Some(pack) = &inst.pack else { return Ok(None) };
    let all = crate::modrinth::all_versions(&pack.project).await?;
    let latest = all.iter().find(|v| v.version_type == "release").or_else(|| all.first());
    Ok(latest.filter(|v| v.id != pack.version_id).cloned())
}

/// Move a pack instance to another build of its pack, keeping the player's
/// own files.
///
/// The new build's files are downloaded first and the ones only the old
/// build listed are deleted after, so a failed download leaves the instance
/// on its old build with, at worst, some new jars beside the old ones, and
/// running the update again finishes it. Overrides are written over, as every
/// launcher does: a pack's configs are part of the build. Worlds, options,
/// screenshots and anything the pack never listed are not touched.
pub async fn update(app: &AppHandle, inst: &mut Instance, version_id: &str) -> Result<()> {
    let pack = inst
        .pack
        .clone()
        .ok_or_else(|| Error::msg("This instance was not installed from a Modrinth pack."))?;
    let version = crate::modrinth::version(version_id).await?;
    if version.project_id != pack.project {
        return Err(Error::msg("That build belongs to a different pack."));
    }

    let archive = fetch(app, &version).await?;
    let result = apply(app, inst, &pack, &version, &archive).await;
    let _ = tokio::fs::remove_file(&archive).await;
    result
}

async fn apply(
    app: &AppHandle,
    inst: &mut Instance,
    old: &PackSource,
    version: &crate::modrinth::Version,
    archive: &Path,
) -> Result<()> {
    let index = tokio::task::spawn_blocking({
        let archive = archive.to_path_buf();
        move || read_index(&archive)
    })
    .await
    .map_err(|e| Error::msg(e.to_string()))??;
    let (mc_version, loader, loader_version) = loader_of(&index.dependencies)?;
    let files = fill(app, inst, index, archive).await?;

    let game_dir = inst.game_dir();
    for stale in stale_files(&old.files, &files) {
        match tokio::fs::remove_file(safe_join(&game_dir, stale)?).await {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }

    if (inst.mc_version.as_str(), inst.loader, inst.loader_version.as_str())
        != (mc_version.as_str(), loader, loader_version.as_str())
    {
        // Libraries, assets and the loader profile are all per version,
        // exactly as when the user changes the version by hand.
        inst.installed = false;
    }
    inst.mc_version = mc_version;
    inst.loader = loader;
    inst.loader_version = loader_version;
    inst.pack = Some(PackSource {
        project: version.project_id.clone(),
        version_id: version.id.clone(),
        version_number: version.version_number.clone(),
        files,
    });
    inst.save().await
}

/// What the old build installed that the new one does not list.
fn stale_files<'a>(old: &'a [String], new: &'a [String]) -> impl Iterator<Item = &'a String> {
    let keep: std::collections::HashSet<&String> = new.iter().collect();
    old.iter().filter(move |f| !keep.contains(f))
}

/// Extract the overrides and download everything the index lists. Returns the
/// game-folder paths the index installed, which is what `PackSource` keeps.
async fn fill(app: &AppHandle, inst: &Instance, index: Index, archive: &Path) -> Result<Vec<String>> {
    let game_dir = inst.game_dir();

    let mut jobs = Vec::new();
    let mut paths = Vec::new();
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
        paths.push(file.path.replace('\\', "/"));
    }

    let archive = archive.to_path_buf();
    let dest = game_dir.clone();
    tokio::task::spawn_blocking(move || extract_overrides(&archive, &dest, &OVERRIDES))
        .await
        .map_err(|e| Error::msg(e.to_string()))??;

    download::run(app, "Pack files", jobs).await?;
    Ok(paths)
}

// --------------------------------------------------------------------- export

/// The mirror of the import index: what a pack publisher writes, not reads.
#[derive(Serialize)]
struct OutIndex {
    #[serde(rename = "formatVersion")]
    format_version: u8,
    game: &'static str,
    #[serde(rename = "versionId")]
    version_id: String,
    name: String,
    files: Vec<OutFile>,
    /// BTreeMap so the output is byte-stable; the format does not care.
    dependencies: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct OutFile {
    path: String,
    hashes: OutHashes,
    downloads: Vec<String>,
    #[serde(rename = "fileSize")]
    file_size: u64,
}

/// The format requires both digests of every file it lists, so both are
/// computed even though the launcher itself only ever verifies the SHA-1.
#[derive(Serialize)]
struct OutHashes {
    sha1: String,
    sha512: String,
}

/// A content file that has been hashed, ready to be resolved against Modrinth.
struct Hashed {
    path: PathBuf,
    rel: String,
    sha1: String,
    sha512: String,
    size: u64,
}

/// Folders and files in the game directory that are the player's own rather
/// than the pack's. A pack shares a setup; it does not share someone's worlds,
/// screenshots, keybinds or server list. `logs` and `natives` go for the same
/// reason the zip export drops them: one holds the session token, the other is
/// rebuilt on every launch.
fn is_own(relative: &str) -> bool {
    let first = relative.split('/').next().unwrap_or(relative);
    matches!(
        first,
        "saves" | "screenshots" | "logs" | "crash-reports" | "debug" | "natives" | ".fabric" | "webcache"
    ) || matches!(relative, "options.txt" | "servers.dat" | "usercache.json" | "usernamecache.json")
        || relative.ends_with(".part")
}

/// A candidate for Modrinth resolution: a file directly inside one of the
/// three content folders, which are the only trees the format knows how to
/// describe. Anything deeper, or anywhere else in the game folder, rides in
/// `overrides/` as-is.
fn is_content_file(relative: &str) -> bool {
    match relative.split_once('/') {
        Some(("mods" | "resourcepacks" | "shaderpacks", name)) => {
            !name.is_empty() && !name.contains('/')
        }
        _ => false,
    }
}

/// Split hashed content files into index entries for the ones Modrinth can
/// serve and overrides for the ones it cannot — a jar built by hand, or
/// downloaded from somewhere Modrinth has no file for. The on-disk path is
/// kept rather than Modrinth's own file name, so a renamed jar still lands
/// where the instance expects it.
fn split_known(
    hashed: Vec<Hashed>,
    known: &HashMap<String, crate::modrinth::Version>,
) -> (Vec<OutFile>, Vec<(PathBuf, String)>) {
    let mut files = Vec::new();
    let mut overrides = Vec::new();
    for h in hashed {
        let url = known.get(&h.sha1).and_then(|version| {
            version
                .files
                .iter()
                .find(|f| f.hashes.sha1.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(&h.sha1)))
                .or_else(|| version.jar())
                .map(|f| f.url.clone())
        });
        match url {
            Some(url) => files.push(OutFile {
                path: h.rel,
                hashes: OutHashes { sha1: h.sha1.clone(), sha512: h.sha512 },
                downloads: vec![url],
                file_size: h.size,
            }),
            None => overrides.push((h.path, h.rel)),
        }
    }
    (files, overrides)
}

/// Write the instance to `exports/<id>.mrpack`, returning the archive path.
///
/// Every jar Modrinth recognises by hash becomes a download entry pointing at
/// its own CDN; everything else in the game folder becomes an override. The
/// result re-imports here and in every other launcher that speaks the format.
pub async fn export(inst: &Instance) -> Result<PathBuf> {
    // The loader build is part of the pack's identity; without it the index
    // would silently describe a vanilla pack.
    if inst.loader != Loader::Vanilla && inst.loader_version.is_empty() {
        return Err(Error::msg(
            "The loader build is not resolved yet. Install the instance once, then export.",
        ));
    }

    let game_dir = inst.game_dir();

    // Walking and hashing a heavily modded folder is blocking IO.
    let (hashed, mut overrides) = tokio::task::spawn_blocking({
        let dir = game_dir.clone();
        move || collect_exportable(&dir)
    })
    .await
    .map_err(|e| Error::msg(e.to_string()))??;

    // Best effort on purpose: with the network down, or for jars Modrinth has
    // never seen, everything still ships — as overrides rather than entries.
    let lookup: Vec<String> = hashed.iter().map(|h| h.sha1.clone()).collect();
    let known = crate::modrinth::version_files(&lookup).await.unwrap_or_default();
    let (files, unresolved) = split_known(hashed, &known);
    overrides.extend(unresolved);

    let mut dependencies = BTreeMap::new();
    dependencies.insert("minecraft".to_string(), inst.mc_version.clone());
    let loader_key = match inst.loader {
        Loader::Vanilla => None,
        Loader::Fabric => Some("fabric-loader"),
        Loader::Quilt => Some("quilt-loader"),
        Loader::Forge => Some("forge"),
        Loader::NeoForge => Some("neoforge"),
    };
    if let Some(key) = loader_key {
        dependencies.insert(key.to_string(), inst.loader_version.clone());
    }

    let index = OutIndex {
        format_version: 1,
        game: "minecraft",
        // A free-form string; a timestamp is honest about what it is, unlike a
        // fake semantic version.
        version_id: instance::now_secs().to_string(),
        name: inst.name.clone(),
        files,
        dependencies,
    };
    let index_bytes = serde_json::to_vec_pretty(&index)?;

    let dest = paths::exports().join(format!("{}.mrpack", inst.id));
    std::fs::create_dir_all(paths::exports())?;
    let write = dest.clone();
    tokio::task::spawn_blocking(move || write_mrpack(&write, &index_bytes, &overrides))
        .await
        .map_err(|e| Error::msg(e.to_string()))??;
    Ok(dest)
}

/// Walk the game folder, splitting it into hashed content candidates and
/// everything-else overrides, with the player's own files left out entirely.
fn collect_exportable(root: &Path) -> Result<(Vec<Hashed>, Vec<(PathBuf, String)>)> {
    let mut hashed = Vec::new();
    let mut overrides = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|_| Error::msg("path escaped the game directory"))?
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");

            if is_own(&relative) {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if is_content_file(&relative) {
                let (sha1, sha512, size) = hash_file(&path)?;
                hashed.push(Hashed { sha1, sha512, path, rel: relative, size });
            } else {
                overrides.push((path, relative));
            }
        }
    }
    Ok((hashed, overrides))
}

/// Both digests the format requires, plus the size, read in chunks rather
/// than into one buffer: a resource pack is routinely hundreds of megabytes.
fn hash_file(path: &Path) -> Result<(String, String, u64)> {
    use sha1::Digest;
    let mut file = std::fs::File::open(path)?;
    let mut one = sha1::Sha1::new();
    let mut many = sha2::Sha512::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        one.update(&buf[..read]);
        many.update(&buf[..read]);
        size += read as u64;
    }
    Ok((hex::encode(one.finalize()), hex::encode(many.finalize()), size))
}

fn write_mrpack(dest: &Path, index: &[u8], overrides: &[(PathBuf, String)]) -> Result<()> {
    use std::io::Write;
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(dest)?);

    zip.start_file(INDEX_NAME, options)?;
    zip.write_all(index)?;
    for (path, relative) in overrides {
        zip.start_file(format!("overrides/{relative}"), options)?;
        let mut file = std::fs::File::open(path)?;
        std::io::copy(&mut file, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
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
pub(crate) fn extract_overrides(archive: &Path, game_dir: &Path, prefixes: &[&str]) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        // `enclosed_name` rejects absolute paths and `..`, so a crafted archive
        // cannot write outside the instance.
        let Some(name) = entry.enclosed_name() else { continue };
        let name = name.to_string_lossy().replace('\\', "/");
        let Some(relative) = prefixes.iter().find_map(|p| name.strip_prefix(p)) else {
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

    /// A pack picked from disk is recognised by its hash, and an old build of
    /// it is offered the newest one: the two answers update offers rest on.
    #[tokio::test]
    #[ignore = "network"]
    async fn an_old_pack_file_is_identified_and_offered_an_update() {
        let all = crate::modrinth::all_versions("fabulously-optimized").await.unwrap();
        let old = all.iter().rev().find(|v| v.version_type == "release").unwrap();
        let file = old.jar().unwrap();
        let path = std::env::temp_dir().join(format!("jl-test-{}.mrpack", old.id));
        download::run_quiet(vec![Job {
            url: file.url.clone(),
            path: path.clone(),
            sha1: file.hashes.sha1.clone(),
            size: Some(file.size),
        }])
        .await
        .unwrap();

        let found = identify(&path).await.expect("not recognised");
        assert_eq!(found.id, old.id);

        let mut inst = instance::tests::sample();
        inst.pack = Some(PackSource {
            project: found.project_id,
            version_id: found.id,
            version_number: found.version_number,
            files: Vec::new(),
        });
        let newer = newer(&inst).await.unwrap().expect("no newer build offered");
        assert_ne!(newer.id, old.id);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_update_removes_only_what_the_new_build_dropped() {
        let list = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let old = list(&["mods/a-1.jar", "mods/b.jar", "resourcepacks/x.zip"]);
        let new = list(&["mods/a-2.jar", "mods/b.jar"]);
        let stale: Vec<_> = stale_files(&old, &new).cloned().collect();
        assert_eq!(stale, list(&["mods/a-1.jar", "resourcepacks/x.zip"]));
    }

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
    fn forge_packs_name_their_loader_by_a_key_of_their_own() {
        for (key, want) in [("forge", Loader::Forge), ("neoforge", Loader::NeoForge)] {
            let (mc, loader, version) =
                loader_of(&deps(&[("minecraft", "1.20.1"), (key, "47.2.0")])).unwrap();
            assert_eq!((mc.as_str(), loader, version.as_str()), ("1.20.1", want, "47.2.0"));
        }
        // A pack naming both is NeoForge's; Forge jars of that era do not load
        // in it, so the more specific key wins.
        let (_, loader, _) = loader_of(&deps(&[
            ("minecraft", "1.20.1"),
            ("forge", "47.2.0"),
            ("neoforge", "47.1.0"),
        ]))
        .unwrap();
        assert_eq!(loader, Loader::NeoForge);
    }

    #[test]
    fn a_pack_without_a_minecraft_version_is_refused() {
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

    #[test]
    fn personal_files_stay_home() {
        for own in [
            "saves/New World/level.dat",
            "screenshots/2026-08-19_12.00.png",
            "logs/latest.log",
            "crash-reports/crash-2026.txt",
            "natives/lwjgl.dll",
            "options.txt",
            "servers.dat",
            "usercache.json",
            "mods/sodium.jar.part",
        ] {
            assert!(is_own(own), "{own} is the player's, not the pack's");
        }
        for packs in ["config/sodium.toml", "mods/sodium.jar", "shaderpacks/BSL/shaders/final.fsh"] {
            assert!(!is_own(packs), "{packs} belongs in the export");
        }
    }

    #[test]
    fn only_the_three_content_folders_feed_the_index() {
        for candidate in ["mods/sodium.jar", "resourcepacks/Faithful.zip", "shaderpacks/BSL.zip"] {
            assert!(is_content_file(candidate), "{candidate}");
        }
        // Nested, unpacked or elsewhere: not index material, override material.
        for plain in [
            "mods/unpacked/dir",
            "config/fabric-api.toml",
            "saves/x/level.dat",
            "shaderpacks/BSL/shaders/final.fsh",
        ] {
            assert!(!is_content_file(plain), "{plain}");
        }
    }

    /// The version Modrinth would answer for a known hash, in the shape the
    /// split has to read.
    fn version_for(sha1: &str, url: &str) -> crate::modrinth::Version {
        crate::modrinth::Version {
            id: "v1".into(),
            project_id: "sodium".into(),
            name: "Sodium 0.6".into(),
            version_number: "0.6.0".into(),
            version_type: "release".into(),
            date_published: String::new(),
            downloads: 0,
            changelog: None,
            dependencies: vec![],
            files: vec![crate::modrinth::VersionFile {
                url: url.into(),
                filename: "sodium.jar".into(),
                hashes: crate::modrinth::Hashes { sha1: Some(sha1.into()) },
                size: 100,
                primary: true,
            }],
        }
    }

    fn hashed(rel: &str, sha1: &str) -> Hashed {
        Hashed {
            path: PathBuf::from(rel),
            rel: rel.into(),
            sha1: sha1.into(),
            sha512: String::new(),
            size: 42,
        }
    }

    #[test]
    fn known_jars_become_entries_and_unknown_become_overrides() {
        let known =
            HashMap::from([("a".to_string(), version_for("a", "https://cdn.modrinth.com/a.jar"))]);
        let (files, overrides) =
            split_known(vec![hashed("mods/known.jar", "a"), hashed("mods/handmade.jar", "b")], &known);

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "mods/known.jar");
        assert_eq!(files[0].downloads, ["https://cdn.modrinth.com/a.jar"]);
        assert_eq!(files[0].file_size, 42);

        // The jar Modrinth has never seen ships in the archive itself.
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].1, "mods/handmade.jar");
    }

    #[test]
    fn an_exported_index_has_every_field_the_format_requires() {
        let index = OutIndex {
            format_version: 1,
            game: "minecraft",
            version_id: "1755859200".into(),
            name: "My Pack".into(),
            files: vec![OutFile {
                path: "mods/sodium.jar".into(),
                hashes: OutHashes {
                    sha1: "a".into(),
                    sha512: "b".into(),
                },
                downloads: vec!["https://cdn.modrinth.com/a.jar".into()],
                file_size: 42,
            }],
            dependencies: BTreeMap::from([
                ("minecraft".to_string(), "1.21.1".to_string()),
                ("fabric-loader".to_string(), "0.16.0".to_string()),
            ]),
        };
        let json = serde_json::to_value(&index).unwrap();
        assert_eq!(json["formatVersion"], 1);
        assert_eq!(json["game"], "minecraft");
        assert_eq!(json["files"][0]["hashes"]["sha1"], "a");
        assert_eq!(json["files"][0]["hashes"]["sha512"], "b");
        assert_eq!(json["files"][0]["fileSize"], 42);
        assert_eq!(json["dependencies"]["fabric-loader"], "0.16.0");

        // What the exporter writes must be what the importer reads.
        let text = serde_json::to_string(&index).unwrap();
        let back: Index = serde_json::from_str(&text).unwrap();
        assert_eq!(back.name, "My Pack");
        assert_eq!(back.files[0].path, "mods/sodium.jar");
        assert_eq!(
            back.dependencies.get("fabric-loader").map(String::as_str),
            Some("0.16.0")
        );
    }
}
