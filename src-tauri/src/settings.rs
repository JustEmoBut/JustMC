//! Launcher-wide preferences: `<root>/settings.json`.
//!
//! Everything else the launcher stores belongs to one instance or one account.
//! These are the few things that do not: what a *new* instance should start
//! with, and how the window behaves while a game runs.
//!
//! Missing or unreadable is not an error — a launcher that will not start
//! because a preferences file got truncated is worse than one that starts with
//! defaults. Unknown fields are ignored, and absent ones fall back, so a file
//! written by an older build still loads.

use crate::error::Result;
use crate::java;
use crate::paths;
use serde::{Deserialize, Serialize};

/// Minecraft rarely benefits above 8 GB and a large heap lengthens GC pauses,
/// so the automatic default never goes higher however much RAM is installed.
const MAX_DEFAULT_MB: u32 = 8192;
/// Below this the game does not reliably start at all.
const MIN_DEFAULT_MB: u32 = 2048;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Heap a new instance is created with.
    pub memory_mb: u32,
    /// Java a new instance is created with; empty means detect automatically.
    pub java_path: String,
    /// Extra JVM arguments a new instance is created with.
    pub jvm_args: String,
    /// Close the launcher window once a game is actually running, reopening it
    /// when the game exits. The name predates that: minimising freed nothing.
    pub minimise_on_play: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            memory_mb: default_memory_mb(),
            java_path: String::new(),
            jvm_args: String::new(),
            minimise_on_play: false,
        }
    }
}

/// Half the machine's RAM, bounded at both ends.
///
/// A flat 4 GB is wrong in both directions: on an 8 GB laptop it leaves the OS
/// nothing and `launch` refuses it outright, and on a 32 GB machine it is
/// needlessly timid. When the platform will not say, 4 GB is the safe guess.
fn default_memory_mb() -> u32 {
    match java::physical_memory_mb() {
        Some(total) => (total / 2).clamp(MIN_DEFAULT_MB, MAX_DEFAULT_MB),
        None => 4096,
    }
}

pub async fn load() -> Settings {
    match tokio::fs::read_to_string(paths::settings_file()).await {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub async fn save(settings: &Settings) -> Result<()> {
    tokio::fs::create_dir_all(paths::root()).await?;
    tokio::fs::write(paths::settings_file(), serde_json::to_vec_pretty(settings)?).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_file_keeps_the_defaults_for_what_it_omits() {
        let written: Settings = serde_json::from_str(r#"{"minimise_on_play": true}"#).unwrap();
        assert!(written.minimise_on_play);
        assert_eq!(written.memory_mb, Settings::default().memory_mb);
        assert!(written.java_path.is_empty());

        // A file from a newer build carries fields this one does not know.
        let newer: Settings = serde_json::from_str(r#"{"memory_mb": 6144, "future": 1}"#).unwrap();
        assert_eq!(newer.memory_mb, 6144);
    }

    #[test]
    fn the_automatic_default_stays_within_bounds() {
        let mb = default_memory_mb();
        assert!((MIN_DEFAULT_MB..=MAX_DEFAULT_MB).contains(&mb), "got {mb}");
    }
}
