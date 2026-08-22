//! Data model for Mojang version metadata, plus the rule evaluation that
//! decides which libraries and arguments apply to the current machine.

use crate::download::{self, Job};
use crate::error::{Error, Result};
use crate::paths;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const ASSET_BASE: &str = "https://resources.download.minecraft.net";

#[derive(Deserialize)]
pub struct Manifest {
    pub latest: Latest,
    pub versions: Vec<ManifestVersion>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ManifestVersion {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
}

// ---------------------------------------------------------------- version json

#[derive(Deserialize, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    #[serde(default)]
    pub inherits_from: Option<String>,
    #[serde(default)]
    pub main_class: String,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub downloads: HashMap<String, Artifact>,
    #[serde(default)]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    /// Pre-1.13 versions ship one flat argument string instead of `arguments`.
    #[serde(default)]
    pub minecraft_arguments: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: String,
}

#[derive(Deserialize, Serialize, Clone, Default)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Arg>,
    #[serde(default)]
    pub jvm: Vec<Arg>,
}

/// An argument is either a bare string or a rule-guarded one-or-many strings.
#[derive(Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum Arg {
    Plain(String),
    Conditional { rules: Vec<Rule>, value: StringOrList },
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum StringOrList {
    One(String),
    Many(Vec<String>),
}

impl StringOrList {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            StringOrList::One(s) => vec![s],
            StringOrList::Many(v) => v,
        }
    }
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    #[serde(default)]
    pub component: String,
    pub major_version: u32,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub url: String,
}

#[derive(Deserialize, Serialize, Clone, Default)]
pub struct Artifact {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    pub url: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// Legacy natives: maps an OS to a key in `downloads.classifiers`.
    #[serde(default)]
    pub natives: HashMap<String, String>,
    #[serde(default)]
    pub extract: Option<Extract>,
    /// Fabric libraries give a bare Maven repo URL instead of `downloads`.
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: HashMap<String, Artifact>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: HashMap<String, bool>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

// ------------------------------------------------------------- rule evaluation

pub fn os_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "osx",
        _ => "linux",
    }
}

fn os_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86" => "x86",
        "aarch64" => "arm64",
        _ => "x64",
    }
}

