//! An instance is a directory with a config file and a `.minecraft` game dir.
//! Nothing is indexed or cached in a database — the folder is the source of
//! truth, so a user can copy, delete or back one up with the file manager.

use crate::error::{Error, Result};
use crate::paths;
use crate::settings;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CONFIG_NAME: &str = "instance.json";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    #[default]
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl Loader {
    /// The prefix Fabric and Quilt use for the profile they publish, which is
    /// also the version id an instance launches. Forge names its own profiles
    /// differently, so `version_id` asks `forge::profile_id` instead.
    pub(crate) fn profile_prefix(self) -> &'static str {
        match self {
            Loader::Fabric => "fabric-loader",
            Loader::Quilt => "quilt-loader",
            _ => "",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Loader::Vanilla => "Vanilla",
            Loader::Fabric => "Fabric",
            Loader::Quilt => "Quilt",
            Loader::Forge => "Forge",
            Loader::NeoForge => "NeoForge",
        }
    }

    /// Whether this loader is installed by running an installer rather than by
    /// fetching a published profile -- the line between `loader.rs` and
    /// `forge.rs`.
    pub fn is_forge(self) -> bool {
        matches!(self, Loader::Forge | Loader::NeoForge)
    }

    /// Modrinth's loader tags for mods this instance can run. Quilt loads
    /// Fabric mods, so both are asked for -- Quilt alone returns a quarter of
    /// the catalogue. NeoForge is asked for on its own: it forked at 1.20.2 and
    /// a Forge jar of that era does not load in it.
    pub fn mod_loaders(self) -> &'static [&'static str] {
        match self {
            Loader::Quilt => &["quilt", "fabric"],
            Loader::Forge => &["forge"],
            Loader::NeoForge => &["neoforge"],
            _ => &["fabric"],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Instance {
    /// Folder name under `instances/`; also the stable identity.
    pub id: String,
    pub name: String,
    pub mc_version: String,
    #[serde(default)]
    pub loader: Loader,
    /// Resolved Fabric loader version, set at install time.
    #[serde(default)]
    pub loader_version: String,
    #[serde(default = "default_memory")]
    pub memory_mb: u32,
    /// Empty means "detect a suitable JVM automatically".
    #[serde(default)]
    pub java_path: String,
    #[serde(default)]
    pub jvm_args: String,
    /// Window size the game opens at. 0 means "leave it to the game", which is
    /// what options.txt already remembers.
    #[serde(default)]
    pub window_width: u32,
    #[serde(default)]
    pub window_height: u32,
    /// Shell command run before the game starts; a non-zero exit cancels the
    /// launch. Empty means none.
    #[serde(default)]
    pub pre_launch: String,
    /// Shell command run after the game exits. Its failure is logged, not
    /// raised: the session is already over.
    #[serde(default)]
    pub post_exit: String,
    /// Unix seconds of the last launch; 0 if never played.
    #[serde(default)]
    pub last_played: u64,
    /// Seconds the game has run in this instance, across every launch.
    #[serde(default)]
    pub play_time: u64,
    /// Whether a session here adds to the counters. Off for an instance kept
    /// for testing, whose minutes are not time the user spent playing.
    #[serde(default = "yes")]
    pub count_play_time: bool,
    /// Whether the install step has completed at least once.
    #[serde(default)]
    pub installed: bool,
}

/// Only for an instance.json that predates the field; a new instance takes
/// its heap from the launcher settings instead.
fn default_memory() -> u32 {
    4096
}

/// `#[serde(default)]` on a bool is `false`; an instance.json that predates the
/// field was counting, so it has to keep counting.
fn yes() -> bool {
    true
}

/// Unix seconds now. The clock can disagree with itself across a launch, so
/// every caller has to cope with 0 rather than trusting the difference.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Instance {
    pub fn dir(&self) -> PathBuf {
        paths::instances().join(&self.id)
    }

    /// The game's working directory, kept in a subfolder so instance metadata
    /// never mixes with saves, mods and configs.
    pub fn game_dir(&self) -> PathBuf {
        self.dir().join(".minecraft")
    }

    pub fn natives_dir(&self) -> PathBuf {
        self.dir().join("natives")
    }

    /// The version id this instance actually launches — for a modded instance
    /// that is the loader profile, not the plain Minecraft version.
    pub fn version_id(&self) -> String {
        match self.loader {
            Loader::Vanilla => self.mc_version.clone(),
            loader if loader.is_forge() => {
                crate::forge::profile_id(loader, &self.mc_version, &self.loader_version)
            }
            loader => format!(
                "{}-{}-{}",
                loader.profile_prefix(),
                self.loader_version,
                self.mc_version
            ),
        }
    }

    pub async fn save(&self) -> Result<()> {
        tokio::fs::create_dir_all(self.dir()).await?;
        tokio::fs::write(
            self.dir().join(CONFIG_NAME),
            serde_json::to_vec_pretty(self)?,
        )
        .await?;
        Ok(())
    }
}

/// Folder names come from user-supplied instance names, so strip anything that
/// is not safe in a path on any platform.
fn slugify(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_lowercase();
    if slug.is_empty() {
        "instance".to_string()
    } else {
        slug
    }
}

/// A free folder name derived from `name`, with a numeric suffix on collision.
pub fn unique_id(name: &str) -> String {
    let base = slugify(name);
    let mut id = base.clone();
    let mut n = 2;
    while paths::instances().join(&id).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

/// `loader_version` empty means "resolve it at install time", which is what
/// the picker's default and every vanilla instance send.
pub async fn create(
    name: &str,
    mc_version: &str,
    loader: Loader,
    loader_version: &str,
) -> Result<Instance> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::msg("Instance name cannot be empty."));
    }
    // What the user set as the default for new instances, not a constant: a
    // flat 4 GB is too much on a small machine and too little on a large one.
    let defaults = settings::load().await;
    let instance = Instance {
        id: unique_id(name),
        name: name.to_string(),
        mc_version: mc_version.to_string(),
        loader,
        // A pinned build is only meaningful with a loader to pin it to.
        loader_version: if loader == Loader::Vanilla {
            String::new()
        } else {
            loader_version.to_string()
        },
        memory_mb: defaults.memory_mb,
        java_path: defaults.java_path,
        jvm_args: defaults.jvm_args,
        window_width: 0,
        window_height: 0,
        pre_launch: String::new(),
        post_exit: String::new(),
        last_played: 0,
        play_time: 0,
        count_play_time: true,
        installed: false,
    };
    tokio::fs::create_dir_all(instance.game_dir()).await?;
    instance.save().await?;
    Ok(instance)
}

