//! Importing a CurseForge modpack: a zip holding `manifest.json` (project and
//! file ids, no URLs) plus an `overrides/` folder.
//!
//! Unlike a Modrinth pack the manifest cannot be downloaded from on its own:
//! every id has to be resolved through the Core API, which needs the user's key
//! from settings. The API's terms forbid caching what it returns, so nothing
//! here is written to disk except the files the pack installs.
//!
//! An author can forbid third-party downloads; the API then answers with a
//! `null` download URL. Those files are not fetched but returned as `Missing`,
//! each with the page the user can download it from by hand.

use crate::download::{self, Job};
use crate::error::{Error, Result};
use crate::instance::{self, Instance, Loader};
use crate::{mods, settings};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use tauri::AppHandle;

const API: &str = "https://api.curseforge.com/v1";
const MANIFEST_NAME: &str = "manifest.json";
/// `hashes[].algo` for SHA-1; 2 is MD5, which the downloader cannot verify.
const ALGO_SHA1: u8 = 1;

#[derive(Deserialize)]
struct Manifest {
    name: String,
    minecraft: ManifestMinecraft,
    #[serde(default)]
    files: Vec<ManifestFile>,
    #[serde(default = "default_overrides")]
    overrides: String,
}

fn default_overrides() -> String {
    "overrides".into()
}

#[derive(Deserialize)]
struct ManifestMinecraft {
    version: String,
    #[serde(rename = "modLoaders", default)]
    mod_loaders: Vec<ModLoader>,
}

#[derive(Deserialize)]
struct ModLoader {
    id: String,
    #[serde(default)]
    primary: bool,
}

#[derive(Deserialize)]
struct ManifestFile {
    #[serde(rename = "projectID")]
    project_id: u64,
    #[serde(rename = "fileID")]
    file_id: u64,
    #[serde(default = "yes")]
    required: bool,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct Data<T> {
    data: T,
}

#[derive(Deserialize)]
struct CfFile {
    id: u64,
    #[serde(rename = "modId")]
    mod_id: u64,
    #[serde(rename = "fileName")]
    file_name: String,
    #[serde(rename = "downloadUrl")]
    download_url: Option<String>,
    #[serde(default)]
    hashes: Vec<CfHash>,
    #[serde(rename = "fileLength")]
    file_length: Option<u64>,
}

#[derive(Deserialize)]
struct CfHash {
    value: String,
    algo: u8,
}

#[derive(Deserialize)]
struct CfMod {
    id: u64,
    #[serde(rename = "classId")]
    class_id: Option<u32>,
    links: CfLinks,
}

#[derive(Deserialize)]
struct CfLinks {
    #[serde(rename = "websiteUrl")]
    website_url: String,
}

/// A file the pack lists but the launcher could not install by itself.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Missing {
    pub file_name: String,
    /// The CurseForge page that offers the file for a manual download.
    pub url: String,
}

/// Whether an archive is a CurseForge pack. Our own exports are zips as well,
/// so the extension cannot tell them apart; the manifest can.
pub fn is_pack(archive: &Path) -> bool {
    std::fs::File::open(archive)
        .ok()
        .and_then(|f| zip::ZipArchive::new(f).ok())
        .is_some_and(|mut z| z.by_name(MANIFEST_NAME).is_ok())
}

/// `fabric-0.19.5`, `forge-47.4.10`, ...: the loader and its build in one id.
fn loader_of(mc: &ManifestMinecraft) -> Result<(Loader, String)> {
    let Some(entry) = mc.mod_loaders.iter().find(|l| l.primary).or(mc.mod_loaders.first()) else {
        return Ok((Loader::Vanilla, String::new()));
    };
    let (name, version) = entry
        .id
        .split_once('-')
        .ok_or_else(|| Error::msg(format!("Pack names an unknown loader: {}", entry.id)))?;
    let loader = match name {
        "fabric" => Loader::Fabric,
        "quilt" => Loader::Quilt,
        "forge" => Loader::Forge,
        "neoforge" => Loader::NeoForge,
        _ => return Err(Error::msg(format!("Pack needs {name}, which this launcher cannot install."))),
    };
    Ok((loader, version.to_string()))
}

/// Where a file lands, from its project's class. `None` is a class the launcher
/// does not install from a pack (a world, a datapack): it goes on the missing
/// list rather than into a folder the game would not read it from.
fn folder_of(class_id: Option<u32>) -> Option<&'static str> {
    match class_id {
        // A project the API did not describe is far most likely a mod.
        Some(6) | None => Some("mods"),
        Some(12) => Some("resourcepacks"),
        Some(6552) => Some("shaderpacks"),
        _ => None,
    }
}

