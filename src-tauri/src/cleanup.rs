//! What the shared store still holds that no instance needs any more.
//!
//! Nothing has ever deleted from `shared/` — a removed instance leaves its
//! version, its assets and possibly a whole JRE behind, and an asset index
//! generation goes out of date without anything noticing. The store is the
//! largest thing this launcher writes, so it is the one place worth sweeping.
//!
//! The filesystem stays the database here too: what is *used* is derived from
//! the instances and the version JSONs they name, never from an index of what
//! was downloaded. That is what makes the answer right after a user has copied
//! an instance in by hand.
//!
//! Libraries are deliberately left alone. A Forge instance's patched client
//! jar lives in `libraries/` and is named by nothing that looks like a
//! download — NeoForge's is found by path rather than by classpath entry — so
//! "unreferenced" there does not mean "unused", and getting it wrong costs a
//! several-minute rebuild. Versions, assets and runtimes are where the
//! gigabytes are.
//
// ponytail: libraries/ untouched, see above. Sweep it too once a Forge output
// can be told apart from the tool jars that produced it.

use crate::error::Result;
use crate::mojang::{AssetIndex, VersionJson};
use crate::{instance, paths};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// One removable thing, as the dialog lists it.
#[derive(Serialize)]
pub struct Entry {
    pub name: String,
    pub size: u64,
}

/// Everything a sweep would remove. Sizes are bytes on disk.
#[derive(Serialize, Default)]
pub struct Report {
    /// Version directories — client jar, version json, Forge installer.
    pub versions: Vec<Entry>,
    /// Downloaded Java runtimes no kept version asks for.
    pub runtimes: Vec<Entry>,
    /// Asset objects and index files, counted rather than listed: one unused
    /// index generation is tens of thousands of files.
    pub asset_files: usize,
    pub asset_bytes: u64,
    pub total: u64,
}

/// Bytes under a path, file or directory.
fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if meta.is_file() {
        return meta.len();
    }
    let Ok(entries) = std::fs::read_dir(path) else { return 0 };
    entries.flatten().map(|e| size_of(&e.path())).sum()
}

/// The version ids every instance needs kept: the one it launches, and the
/// vanilla version that one inherits from.
async fn kept_versions() -> HashSet<String> {
    let mut kept = HashSet::new();
    for instance in instance::list().await {
        kept.insert(instance.version_id());
        kept.insert(instance.mc_version.clone());
    }
    kept
}

/// The version JSON cached in a version directory, if it holds a readable one.
fn version_json(dir: &Path, id: &str) -> Option<VersionJson> {
    let text = std::fs::read_to_string(dir.join(format!("{id}.json"))).ok()?;
    serde_json::from_str(&text).ok()
}

/// The asset index id and Java component every kept version still points at.
fn still_pointed_at(kept: &HashSet<String>) -> Option<(HashSet<String>, HashSet<String>)> {
    let mut indexes = HashSet::new();
    let mut runtimes = HashSet::new();
    for id in kept {
        let dir = paths::versions().join(id);
        if !dir.is_dir() {
            continue; // never installed; nothing of it is on disk to keep
        }
        // A kept version whose JSON will not parse cannot answer what it uses,
        // and guessing "nothing" would sweep away assets it needs.
        let version = version_json(&dir, id)?;
        if let Some(index) = &version.asset_index {
            indexes.insert(index.id.clone());
        } else if let Some(assets) = &version.assets {
            indexes.insert(assets.clone());
        }
        if let Some(java) = &version.java_version {
            runtimes.insert(java.component.clone());
        }
    }
    Some((indexes, runtimes))
}

/// Every hash in the indexes named, or `None` if one of them will not parse.
fn objects_named_by(indexes: &HashSet<String>) -> Option<HashSet<String>> {
    let mut used = HashSet::new();
    for id in indexes {
        let path = paths::assets().join("indexes").join(format!("{id}.json"));
        if !path.is_file() {
            continue; // not downloaded yet; it names nothing on disk
        }
        let text = std::fs::read_to_string(&path).ok()?;
        let index: AssetIndex = serde_json::from_str(&text).ok()?;
        used.extend(index.objects.values().map(|o| o.hash.clone()));
    }
    Some(used)
}