/// Rules are evaluated in order and the last match wins; no rules at all means
/// allowed. We declare no optional features (demo mode, custom resolution), so
/// any feature-gated rule simply does not match.
pub fn rules_allow(rules: &[Rule]) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        let matches = rule.features.is_empty()
            && match &rule.os {
                None => true,
                Some(os) => {
                    os.name.as_deref().map_or(true, |n| n == os_name())
                        && os.arch.as_deref().map_or(true, |a| a == os_arch())
                }
            };
        if matches {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

// ------------------------------------------------------------ library handling

/// Turn `group:artifact:version[:classifier][@extension]` into a Maven
/// relative path.
///
/// The `@extension` suffix comes from Forge's install profiles, which name
/// `neoform:1.21.1-...@zip` and `...:mappings@txt` — Mojang's own metadata
/// never uses it, and reading it as part of the version instead produces a
/// directory named `1.21.1-...@zip` that nothing will ever create.
pub fn maven_path(name: &str) -> Result<String> {
    let (name, extension) = match name.split_once('@') {
        Some((rest, ext)) => (rest, ext),
        None => (name, "jar"),
    };
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 || parts.iter().take(3).any(|p| p.is_empty()) {
        return Err(Error::msg(format!("malformed library name: {name}")));
    }
    let (group, artifact, version) = (parts[0], parts[1], parts[2]);
    let classifier = parts.get(3).map(|c| format!("-{c}")).unwrap_or_default();
    Ok(format!(
        "{}/{artifact}/{version}/{artifact}-{version}{classifier}.{extension}",
        group.replace('.', "/")
    ))
}

/// A library resolved to a concrete file, plus whether it is a natives archive
/// that must be extracted rather than put on the classpath.
pub struct ResolvedLib {
    pub path: PathBuf,
    pub job: Option<Job>,
    pub natives: Option<Extract>,
}

impl Library {
    /// `None` when this library does not apply to the current OS.
    pub fn resolve(&self) -> Result<Option<ResolvedLib>> {
        if !rules_allow(&self.rules) {
            return Ok(None);
        }
        let downloads = self.downloads.clone().unwrap_or_default();

        // Legacy natives jar: pick the classifier for this OS.
        if let Some(key) = self.natives.get(os_name()) {
            let key = key.replace("${arch}", if os_arch() == "x86" { "32" } else { "64" });
            let Some(art) = downloads.classifiers.get(&key) else {
                return Ok(None);
            };
            let rel = match &art.path {
                Some(p) => p.clone(),
                None => maven_path(&format!("{}:{key}", self.name))?,
            };
            let path = paths::libraries().join(&rel);
            return Ok(Some(ResolvedLib {
                job: Some(Job {
                    url: art.url.clone(),
                    path: path.clone(),
                    sha1: art.sha1.clone(),
                    size: art.size,
                }),
                path,
                natives: Some(self.extract.clone().unwrap_or_default()),
            }));
        }

        let rel = match &downloads.artifact {
            Some(a) if a.path.is_some() => a.path.clone().unwrap(),
            _ => maven_path(&self.name)?,
        };
        let path = paths::libraries().join(&rel);

        let job = match &downloads.artifact {
            // An empty URL means the file is produced on this machine, not
            // fetched: Forge lists its patched client jar that way, with a
            // hash and a size but nowhere to get it from.
            Some(a) if a.url.is_empty() => None,
            Some(a) => Some(Job {
                url: a.url.clone(),
                path: path.clone(),
                sha1: a.sha1.clone(),
                size: a.size,
            }),
            // Fabric-style entry: build the URL from the repo base + Maven path.
            None => self.url.as_ref().map(|base| Job {
                url: format!("{}{rel}", if base.ends_with('/') { base.clone() } else { format!("{base}/") }),
                path: path.clone(),
                sha1: None,
                size: None,
            }),
        };

        // Modern versions name native libraries `...:natives-windows` with no
        // `natives` block; those belong on the classpath, not the extract dir.
        Ok(Some(ResolvedLib { path, job, natives: None }))
    }
}

// -------------------------------------------------------------------- fetching

pub async fn manifest() -> Result<Manifest> {
    download::json(MANIFEST_URL).await
}

/// Fetch a version JSON, caching it under the shared version store.
pub async fn version_json(id: &str, url: &str) -> Result<VersionJson> {
    let path = paths::versions().join(id).join(format!("{id}.json"));
    if let Ok(text) = tokio::fs::read_to_string(&path).await {
        if let Ok(v) = serde_json::from_str(&text) {
            return Ok(v);
        }
    }
    let text = reqwest::get(url).await?.error_for_status()?.text().await?;
    let parsed: VersionJson = serde_json::from_str(&text)?;
    tokio::fs::create_dir_all(path.parent().unwrap()).await?;
    tokio::fs::write(&path, &text).await?;
    Ok(parsed)
}

/// Merge a child profile (Fabric) onto the vanilla version it inherits from.
pub fn merge(parent: VersionJson, child: VersionJson) -> VersionJson {
    let mut out = parent;
    out.id = child.id;
    if !child.main_class.is_empty() {
        out.main_class = child.main_class;
    }
    // Child libraries go first: the classpath is scanned in order, so a loader's
    // patched copy of a library must shadow the vanilla one.
    let mut libs = child.libraries;
    libs.extend(out.libraries);
    out.libraries = libs;

    if let Some(child_args) = child.arguments {
        let mut args = out.arguments.unwrap_or_default();
        args.game.extend(child_args.game);
        args.jvm.extend(child_args.jvm);
        out.arguments = Some(args);
    }
    if child.java_version.is_some() {
        out.java_version = child.java_version;
    }
    out.inherits_from = None;
    out
}

// ---------------------------------------------------------------------- assets

#[derive(Deserialize)]
pub struct AssetIndex {
    pub objects: HashMap<String, AssetObject>,
    /// 1.6 to 1.7.2: the game reads assets by their real names out of
    /// `assets/virtual/<index id>` instead of by hash, so the hashed store has
    /// to be materialised into a named tree after downloading.
    #[serde(default, rename = "virtual")]
    pub is_virtual: bool,
    /// 1.5.2 and older: same idea, but the named tree belongs inside the
    /// instance itself, as `.minecraft/resources`.
    #[serde(default)]
    pub map_to_resources: bool,
}

#[derive(Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

impl AssetObject {
    /// Where the object lives in the shared, hash-addressed store.
    pub fn path(&self) -> PathBuf {
        paths::assets()
            .join("objects")
            .join(&self.hash[..2])
            .join(&self.hash)
    }
}

impl AssetIndex {
    pub fn jobs(&self) -> Vec<Job> {
        self.objects
            .values()
            .map(|o| Job {
                url: format!("{ASSET_BASE}/{}/{}", &o.hash[..2], o.hash),
                path: o.path(),
                sha1: Some(o.hash.clone()),
                size: Some(o.size),
            })
            .collect()
    }

    /// True for versions that cannot read the hashed store directly.
    pub fn needs_named_copy(&self) -> bool {
        self.is_virtual || self.map_to_resources
    }
}

/// Fetch an asset index, caching it under the shared asset store.
pub async fn asset_index(index: &AssetIndexRef) -> Result<AssetIndex> {
    let index_path = paths::assets()
        .join("indexes")
        .join(format!("{}.json", index.id));
    let text = match tokio::fs::read_to_string(&index_path).await {
        Ok(t) => t,
        Err(_) => {
            let t = reqwest::get(&index.url).await?.error_for_status()?.text().await?;
            tokio::fs::create_dir_all(index_path.parent().unwrap()).await?;
            tokio::fs::write(&index_path, &t).await?;
            t
        }
    };
    Ok(serde_json::from_str(&text)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os_rule(name: &str, action: &str) -> Rule {
        Rule {
            action: action.into(),
            os: Some(OsRule { name: Some(name.into()), arch: None, version: None }),
            features: HashMap::new(),
        }
    }

    #[test]
    fn no_rules_means_allowed() {
        assert!(rules_allow(&[]));
    }

    #[test]
    fn allow_then_disallow_this_os_denies() {
        let rules = vec![
            Rule { action: "allow".into(), os: None, features: HashMap::new() },
            os_rule(os_name(), "disallow"),
        ];
        assert!(!rules_allow(&rules));
    }

    #[test]
    fn other_os_rule_does_not_apply() {
        let other = if os_name() == "windows" { "linux" } else { "windows" };
        assert!(!rules_allow(&[os_rule(other, "allow")]));
        assert!(rules_allow(&[os_rule(os_name(), "allow")]));
    }

    #[test]
    fn feature_gated_rules_never_match() {
        let mut features = HashMap::new();
        features.insert("is_demo_user".to_string(), true);
        let rules = vec![Rule { action: "allow".into(), os: None, features }];
        assert!(!rules_allow(&rules));
    }

    #[test]
    fn maven_paths() {
        assert_eq!(
            maven_path("com.mojang:logging:1.1.1").unwrap(),
            "com/mojang/logging/1.1.1/logging-1.1.1.jar"
        );
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.3.3:natives-windows").unwrap(),
            "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-windows.jar"
        );
        // Forge's own coordinates: an explicit extension, with and without a
        // classifier. Both taken from a real install profile.
        assert_eq!(
            maven_path("net.neoforged:neoform:1.21.1-20240808.144430@zip").unwrap(),
            "net/neoforged/neoform/1.21.1-20240808.144430/neoform-1.21.1-20240808.144430.zip"
        );
        assert_eq!(
            maven_path("net.neoforged:neoform:1.21.1-20240808.144430:mappings@txt").unwrap(),
            "net/neoforged/neoform/1.21.1-20240808.144430/neoform-1.21.1-20240808.144430-mappings.txt"
        );
        assert!(maven_path("nope").is_err());
        assert!(maven_path("a::1").is_err(), "an empty part is not a coordinate");
    }
}
