//! An instance is a directory with a config file and a `.minecraft` game dir.
//! Nothing is indexed or cached in a database — the folder is the source of
//! truth, so a user can copy, delete or back one up with the file manager.

use crate::error::{Error, Result};
use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CONFIG_NAME: &str = "instance.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    #[default]
    Vanilla,
    Fabric,
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
    /// Unix seconds of the last launch; 0 if never played.
    #[serde(default)]
    pub last_played: u64,
    /// Whether the install step has completed at least once.
    #[serde(default)]
    pub installed: bool,
}

fn default_memory() -> u32 {
    4096
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

    /// The version id this instance actually launches — for Fabric that is the
    /// loader profile, not the plain Minecraft version.
    pub fn version_id(&self) -> String {
        match self.loader {
            Loader::Vanilla => self.mc_version.clone(),
            Loader::Fabric => format!("fabric-loader-{}-{}", self.loader_version, self.mc_version),
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

pub async fn create(name: &str, mc_version: &str, loader: Loader) -> Result<Instance> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::msg("Instance name cannot be empty."));
    }
    let instance = Instance {
        id: unique_id(name),
        name: name.to_string(),
        mc_version: mc_version.to_string(),
        loader,
        loader_version: String::new(),
        memory_mb: default_memory(),
        java_path: String::new(),
        jvm_args: String::new(),
        last_played: 0,
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
mod tests {
    use super::*;

    #[test]
    fn slugify_strips_unsafe_characters() {
        assert_eq!(slugify("My Pack 1.20"), "my-pack-1-20");
        assert_eq!(slugify("../etc/passwd"), "etc-passwd");
        assert_eq!(slugify("***"), "instance");
    }

    #[test]
    fn version_id_reflects_loader() {
        let mut i = Instance {
            id: "x".into(),
            name: "x".into(),
            mc_version: "1.21".into(),
            loader: Loader::Vanilla,
            loader_version: String::new(),
            memory_mb: 4096,
            java_path: String::new(),
            jvm_args: String::new(),
            last_played: 0,
            installed: false,
        };
        assert_eq!(i.version_id(), "1.21");
        i.loader = Loader::Fabric;
        i.loader_version = "0.16.0".into();
        assert_eq!(i.version_id(), "fabric-loader-0.16.0-1.21");
    }

    #[tokio::test]
    async fn delete_rejects_traversal_and_non_instances() {
        assert!(delete("../..").await.is_err());
        assert!(delete("").await.is_err());
        assert!(delete("definitely-not-here").await.is_err());
    }
}