/// Every object in the hash store that no kept index names.
fn unused_objects(used: &HashSet<String>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(buckets) = std::fs::read_dir(paths::assets().join("objects")) else { return out };
    for bucket in buckets.flatten() {
        let Ok(objects) = std::fs::read_dir(bucket.path()) else { continue };
        for object in objects.flatten() {
            if !used.contains(&object.file_name().to_string_lossy().into_owned()) {
                out.push(object.path());
            }
        }
    }
    out
}

/// Index files, and the named trees built from them, for generations nothing
/// uses any more.
fn unused_indexes(used: &HashSet<String>) -> Vec<(PathBuf, PathBuf)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(paths::assets().join("indexes")) else { return out };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let id = name.strip_suffix(".json").unwrap_or(&name).to_string();
        if !used.contains(&id) {
            out.push((entry.path(), paths::assets().join("virtual").join(&id)));
        }
    }
    out
}

/// Version directories no instance launches.
fn unused_versions(kept: &HashSet<String>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(paths::versions()) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        // `version_manifest.json` lives here too, and it is not a version.
        if !path.is_dir() {
            continue;
        }
        if !kept.contains(&entry.file_name().to_string_lossy().into_owned()) {
            out.push(path);
        }
    }
    out
}

/// Runtimes, one directory per component under each platform folder.
fn unused_runtimes(used: &HashSet<String>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(platforms) = std::fs::read_dir(paths::java_runtimes()) else { return out };
    for platform in platforms.flatten() {
        let Ok(components) = std::fs::read_dir(platform.path()) else { continue };
        for component in components.flatten() {
            if !used.contains(&component.file_name().to_string_lossy().into_owned()) {
                out.push(component.path());
            }
        }
    }
    out
}

fn name_of(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// What a sweep would remove, without removing anything.
pub async fn scan() -> Result<Report> {
    let kept = kept_versions().await;
    let mut report = Report::default();

    report.versions = unused_versions(&kept)
        .iter()
        .map(|p| Entry { name: name_of(p), size: size_of(p) })
        .collect();

    // Assets and runtimes hang off what the kept versions declare, so an
    // unreadable one stops that half of the sweep rather than guessing.
    if let Some((indexes, runtimes)) = still_pointed_at(&kept) {
        if let Some(objects) = objects_named_by(&indexes) {
            for path in unused_objects(&objects) {
                report.asset_files += 1;
                report.asset_bytes += size_of(&path);
            }
            for (index, tree) in unused_indexes(&indexes) {
                report.asset_files += 1;
                report.asset_bytes += size_of(&index) + size_of(&tree);
            }
        }
        report.runtimes = unused_runtimes(&runtimes)
            .iter()
            .map(|p| Entry { name: name_of(p), size: size_of(p) })
            .collect();
    }

    report.total = report.versions.iter().map(|e| e.size).sum::<u64>()
        + report.runtimes.iter().map(|e| e.size).sum::<u64>()
        + report.asset_bytes;
    Ok(report)
}

/// Sweep, and report what went.
///
/// Works the sets out again rather than taking paths from the caller: the
/// frontend never names a path this deletes, and a list from a dialog left
/// open while an instance was created would delete something in use.
pub async fn clean() -> Result<Report> {
    let report = scan().await?;
    let kept = kept_versions().await;

    for path in unused_versions(&kept) {
        let _ = std::fs::remove_dir_all(path);
    }

    if let Some((indexes, runtimes)) = still_pointed_at(&kept) {
        if let Some(objects) = objects_named_by(&indexes) {
            for path in unused_objects(&objects) {
                let _ = std::fs::remove_file(path);
            }
            for (index, tree) in unused_indexes(&indexes) {
                let _ = std::fs::remove_file(index);
                let _ = std::fs::remove_dir_all(tree);
            }
        }
        for path in unused_runtimes(&runtimes) {
            let _ = std::fs::remove_dir_all(path);
        }
    }

    Ok(report)
}