/// Prism's link form: the project page's own download page for that file.
fn page_url(website: &str, file_id: u64) -> String {
    format!("{}/download/{file_id}", website.trim_end_matches('/'))
}

async fn post<T: serde::de::DeserializeOwned>(key: &str, path: &str, body: serde_json::Value) -> Result<T> {
    let request = download::client()?.post(format!("{API}{path}")).header("x-api-key", key).json(&body);
    let reply: Data<T> = download::send(request).await?.json().await?;
    Ok(reply.data)
}

async fn get<T: serde::de::DeserializeOwned>(key: &str, path: &str) -> Result<T> {
    let request = download::client()?.get(format!("{API}{path}")).header("x-api-key", key);
    let reply: Data<T> = download::send(request).await?.json().await?;
    Ok(reply.data)
}

/// The user's key, or `None` when CurseForge is switched off.
pub async fn configured_key() -> Option<String> {
    let key = settings::load().await.curseforge_api_key.trim().to_string();
    (!key.is_empty()).then_some(key)
}

async fn key() -> Result<String> {
    configured_key()
        .await
        .ok_or_else(|| Error::msg("Add a CurseForge API key in Settings to use CurseForge."))
}

// ------------------------------------------------------------------- browsing
//
// Everything below answers in Modrinth's shapes (`modrinth::Hit`, `Project`,
// `Version`) with CurseForge's numeric ids as strings, so the browser and the
// install loop need no second set of types.

use crate::modrinth;
use crate::mods::Kind;

const GAME_MINECRAFT: u32 = 432;
const PAGE_SIZE: u32 = 20;
/// `relationType` of a dependency the mod cannot run without.
const RELATION_REQUIRED: u8 = 3;
/// The API refuses a page reaching past this many results.
const MAX_REACH: u32 = 10_000;

#[derive(Deserialize)]
struct Paged<T> {
    data: T,
    pagination: Pagination,
}

#[derive(Deserialize)]
struct Pagination {
    #[serde(rename = "totalCount")]
    total_count: u64,
}

#[derive(Deserialize)]
struct Project {
    id: u64,
    slug: String,
    name: String,
    summary: String,
    #[serde(rename = "downloadCount")]
    download_count: f64,
    #[serde(rename = "thumbsUpCount", default)]
    thumbs_up: u64,
    #[serde(default)]
    authors: Vec<Named>,
    #[serde(default)]
    categories: Vec<Named>,
    logo: Option<Logo>,
    links: ProjectLinks,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
struct Logo {
    #[serde(rename = "thumbnailUrl")]
    thumbnail_url: Option<String>,
}

#[derive(Deserialize)]
struct ProjectLinks {
    #[serde(rename = "websiteUrl")]
    website_url: String,
    #[serde(rename = "sourceUrl")]
    source_url: Option<String>,
    #[serde(rename = "issuesUrl")]
    issues_url: Option<String>,
    #[serde(rename = "wikiUrl")]
    wiki_url: Option<String>,
}

#[derive(Deserialize)]
struct FileDetail {
    id: u64,
    #[serde(rename = "modId")]
    mod_id: u64,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "fileName")]
    file_name: String,
    #[serde(rename = "releaseType")]
    release_type: u8,
    #[serde(rename = "fileDate")]
    file_date: String,
    #[serde(rename = "downloadCount", default)]
    download_count: u64,
    #[serde(rename = "downloadUrl")]
    download_url: Option<String>,
    #[serde(default)]
    hashes: Vec<CfHash>,
    #[serde(rename = "fileLength")]
    file_length: u64,
    #[serde(default)]
    dependencies: Vec<FileDependency>,
}

#[derive(Deserialize)]
struct FileDependency {
    #[serde(rename = "modId")]
    mod_id: u64,
    #[serde(rename = "relationType")]
    relation_type: u8,
}

/// CurseForge's class for each content folder.
fn class_id(kind: Kind) -> u32 {
    match kind {
        Kind::Mods => 6,
        Kind::Resourcepacks => 12,
        Kind::Shaderpacks => 6552,
    }
}

/// `ModLoaderType` values a mod build may carry for this instance. A Quilt
/// instance asks for Fabric builds too, as it does on Modrinth.
fn loader_types(kind: Kind, loader: Loader) -> &'static [u8] {
    if kind != Kind::Mods {
        return &[];
    }
    match loader {
        Loader::Forge => &[1],
        Loader::Fabric => &[4],
        Loader::Quilt => &[5, 4],
        Loader::NeoForge => &[6],
        Loader::Vanilla => &[],
    }
}

