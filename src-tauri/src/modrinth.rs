//! Searching Modrinth, resolving a project to a downloadable jar, and asking
//! which installed jars have newer builds.
//!
//! Read-only and unauthenticated: the public v2 API needs no key. CurseForge
//! is deliberately absent -- it requires a per-launcher API key and forbids
//! third-party downloads for some projects, so it cannot be a drop-in second
//! provider. See README.

use crate::download;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const API: &str = "https://api.modrinth.com/v2";

#[derive(Deserialize, Serialize)]
pub struct SearchPage {
    pub hits: Vec<Hit>,
    pub total_hits: u64,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Hit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub downloads: u64,
    pub follows: u64,
    pub categories: Vec<String>,
    pub icon_url: Option<String>,
}

/// Everything the detail pane shows. `body` is Modrinth-flavoured markdown and
/// is passed through untouched -- the UI renders it as plain text rather than
/// pulling in a markdown parser.
#[derive(Deserialize, Serialize, Clone)]
pub struct Project {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub body: String,
    pub downloads: u64,
    pub followers: u64,
    pub categories: Vec<String>,
    pub icon_url: Option<String>,
    pub source_url: Option<String>,
    pub issues_url: Option<String>,
    pub wiki_url: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Version {
    pub id: String,
    /// The project this build belongs to; what identifies a mod across renames.
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    /// "release", "beta" or "alpha".
    pub version_type: String,
    pub date_published: String,
    pub downloads: u64,
    /// Release notes, markdown, often absent.
    #[serde(default)]
    pub changelog: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    pub files: Vec<VersionFile>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Dependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    /// "required", "optional", "incompatible" or "embedded".
    pub dependency_type: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct VersionFile {
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub hashes: Hashes,
    pub size: u64,
    pub primary: bool,
}

#[derive(Deserialize, Serialize, Clone, Default)]
pub struct Hashes {
    pub sha1: Option<String>,
}

impl Version {
    /// The mod jar. Sources and javadoc archives ride along in `files`, and
    /// exactly one entry is flagged primary; older versions flag none, so fall
    /// back to the first file rather than failing.
    pub fn jar(&self) -> Option<&VersionFile> {
        self.files.iter().find(|f| f.primary).or_else(|| self.files.first())
    }
}

/// One page of Fabric mods for a Minecraft version.
///
/// `sort` is Modrinth's search index: relevance, downloads, follows, newest or
/// updated. `category` narrows to one of the tag names from `/tag/category`.
pub async fn search(
    query: &str,
    mc_version: &str,
    sort: &str,
    category: Option<&str>,
    offset: u32,
    limit: u32,
) -> Result<SearchPage> {
    let mut facets = vec![
        r#"["project_type:mod"]"#.to_string(),
        r#"["categories:fabric"]"#.to_string(),
        format!(r#"["versions:{mc_version}"]"#),
    ];
    if let Some(category) = category.filter(|c| !c.is_empty()) {
        facets.push(format!(r#"["categories:{category}"]"#));
    }
    let url = format!(
        "{API}/search?limit={limit}&offset={offset}&index={}&query={}&facets={}",
        urlencode(sort),
        urlencode(query),
        urlencode(&format!("[{}]", facets.join(",")))
    );
    download::json(&url).await
}

pub async fn project(id: &str) -> Result<Project> {
    download::json(&format!("{API}/project/{}", urlencode(id))).await
}

/// Every Fabric release of a project for one Minecraft version, newest first.
pub async fn versions(project: &str, mc_version: &str) -> Result<Vec<Version>> {
    let url = format!(
        "{API}/project/{}/version?loaders=%5B%22fabric%22%5D&game_versions=%5B%22{mc_version}%22%5D",
        urlencode(project)
    );
    download::json(&url).await
}

/// The newest version of a project for this Minecraft version, preferring a
/// full release over a beta or alpha.
pub async fn latest_version(project: &str, mc_version: &str) -> Result<Version> {
    let versions = versions(project, mc_version).await?;
    versions
        .iter()
        .find(|v| v.version_type == "release")
        .or_else(|| versions.first())
        .cloned()
        .ok_or_else(|| {
            Error::msg(format!("This mod has no Fabric release for Minecraft {mc_version}."))
        })
}

pub async fn version(id: &str) -> Result<Version> {
    download::json(&format!("{API}/version/{}", urlencode(id))).await
}

/// Identify installed jars by their SHA-1: hash -> the version it is, which
/// carries the project it belongs to. Hashes Modrinth does not know are absent
/// from the map.
///
/// This is the only reliable way to tell whether a project is already
/// installed. Names cannot do it -- Modrinth lists Jade as "Jade 🔍" while the
/// jar calls itself "Jade", and a mod is free to rename itself in either place.
pub async fn version_files(hashes: &[String]) -> Result<HashMap<String, Version>> {
    if hashes.is_empty() {
        return Ok(HashMap::new());
    }
    let body = serde_json::json!({ "hashes": hashes, "algorithm": "sha1" });
    download::post_json(&format!("{API}/version_files"), &body).await
}

/// Ask Modrinth which of these files have a newer build, keyed by the SHA-1
/// that was sent. Files it does not recognise are simply absent from the map,
/// which is how a hand-dropped jar stays out of the update list.
pub async fn updates(
    hashes: &[String],
    mc_version: &str,
) -> Result<HashMap<String, Version>> {
    if hashes.is_empty() {
        return Ok(HashMap::new());
    }
    let body = serde_json::json!({
        "hashes": hashes,
        "algorithm": "sha1",
        "loaders": ["fabric"],
        "game_versions": [mc_version],
    });
    download::post_json(&format!("{API}/version_files/update"), &body).await
}

/// Percent-encode a query string component. The whole alphabet we send is
/// ASCII, so this stays a few lines instead of a dependency.
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::urlencode;

    #[test]
    fn encodes_what_a_facet_string_contains() {
        assert_eq!(urlencode("sodium"), "sodium");
        assert_eq!(urlencode("just enough items"), "just+enough+items");
        assert_eq!(urlencode(r#"[["a:b"]]"#), "%5B%5B%22a%3Ab%22%5D%5D");
    }
}
