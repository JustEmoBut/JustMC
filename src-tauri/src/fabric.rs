//! Fabric loader metadata. Fabric publishes a ready-made version profile that
//! inherits from the vanilla version, so installing it is just fetching that
//! profile and merging it — no jar patching involved.

use crate::download;
use crate::error::{Error, Result};
use crate::mojang::VersionJson;
use serde::{Deserialize, Serialize};

const META: &str = "https://meta.fabricmc.net/v2/versions";

#[derive(Deserialize)]
struct LoaderEntry {
    loader: LoaderInfo,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct LoaderInfo {
    pub version: String,
    pub stable: bool,
}

/// Loader versions compatible with a given Minecraft version, newest first.
pub async fn loaders(mc_version: &str) -> Result<Vec<LoaderInfo>> {
    let entries: Vec<LoaderEntry> = match download::json(&format!("{META}/loader/{mc_version}")).await
    {
        Ok(entries) => entries,
        // Fabric answers 400 for a Minecraft version it has never supported.
        // That is an answer, not a failure: no loaders exist.
        Err(Error::Http(e)) if e.status() == Some(reqwest::StatusCode::BAD_REQUEST) => Vec::new(),
        Err(e) => return Err(e),
    };
    Ok(entries.into_iter().map(|e| e.loader).collect())
}

/// The newest stable loader, falling back to the newest of any kind.
pub async fn latest_loader(mc_version: &str) -> Result<String> {
    let list = loaders(mc_version).await?;
    list.iter()
        .find(|l| l.stable)
        .or_else(|| list.first())
        .map(|l| l.version.clone())
        .ok_or_else(|| Error::msg(format!("Fabric does not support Minecraft {mc_version}.")))
}

/// The loader's version profile, to be merged onto the vanilla version json.
pub async fn profile(mc_version: &str, loader_version: &str) -> Result<VersionJson> {
    download::json(&format!(
        "{META}/loader/{mc_version}/{loader_version}/profile/json"
    ))
    .await
}
