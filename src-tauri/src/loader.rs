//! Mod loader metadata, for Fabric and Quilt.
//!
//! Both publish a ready-made version profile that inherits from the vanilla
//! version, so installing either is just fetching that profile and merging it —
//! no jar patching involved. That is why they share this module and why Forge
//! does not: Forge patches the client jar.
//!
//! The two APIs are the same shape but not the same in three details, each of
//! which is a real bug if assumed away:
//!
//! - **"no loaders" is a different status.** Fabric answers 400 for a Minecraft
//!   version it never supported, Quilt answers 404. Both are answers, not
//!   failures.
//! - **Quilt publishes no `stable` flag, and means it.** Fabric marks the one
//!   build it recommends and that is what a new instance gets. Quilt recommends
//!   nothing and ships betas as its normal channel — its newest build for
//!   1.21.1 was `0.30.1-beta.1` while the newest plain release was four minor
//!   versions behind — so a Quilt instance takes the newest, and nothing is
//!   labelled recommended in the picker.
//! - **Quilt's list is unordered.** Fabric returns newest first; Quilt returned
//!   `0.20.0-beta.9, 0.20.0-beta.7, 0.20.0-beta.8, …, 0.24.0` when this was
//!   written, so taking the first entry would pin an old beta. Both lists are
//!   sorted here.

use crate::download;
use crate::error::{Error, Result};
use crate::instance::Loader;
use crate::mojang::VersionJson;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct LoaderEntry {
    loader: LoaderInfo,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct LoaderInfo {
    pub version: String,
    /// The one build Fabric recommends. Absent from Quilt's metadata, and left
    /// false rather than guessed: every other build being flagged "not
    /// recommended" would be a claim Quilt never made.
    #[serde(default)]
    pub stable: bool,
}

/// Where a loader's metadata lives, and what it answers for a version it does
/// not support.
fn meta(loader: Loader) -> (&'static str, reqwest::StatusCode) {
    match loader {
        Loader::Quilt => (
            "https://meta.quiltmc.org/v3/versions",
            reqwest::StatusCode::NOT_FOUND,
        ),
        // Vanilla never reaches here; treat it as Fabric rather than panicking.
        _ => (
            "https://meta.fabricmc.net/v2/versions",
            reqwest::StatusCode::BAD_REQUEST,
        ),
    }
}

/// Sort key: the numeric components, so `0.24.0` outranks `0.20.0`, with a
/// plain release ahead of any pre-release of the same numbers.
fn version_key(version: &str) -> (Vec<u64>, bool) {
    let numbers = version
        .split(['.', '-', '+'])
        .map_while(|part| part.parse::<u64>().ok())
        .collect();
    (numbers, !version.contains('-'))
}

/// Loader versions compatible with a given Minecraft version, newest first.
/// Every loader's build list, whichever module knows how to get it. One entry
/// point so the UI does not have to know which loaders ship an installer.
pub async fn builds(loader: Loader, mc_version: &str) -> Result<Vec<LoaderInfo>> {
    match loader {
        Loader::Vanilla => Ok(Vec::new()),
        l if l.is_forge() => crate::forge::versions(l, mc_version).await,
        l => loaders(l, mc_version).await,
    }
}

pub async fn loaders(loader: Loader, mc_version: &str) -> Result<Vec<LoaderInfo>> {
    let (meta, absent) = meta(loader);
    let entries: Vec<LoaderEntry> =
        match download::json(&format!("{meta}/loader/{mc_version}")).await {
            Ok(entries) => entries,
            // The loader has never supported this Minecraft version. That is an
            // answer -- no loaders exist -- so the UI can say so before an
            // instance is created.
            Err(Error::Http(e)) if e.status() == Some(absent) => Vec::new(),
            Err(e) => return Err(e),
        };

    let mut list: Vec<LoaderInfo> = entries.into_iter().map(|e| e.loader).collect();
    list.sort_by(|a, b| version_key(&b.version).cmp(&version_key(&a.version)));
    Ok(list)
}

/// The build a new instance gets: the one Fabric recommends, or -- for Quilt,
/// which recommends none -- simply the newest.
pub async fn latest_loader(loader: Loader, mc_version: &str) -> Result<String> {
    if loader.is_forge() {
        return crate::forge::latest(loader, mc_version).await;
    }
    let list = loaders(loader, mc_version).await?;
    list.iter()
        .find(|l| l.stable)
        .or_else(|| list.first())
        .map(|l| l.version.clone())
        .ok_or_else(|| {
            Error::msg(format!("{} does not support Minecraft {mc_version}.", loader.label()))
        })
}

/// The loader's version profile, to be merged onto the vanilla version json.
pub async fn profile(loader: Loader, mc_version: &str, loader_version: &str) -> Result<VersionJson> {
    let (meta, _) = meta(loader);
    download::json(&format!("{meta}/loader/{mc_version}/{loader_version}/profile/json")).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_loader_knows_where_its_metadata_is() {
        assert!(meta(Loader::Fabric).0.contains("fabricmc"));
        assert!(meta(Loader::Quilt).0.contains("quiltmc"));
        // The status that means "never supported" differs between them.
        assert_eq!(meta(Loader::Fabric).1, reqwest::StatusCode::BAD_REQUEST);
        assert_eq!(meta(Loader::Quilt).1, reqwest::StatusCode::NOT_FOUND);
    }

    #[test]
    fn newest_wins_however_the_server_ordered_the_list() {
        // The order Quilt actually returned for 1.21.1.
        let mut list = ["0.20.0-beta.9", "0.20.0-beta.7", "0.24.0", "0.20.2-beta.1"];
        list.sort_by(|a, b| version_key(b).cmp(&version_key(a)));
        assert_eq!(list[0], "0.24.0");
        // A release outranks a pre-release of the same numbers.
        assert!(version_key("0.20.0") > version_key("0.20.0-beta.9"));
    }
}