/// Modrinth's sort names onto `ModsSearchSortField`. CurseForge has no
/// relevance index or follower count; popularity stands in for both.
fn sort_field(sort: &str) -> u8 {
    match sort {
        "downloads" => 6,
        "newest" => 11,
        "updated" => 3,
        _ => 2,
    }
}

fn version_type(release_type: u8) -> &'static str {
    match release_type {
        2 => "beta",
        3 => "alpha",
        _ => "release",
    }
}

fn hit(p: Project) -> modrinth::Hit {
    modrinth::Hit {
        project_id: p.id.to_string(),
        slug: p.slug,
        title: p.name,
        description: p.summary,
        author: p.authors.into_iter().next().map(|a| a.name).unwrap_or_default(),
        downloads: p.download_count as u64,
        follows: p.thumbs_up,
        categories: p.categories.into_iter().map(|c| c.name).collect(),
        icon_url: p.logo.and_then(|l| l.thumbnail_url),
    }
}

/// A file as a one-file Modrinth version. A file whose author forbids
/// third-party downloads keeps an empty URL; installing reads that as "send the
/// user to the page".
fn version(f: FileDetail) -> modrinth::Version {
    let sha1 = f.hashes.iter().find(|h| h.algo == ALGO_SHA1).map(|h| h.value.clone());
    modrinth::Version {
        id: f.id.to_string(),
        project_id: f.mod_id.to_string(),
        version_number: f.display_name.clone(),
        name: f.display_name,
        version_type: version_type(f.release_type).into(),
        date_published: f.file_date,
        downloads: f.download_count,
        changelog: None,
        dependencies: f
            .dependencies
            .into_iter()
            .map(|d| modrinth::Dependency {
                project_id: Some(d.mod_id.to_string()),
                version_id: None,
                dependency_type: if d.relation_type == RELATION_REQUIRED { "required" } else { "other" }.into(),
            })
            .collect(),
        files: vec![modrinth::VersionFile {
            url: f.download_url.unwrap_or_default(),
            filename: f.file_name,
            hashes: modrinth::Hashes { sha1 },
            size: f.file_length,
            primary: true,
        }],
    }
}

fn query_list(values: &[u8]) -> String {
    let items: Vec<String> = values.iter().map(u8::to_string).collect();
    format!("%5B{}%5D", items.join(","))
}

pub async fn search(
    query: &str,
    mc_version: &str,
    sort: &str,
    category: Option<&str>,
    offset: u32,
    kind: Kind,
    loader: Loader,
) -> Result<modrinth::SearchPage> {
    let key = key().await?;
    let mut path = format!(
        "/mods/search?gameId={GAME_MINECRAFT}&classId={}&gameVersion={}&sortField={}&sortOrder=desc&pageSize={PAGE_SIZE}&index={offset}&searchFilter={}",
        class_id(kind),
        modrinth::urlencode(mc_version),
        sort_field(sort),
        modrinth::urlencode(query),
    );
    let loaders = loader_types(kind, loader);
    if !loaders.is_empty() {
        path.push_str(&format!("&modLoaderTypes={}", query_list(loaders)));
    }
    if let Some(category) = category.filter(|c| !c.is_empty()) {
        path.push_str(&format!("&categoryIds=%5B{}%5D", numeric(category)?));
    }
    // Past the API's reach there is nothing more to ask for; say so by
    // reporting the total as what was already held.
    if offset + PAGE_SIZE > MAX_REACH {
        return Ok(modrinth::SearchPage { hits: Vec::new(), total_hits: offset as u64 });
    }
    let page: Paged<Vec<Project>> = {
        let request = download::client()?.get(format!("{API}{path}")).header("x-api-key", &key);
        download::send(request).await?.json().await?
    };
    Ok(modrinth::SearchPage {
        hits: page.data.into_iter().map(hit).collect(),
        total_hits: page.pagination.total_count.min(MAX_REACH as u64),
    })
}

pub async fn project(id: &str) -> Result<modrinth::Project> {
    let key = key().await?;
    let id = numeric(id)?;
    let p: Project = get(&key, &format!("/mods/{id}")).await?;
    // HTML, not markdown; the renderer passes it through the same sanitiser.
    let body: String = get(&key, &format!("/mods/{id}/description")).await.unwrap_or_default();
    Ok(modrinth::Project {
        id: p.id.to_string(),
        slug: p.slug,
        title: p.name,
        description: p.summary,
        body,
        downloads: p.download_count as u64,
        followers: p.thumbs_up,
        categories: p.categories.into_iter().map(|c| c.name).collect(),
        icon_url: p.logo.and_then(|l| l.thumbnail_url),
        source_url: p.links.source_url,
        issues_url: p.links.issues_url,
        wiki_url: p.links.wiki_url,
        page_url: Some(p.links.website_url),
    })
}