pub async fn list() -> Vec<Instance> {
    let Ok(mut entries) = tokio::fs::read_dir(paths::instances()).await else {
        return Vec::new();
    };
    let mut out = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let config = entry.path().join(CONFIG_NAME);
        if let Ok(text) = tokio::fs::read_to_string(&config).await {
            if let Ok(instance) = serde_json::from_str::<Instance>(&text) {
                out.push(instance);
            }
        }
    }
    // Most recently played first; never-played instances fall to the end.
    out.sort_by(|a, b| b.last_played.cmp(&a.last_played).then(a.name.cmp(&b.name)));
    out
}

pub async fn get(id: &str) -> Result<Instance> {
    let text = tokio::fs::read_to_string(paths::instances().join(id).join(CONFIG_NAME))
        .await
        .map_err(|_| Error::msg(format!("No such instance: {id}")))?;
    Ok(serde_json::from_str(&text)?)
}

pub async fn delete(id: &str) -> Result<()> {
    // Reject anything that could escape the instances directory before removing
    // a tree recursively — this is the one call here that destroys user data.
    if id.is_empty() || id.contains(['/', '\\']) || id.contains("..") {
        return Err(Error::msg("Invalid instance id."));
    }
    let dir = paths::instances().join(id);
    if !dir.join(CONFIG_NAME).exists() {
        return Err(Error::msg("Not an instance directory; refusing to delete."));
    }
    tokio::fs::remove_dir_all(dir).await?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A plain instance for a test to bend into shape. Shared so a new field
    /// does not have to be added to a literal in every test module.
    pub(crate) fn sample() -> Instance {
        Instance {
            id: "x".into(),
            name: "x".into(),
            mc_version: "1.21".into(),
            loader: Loader::Vanilla,
            loader_version: String::new(),
            memory_mb: 4096,
            java_path: String::new(),
            jvm_args: String::new(),
            window_width: 0,
            window_height: 0,
            pre_launch: String::new(),
            post_exit: String::new(),
            last_played: 0,
            play_time: 0,
            count_play_time: true,
            installed: false,
        }
    }

    #[test]
    fn slugify_strips_unsafe_characters() {
        assert_eq!(slugify("My Pack 1.20"), "my-pack-1-20");
        assert_eq!(slugify("../etc/passwd"), "etc-passwd");
        assert_eq!(slugify("***"), "instance");
    }

    #[test]
    fn version_id_reflects_loader() {
        let mut i = sample();
        assert_eq!(i.version_id(), "1.21");
        i.loader = Loader::Quilt;
        i.loader_version = "0.24.0".into();
        assert_eq!(i.version_id(), "quilt-loader-0.24.0-1.21");
        i.loader = Loader::Fabric;
        i.loader_version = "0.16.0".into();
        assert_eq!(i.version_id(), "fabric-loader-0.16.0-1.21");
    }

    #[test]
    fn an_instance_from_before_the_field_keeps_counting() {
        let old: Instance = serde_json::from_str(
            r#"{"id":"a","name":"a","mc_version":"1.21","play_time":60}"#,
        )
        .unwrap();
        assert!(old.count_play_time);
    }

    #[tokio::test]
    async fn delete_rejects_traversal_and_non_instances() {
        assert!(delete("../..").await.is_err());
        assert!(delete("").await.is_err());
        assert!(delete("definitely-not-here").await.is_err());
    }
}
