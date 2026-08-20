//! Resolving an instance to a concrete version, downloading everything it
//! needs, and unpacking native libraries.

use crate::download::{self, Job};
use crate::error::{Error, Result};
use crate::instance::{Instance, Loader};
use crate::mojang::{self, VersionJson};
use crate::{loader, paths};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

/// The vanilla client jar for a Minecraft version, in the shared store.
pub fn client_jar(mc_version: &str) -> PathBuf {
    paths::versions()
        .join(mc_version)
        .join(format!("{mc_version}.jar"))
}

async fn vanilla_version(mc_version: &str) -> Result<VersionJson> {
    let manifest = mojang::manifest().await?;
    let entry = manifest
        .versions
        .iter()
        .find(|v| v.id == mc_version)
        .ok_or_else(|| Error::msg(format!("Unknown Minecraft version: {mc_version}")))?;
    mojang::version_json(&entry.id, &entry.url).await
}

/// The fully merged version json an instance launches. For a modded instance
/// this is the loader profile stacked on top of the vanilla version it
/// inherits from -- the same shape for Fabric and Quilt.
pub async fn resolve(instance: &Instance) -> Result<VersionJson> {
    let vanilla = vanilla_version(&instance.mc_version).await?;
    if instance.loader == Loader::Vanilla {
        return Ok(vanilla);
    }
    if instance.loader_version.is_empty() {
        return Err(Error::msg(format!(
            "{} loader version not selected yet.",
            instance.loader.label()
        )));
    }
    let child =
        loader::profile(instance.loader, &instance.mc_version, &instance.loader_version).await?;
    Ok(mojang::merge(vanilla, child))
}

/// Classpath entries and native archives for a resolved version.
pub struct Classpath {
    pub jars: Vec<PathBuf>,
    pub native_archives: Vec<(PathBuf, Vec<String>)>,
    pub jobs: Vec<Job>,
}

pub fn classpath(version: &VersionJson, mc_version: &str) -> Result<Classpath> {
    let mut jars = Vec::new();
    let mut native_archives = Vec::new();
    let mut jobs = Vec::new();

    for lib in &version.libraries {
        let Some(resolved) = lib.resolve()? else { continue };
        if let Some(job) = resolved.job {
            jobs.push(job);
        }
        match resolved.natives {
            Some(extract) => native_archives.push((resolved.path, extract.exclude)),
            // Fabric can list the same library twice across merged profiles;
            // a duplicate classpath entry is harmless but wasteful, so skip it.
            None => {
                if !jars.contains(&resolved.path) {
                    jars.push(resolved.path);
                }
            }
        }
    }

    // The client jar goes last so any patched library shadows it.
    let jar = client_jar(mc_version);
    if let Some(client) = version.downloads.get("client") {
        jobs.push(Job {
            url: client.url.clone(),
            path: jar.clone(),
            sha1: client.sha1.clone(),
            size: client.size,
        });
    }
    jars.push(jar);

    Ok(Classpath { jars, native_archives, jobs })
}

/// Unpack native libraries into the instance's natives dir. Done on every
/// install rather than cached, because the directory is cheap to rebuild and a
/// stale one produces UnsatisfiedLinkError crashes that are miserable to debug.
fn extract_natives(archives: &[(PathBuf, Vec<String>)], dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for (archive, exclude) in archives {
        let file = std::fs::File::open(archive)?;
        let mut zip = zip::ZipArchive::new(file)?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            // `enclosed_name` rejects absolute paths and `..` traversal, which
            // a hostile or malformed jar could otherwise use to write anywhere.
            let Some(name) = entry.enclosed_name() else { continue };
            let rel = name.to_string_lossy().replace('\\', "/");
            if entry.is_dir()
                || rel.starts_with("META-INF/")
                || exclude.iter().any(|e| rel.starts_with(e.trim_end_matches('/')))
            {
                continue;
            }
            let out = dest.join(&name);
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut writer = std::fs::File::create(&out)?;
            std::io::copy(&mut entry, &mut writer)?;
        }
    }
    Ok(())
}

/// Materialise the hash-addressed asset store into a tree of real filenames.
///
/// Pre-1.8 clients look assets up by name, not by hash. Files are copied rather
/// than linked because symlinks need elevation on Windows, and a legacy asset
/// set is well under 100 MB.
fn write_named_assets(index: &mojang::AssetIndex, dest: &Path) -> Result<()> {
    for (name, object) in &index.objects {
        let relative = Path::new(name);
        // Names come from a downloaded index, so treat them as untrusted input.
        if relative.is_absolute() || relative.components().any(|c| c.as_os_str() == "..") {
            return Err(Error::msg(format!("unsafe asset name: {name}")));
        }
        let out = dest.join(relative);
        // Same size means the copy is already there; re-copying thousands of
        // files on every launch would be pure waste.
        if std::fs::metadata(&out).is_ok_and(|m| m.len() == object.size) {
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(object.path(), &out)?;
    }
    Ok(())
}

/// Download everything the instance needs and prepare its natives directory.
/// Safe to re-run: existing valid files are skipped.
pub async fn install(app: &AppHandle, instance: &mut Instance) -> Result<VersionJson> {
    if instance.loader != Loader::Vanilla && instance.loader_version.is_empty() {
        instance.loader_version =
            loader::latest_loader(instance.loader, &instance.mc_version).await?;
        instance.save().await?;
    }

    let version = resolve(instance).await?;
    let cp = classpath(&version, &instance.mc_version)?;

    download::run(app, "Libraries", cp.jobs).await?;

    if let Some(index_ref) = &version.asset_index {
        let index = mojang::asset_index(index_ref).await?;
        download::run(app, "Assets", index.jobs()).await?;

        if index.needs_named_copy() {
            let dest = if index.map_to_resources {
                instance.game_dir().join("resources")
            } else {
                paths::assets().join("virtual").join(&index_ref.id)
            };
            // Thousands of small file copies; keep them off the async runtime.
            tokio::task::spawn_blocking(move || write_named_assets(&index, &dest))
                .await
                .map_err(|e| Error::msg(e.to_string()))??;
        }
    }

    let natives_dir = instance.natives_dir();
    let archives = cp.native_archives;
    // Zip extraction is blocking CPU/IO work; keep it off the async runtime.
    tokio::task::spawn_blocking(move || extract_natives(&archives, &natives_dir))
        .await
        .map_err(|e| Error::msg(e.to_string()))??;

    instance.installed = true;
    instance.save().await?;
    Ok(version)
}