/// One of CurseForge's categories, as the browser's picker lists it.
#[derive(Serialize, Debug, PartialEq)]
pub struct Category {
    /// Sent back as the search filter.
    pub id: String,
    /// A sub-category carries its parent's name: "Addons: Create".
    pub name: String,
}

#[derive(Deserialize)]
struct RawCategory {
    id: u64,
    name: String,
    #[serde(rename = "parentCategoryId")]
    parent: Option<u64>,
}

/// The categories of one class, fetched each time: the terms forbid caching.
pub async fn categories(kind: Kind) -> Result<Vec<Category>> {
    let key = key().await?;
    let raw: Vec<RawCategory> = get(&key, &format!("/categories?gameId={GAME_MINECRAFT}&classId={}", class_id(kind))).await?;
    Ok(named_categories(raw, class_id(kind) as u64))
}

/// A top-level category's parent is the class itself; any other parent is a
/// category, whose name prefixes the child's so "Create" reads as an addon.
fn named_categories(raw: Vec<RawCategory>, class: u64) -> Vec<Category> {
    let names: HashMap<u64, String> = raw.iter().map(|c| (c.id, c.name.clone())).collect();
    let mut out: Vec<Category> = raw
        .into_iter()
        .map(|c| Category {
            id: c.id.to_string(),
            name: match c.parent.filter(|p| *p != class).and_then(|p| names.get(&p)) {
                Some(parent) => format!("{parent}: {}", c.name),
                None => c.name,
            },
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// The project's page, for a file the user has to download by hand.
pub async fn page(project: &str) -> Result<String> {
    let key = key().await?;
    let p: Project = get(&key, &format!("/mods/{}", numeric(project)?)).await?;
    Ok(p.links.website_url)
}

/// Every file of a project for one Minecraft version and loader, newest first.
pub async fn versions(project: &str, mc_version: &str, kind: Kind, loader: Loader) -> Result<Vec<modrinth::Version>> {
    let key = key().await?;
    let mut path = format!(
        "/mods/{}/files?gameVersion={}&pageSize=50",
        numeric(project)?,
        modrinth::urlencode(mc_version)
    );
    // This endpoint takes one loader, not a list; a Quilt instance gets Quilt
    // builds here and Fabric ones only through search.
    // ponytail: first loader only, merge two requests if Quilt users miss Fabric builds
    if let Some(l) = loader_types(kind, loader).first() {
        path.push_str(&format!("&modLoaderType={l}"));
    }
    let files: Vec<FileDetail> = get(&key, &path).await?;
    Ok(files.into_iter().map(version).collect())
}

pub async fn latest_version(project: &str, mc_version: &str, kind: Kind, loader: Loader) -> Result<modrinth::Version> {
    let all = versions(project, mc_version, kind, loader).await?;
    all.iter()
        .find(|v| v.version_type == "release")
        .or_else(|| all.first())
        .cloned()
        .ok_or_else(|| Error::msg(format!("This has no build for Minecraft {mc_version} on CurseForge.")))
}

pub async fn file(project: &str, file_id: &str) -> Result<modrinth::Version> {
    let key = key().await?;
    let f: FileDetail = get(&key, &format!("/mods/{}/files/{}", numeric(project)?, numeric(file_id)?)).await?;
    Ok(version(f))
}

/// Ids arrive from the frontend and go into a URL path.
fn numeric(id: &str) -> Result<u64> {
    id.parse().map_err(|_| Error::msg(format!("Not a CurseForge id: {id}")))
}

// -------------------------------------------------------------- fingerprints

/// CurseForge's file fingerprint: MurmurHash2, seed 1, over the file with
/// tab, LF, CR and space bytes removed. Verified against `fileFingerprint` of
/// a real jar.
pub fn fingerprint(bytes: &[u8]) -> u32 {
    const M: u32 = 0x5bd1_e995;
    let kept = |b: &&u8| !matches!(**b, 9 | 10 | 13 | 32);
    let len = bytes.iter().filter(kept).count() as u32;
    let mut h = 1 ^ len;
    let mut word = [0u8; 4];
    let mut fill = 0;
    for &b in bytes.iter().filter(kept) {
        word[fill] = b;
        fill += 1;
        if fill == 4 {
            let mut k = u32::from_le_bytes(word).wrapping_mul(M);
            k ^= k >> 24;
            h = h.wrapping_mul(M) ^ k.wrapping_mul(M);
            fill = 0;
        }
    }
    if fill > 0 {
        if fill == 3 {
            h ^= (word[2] as u32) << 16;
        }
        if fill >= 2 {
            h ^= (word[1] as u32) << 8;
        }
        h ^= word[0] as u32;
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^ (h >> 15)
}

#[derive(Deserialize)]
struct Matches {
    #[serde(rename = "exactMatches", default)]
    exact_matches: Vec<Match>,
}

#[derive(Deserialize)]
struct Match {
    id: u64,
    file: MatchFile,
}

#[derive(Deserialize)]
struct MatchFile {
    #[serde(rename = "fileFingerprint")]
    file_fingerprint: u32,
}

/// Fingerprint -> CurseForge project id, for the fingerprints it recognises.
pub async fn identify(key: &str, fingerprints: &[u32]) -> Result<HashMap<u32, String>> {
    if fingerprints.is_empty() {
        return Ok(HashMap::new());
    }
    let found: Matches = post(
        key,
        &format!("/fingerprints/{GAME_MINECRAFT}"),
        serde_json::json!({ "fingerprints": fingerprints }),
    )
    .await?;
    Ok(found.exact_matches.into_iter().map(|m| (m.file.file_fingerprint, m.id.to_string())).collect())
}

/// Extract the pack into a new instance and download everything it lists that
/// CurseForge allows. Returns what it could not fetch.
pub async fn import(app: &AppHandle, archive: &Path) -> Result<(Instance, Vec<Missing>)> {
    let key = settings::load().await.curseforge_api_key;
    if key.trim().is_empty() {
        return Err(Error::msg("This is a CurseForge pack. Add a CurseForge API key in Settings to import it."));
    }

    let path = archive.to_path_buf();
    let manifest = tokio::task::spawn_blocking({
        let path = path.clone();
        move || read_manifest(&path)
    })
    .await
    .map_err(|e| Error::msg(e.to_string()))??;

    let (loader, loader_version) = loader_of(&manifest.minecraft)?;
    let inst = instance::create(&manifest.name, &manifest.minecraft.version, loader, &loader_version).await?;

    // A failure part-way leaves something that looks playable and is not.
    match fill(app, &inst, &manifest, &path, key.trim()).await {
        Ok(missing) => Ok((inst, missing)),
        Err(e) => {
            let _ = instance::delete(&inst.id).await;
            Err(e)
        }
    }
}

async fn fill(app: &AppHandle, inst: &Instance, manifest: &Manifest, archive: &Path, key: &str) -> Result<Vec<Missing>> {
    let (jobs, missing) = resolve(key, manifest, &inst.game_dir()).await?;

    let archive = archive.to_path_buf();
    let dest = inst.game_dir();
    let prefix = format!("{}/", manifest.overrides.trim_end_matches('/'));
    tokio::task::spawn_blocking(move || crate::mrpack::extract_overrides(&archive, &dest, &[prefix.as_str()]))
        .await
        .map_err(|e| Error::msg(e.to_string()))??;

    download::run(app, "Pack files", jobs).await?;
    Ok(missing)
}

/// Ask the API what each listed file is and where it comes from.
async fn resolve(key: &str, manifest: &Manifest, game_dir: &Path) -> Result<(Vec<Job>, Vec<Missing>)> {
    let wanted: Vec<&ManifestFile> = manifest.files.iter().filter(|f| f.required).collect();
    let file_ids: Vec<u64> = wanted.iter().map(|f| f.file_id).collect();
    let mod_ids: Vec<u64> = wanted.iter().map(|f| f.project_id).collect();

    let files: Vec<CfFile> = post(key, "/mods/files", serde_json::json!({ "fileIds": file_ids })).await?;
    let projects: Vec<CfMod> = post(key, "/mods", serde_json::json!({ "modIds": mod_ids })).await?;
    let projects: HashMap<u64, CfMod> = projects.into_iter().map(|m| (m.id, m)).collect();

    if files.len() != wanted.len() {
        return Err(Error::msg(format!(
            "CurseForge knows {} of the {} files this pack lists.",
            files.len(),
            wanted.len()
        )));
    }

    plan(game_dir, files, &projects)
}

/// Split resolved files into downloads and the ones the user has to fetch.
fn plan(game_dir: &Path, files: Vec<CfFile>, projects: &HashMap<u64, CfMod>) -> Result<(Vec<Job>, Vec<Missing>)> {
    let mut jobs = Vec::new();
    let mut missing = Vec::new();
    for file in files {
        let project = projects.get(&file.mod_id);
        let folder = folder_of(project.and_then(|p| p.class_id));
        match (&file.download_url, folder) {
            (Some(url), Some(folder)) => {
                // The API chooses both the URL and the file name.
                if !url.starts_with("https://") {
                    return Err(Error::msg(format!("Pack wants to download {url} over plain HTTP.")));
                }
                jobs.push(Job {
                    url: url.clone(),
                    path: game_dir.join(folder).join(mods::checked_name(&file.file_name)?),
                    sha1: file.hashes.iter().find(|h| h.algo == ALGO_SHA1).map(|h| h.value.clone()),
                    size: file.file_length,
                });
            }
            _ => missing.push(Missing {
                url: project
                    .map(|p| page_url(&p.links.website_url, file.id))
                    .unwrap_or_else(|| format!("https://www.curseforge.com/projects/{}", file.mod_id)),
                file_name: file.file_name,
            }),
        }
    }
    Ok((jobs, missing))
}

fn read_manifest(archive: &Path) -> Result<Manifest> {
    let file = std::fs::File::open(archive)
        .map_err(|e| Error::msg(format!("Cannot open {}: {e}", archive.display())))?;
    let mut zip = zip::ZipArchive::new(file)?;
    let mut text = String::new();
    zip.by_name(MANIFEST_NAME)
        .map_err(|_| Error::msg("That file is not a CurseForge pack (no manifest.json)."))?
        .read_to_string(&mut text)?;
    serde_json::from_str(&text).map_err(|e| Error::msg(format!("Pack manifest is invalid: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// From a published pack (VerityPack, 1.20.1 Forge), trimmed to two files.
    const REAL_MANIFEST: &str = r#"{
      "minecraft": { "version": "1.20.1", "modLoaders": [ { "id": "forge-47.4.10", "primary": true } ] },
      "manifestType": "minecraftModpack", "manifestVersion": 1,
      "name": "VerityPack", "version": "1.20.1", "author": "", "overrides": "overrides",
      "files": [
        { "projectID": 453763, "fileID": 7670377, "required": true, "isLocked": false },
        { "projectID": 429235, "fileID": 4810975, "required": false, "isLocked": false }
      ]
    }"#;

    /// `POST /v1/mods/files` and `POST /v1/mods` in the shape the live API
    /// answered, trimmed to the fields read; names and hashes are stand-ins.
    /// The second file's author forbids third-party downloads, which the API
    /// reports as a `null` URL.
    const REAL_FILES: &str = r#"{"data":[
      {"id":7670377,"modId":453763,"fileName":"jei-1.20.1-forge.jar",
       "downloadUrl":"https://edge.forgecdn.net/files/7670/377/jei-1.20.1-forge.jar",
       "hashes":[{"value":"2f9f989ce2a4c17c61f2868bbc007c2e11232f15","algo":1},{"value":"3a56079331bb69269acc9dc7714f4739","algo":2}],
       "fileLength":1234},
      {"id":4810975,"modId":429235,"fileName":"FreshAnimations_v1.10.4.zip","downloadUrl":null,
       "hashes":[{"value":"aa","algo":1}],"fileLength":99}
    ]}"#;
    const REAL_MODS: &str = r#"{"data":[
      {"id":453763,"classId":6,"links":{"websiteUrl":"https://www.curseforge.com/minecraft/mc-mods/jei"}},
      {"id":429235,"classId":12,"links":{"websiteUrl":"https://www.curseforge.com/minecraft/texture-packs/fresh-animations"}}
    ]}"#;

    #[test]
    fn a_real_manifest_parses() {
        let m: Manifest = serde_json::from_str(REAL_MANIFEST).unwrap();
        assert_eq!(m.name, "VerityPack");
        assert_eq!(m.minecraft.version, "1.20.1");
        assert_eq!(loader_of(&m.minecraft).unwrap(), (Loader::Forge, "47.4.10".into()));
        assert_eq!(m.overrides, "overrides");
        assert!(m.files[0].required && !m.files[1].required);
    }

    #[test]
    fn loader_ids_split_on_the_first_dash() {
        let mc = |id: &str| ManifestMinecraft {
            version: "1.21.1".into(),
            mod_loaders: vec![ModLoader { id: id.into(), primary: true }],
        };
        assert_eq!(loader_of(&mc("fabric-0.19.5")).unwrap(), (Loader::Fabric, "0.19.5".into()));
        assert_eq!(loader_of(&mc("neoforge-21.1.248")).unwrap(), (Loader::NeoForge, "21.1.248".into()));
        assert_eq!(loader_of(&mc("quilt-0.24.0-beta.1")).unwrap(), (Loader::Quilt, "0.24.0-beta.1".into()));
        assert!(loader_of(&mc("liteloader-1.12")).is_err());
        let none = ManifestMinecraft { version: "1.21.1".into(), mod_loaders: vec![] };
        assert_eq!(loader_of(&none).unwrap(), (Loader::Vanilla, String::new()));
    }

    #[test]
    fn blocked_files_are_returned_with_their_page_not_downloaded() {
        let files: Data<Vec<CfFile>> = serde_json::from_str(REAL_FILES).unwrap();
        let mods: Data<Vec<CfMod>> = serde_json::from_str(REAL_MODS).unwrap();
        let mods = mods.data.into_iter().map(|m| (m.id, m)).collect();
        let (jobs, missing) = plan(Path::new("/game"), files.data, &mods).unwrap();

        assert_eq!(jobs.len(), 1);
        assert!(jobs[0].path.ends_with("mods/jei-1.20.1-forge.jar"));
        assert_eq!(jobs[0].sha1.as_deref(), Some("2f9f989ce2a4c17c61f2868bbc007c2e11232f15"));
        assert_eq!(jobs[0].size, Some(1234));

        assert_eq!(
            missing,
            [Missing {
                file_name: "FreshAnimations_v1.10.4.zip".into(),
                url: "https://www.curseforge.com/minecraft/texture-packs/fresh-animations/download/4810975".into(),
            }]
        );
    }

    #[test]
    fn classes_map_to_the_folder_the_game_reads() {
        assert_eq!(folder_of(Some(6)), Some("mods"));
        assert_eq!(folder_of(Some(12)), Some("resourcepacks"));
        assert_eq!(folder_of(Some(6552)), Some("shaderpacks"));
        assert_eq!(folder_of(Some(17)), None);
    }

    /// Resolves and downloads a real pack against the live API:
    /// `CF_API_KEY=... CF_PACK=path/to/pack.zip cargo test curseforge::tests::live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_pack_resolves_and_downloads() {
        let key = std::env::var("CF_API_KEY").expect("CF_API_KEY");
        let manifest = read_manifest(Path::new(&std::env::var("CF_PACK").expect("CF_PACK"))).unwrap();
        let dir = std::env::temp_dir().join("jl-cf-live");
        let _ = std::fs::remove_dir_all(&dir);

        let (jobs, missing) = resolve(&key, &manifest, &dir).await.unwrap();
        println!("{} downloads, missing: {missing:?}", jobs.len());
        assert!(!jobs.is_empty());
        assert!(jobs.iter().all(|j| j.sha1.is_some()));
        download::run_quiet(jobs.clone()).await.unwrap();
        assert!(jobs.iter().all(|j| j.path.is_file()));
    }

    /// Values from a reference implementation that reproduced the API's
    /// `fileFingerprint` for a real jar (sodium-fabric-0.8.13+mc1.21.1 ->
    /// 254184734); every tail length is covered.
    #[test]
    fn fingerprints_match_curseforge() {
        for (input, want) in [
            (&b""[..], 1540447798u32),
            (b"a", 626045324),
            (b"ab", 1692487918),
            (b"abc", 1621425345),
            (b"hello world", 2824650221),
        ] {
            assert_eq!(fingerprint(input), want, "{input:?}");
        }
        // Whitespace is not part of the fingerprint.
        assert_eq!(fingerprint(b"hello\r\n\tworld  !"), fingerprint(b"helloworld!"));
    }

    /// The search reply as the live API sent it for "sodium", trimmed.
    #[test]
    fn a_search_hit_reads_as_a_modrinth_hit() {
        let page: Paged<Vec<Project>> = serde_json::from_str(r#"{"data":[{"id":394468,"slug":"sodium",
          "name":"Sodium","summary":"The fastest rendering optimization mod.","downloadCount":155581098.0,
          "thumbsUpCount":0,"authors":[{"id":28746583,"name":"JellySquid"}],
          "categories":[{"id":6814,"name":"Performance"}],
          "logo":{"thumbnailUrl":"https://media.forgecdn.net/avatars/thumbnails/284/773/256/256/637298471098686391.png"},
          "links":{"websiteUrl":"https://www.curseforge.com/minecraft/mc-mods/sodium","wikiUrl":null,"issuesUrl":null,"sourceUrl":"https://github.com/CaffeineMC/sodium"}}],
          "pagination":{"index":0,"pageSize":1,"resultCount":1,"totalCount":50}}"#).unwrap();
        assert_eq!(page.pagination.total_count, 50);
        let h = hit(page.data.into_iter().next().unwrap());
        assert_eq!((h.project_id.as_str(), h.author.as_str(), h.downloads), ("394468", "JellySquid", 155581098));
        assert_eq!(h.categories, ["Performance"]);
    }

    /// A file from `GET /mods/308702/files` (Mod Menu) in the live reply's
    /// shape; one dependency is made optional to prove the filter.
    #[test]
    fn a_file_reads_as_a_version_with_its_required_dependencies() {
        let f: FileDetail = serde_json::from_str(r#"{"id":8966434,"modId":308702,"displayName":"Mod Menu 11.0.5",
          "fileName":"modmenu-11.0.5.jar","releaseType":1,"fileDate":"2026-09-01T00:00:00Z","downloadCount":0,
          "downloadUrl":"https://edge.forgecdn.net/files/8966/434/modmenu-11.0.5.jar",
          "hashes":[{"value":"aa","algo":2},{"value":"bb","algo":1}],"fileLength":10,
          "dependencies":[{"modId":1037459,"relationType":3},{"modId":306612,"relationType":2}]}"#).unwrap();
        let v = version(f);
        assert_eq!((v.id.as_str(), v.project_id.as_str(), v.version_type.as_str()), ("8966434", "308702", "release"));
        assert_eq!(v.jar().unwrap().hashes.sha1.as_deref(), Some("bb"));
        let required: Vec<_> = v.dependencies.iter().filter(|d| d.dependency_type == "required").collect();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0].project_id.as_deref(), Some("1037459"));
    }

    /// Browsing against the live API. Reads the key from settings, so point
    /// `JUSTLAUNCHER_HOME` at a scratch root whose settings.json holds one.
    #[tokio::test]
    #[ignore]
    async fn live_browse_resolves_a_mod_and_its_dependency() {
        let page = search("mod menu", "1.21.1", "relevance", None, 0, Kind::Mods, Loader::Fabric).await.unwrap();
        assert!(page.total_hits > 0);
        let performance = categories(Kind::Mods).await.unwrap().into_iter().find(|c| c.name == "Performance").unwrap();
        let narrowed = search("", "1.21.1", "downloads", Some(&performance.id), 0, Kind::Mods, Loader::Fabric).await.unwrap();
        assert!(narrowed.total_hits > 0 && narrowed.hits.iter().all(|h| h.categories.iter().any(|c| c == "Performance")));
        let menu = page.hits.iter().find(|h| h.slug == "modmenu").expect("Mod Menu in results");

        let project = project(&menu.project_id).await.unwrap();
        assert!(project.body.contains('<'), "description is HTML");
        assert!(project.page_url.unwrap().starts_with("https://www.curseforge.com/"));

        let latest = latest_version(&menu.project_id, "1.21.1", Kind::Mods, Loader::Fabric).await.unwrap();
        let jar = latest.jar().unwrap();
        assert!(jar.url.starts_with("https://") && jar.hashes.sha1.is_some());
        let dep = latest.dependencies.iter().find(|d| d.dependency_type == "required").expect("needs Fabric API");
        let dep = latest_version(dep.project_id.as_deref().unwrap(), "1.21.1", Kind::Mods, Loader::Fabric).await.unwrap();
        println!("{} -> needs {}", jar.filename, dep.jar().unwrap().filename);

        // Download it and find it again by fingerprint, the way the installed
        // list labels a file.
        let path = std::env::temp_dir().join("jl-cf-live-menu.jar");
        let _ = std::fs::remove_file(&path);
        download::fetch(&jar.url, &path).await.unwrap();
        let print = fingerprint(&std::fs::read(&path).unwrap());
        let known = identify(&key().await.unwrap(), &[print]).await.unwrap();
        assert_eq!(known.get(&print), Some(&menu.project_id));
    }

    /// Entries from the live `/categories?classId=6` reply.
    #[test]
    fn sub_categories_carry_their_parent_name() {
        let raw: Vec<RawCategory> = serde_json::from_str(r#"[
          {"id":434,"name":"Armor, Tools, and Weapons","parentCategoryId":6},
          {"id":6484,"name":"Create","parentCategoryId":426},
          {"id":426,"name":"Addons","parentCategoryId":6}]"#).unwrap();
        let names: Vec<String> = named_categories(raw, 6).into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["Addons", "Addons: Create", "Armor, Tools, and Weapons"]);
    }

    #[test]
    fn an_api_file_name_cannot_escape_its_folder() {
        let files = vec![CfFile {
            id: 1,
            mod_id: 1,
            file_name: "../../evil.jar".into(),
            download_url: Some("https://edge.forgecdn.net/x".into()),
            hashes: vec![],
            file_length: None,
        }];
        assert!(plan(Path::new("/game"), files, &HashMap::new()).is_err());
    }
}
