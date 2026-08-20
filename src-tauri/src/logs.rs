//! Reading logs and crash reports back after the fact.
//!
//! Three directories hold them and the user does not care which: our own
//! `<instance>/logs/latest.log`, the game's rotated `.minecraft/logs`, and
//! `.minecraft/crash-reports`. `Source` is the only thing that differs, and it
//! is what turns a file name back into a directory without the frontend ever
//! sending a path.
//!
//! Like the instance list and the mods folder, the directory is the database:
//! nothing is indexed or cached.

use crate::error::{Error, Result};
use crate::instance;
use crate::mods::checked_name;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Tail kept when opening a file. The live view caps at the same number, for
/// the same reason: a long session writes tens of thousands of lines and
/// rendering all of them is what makes launcher log views crawl.
const MAX_LINES: usize = 2000;

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// What the launcher captured from the process, truncated per launch.
    Launcher,
    /// The game's own logs, where anything but `latest.log` is gzipped.
    Game,
    /// `crash-reports/`, plain text, written when the game dies.
    Crash,
}

#[derive(Serialize)]
pub struct LogFile {
    pub file: String,
    pub source: Source,
    pub size: u64,
    /// Unix seconds, so the list can be sorted and dated in the UI.
    pub modified: u64,
}

async fn dir(id: &str, source: Source) -> Result<PathBuf> {
    let instance = instance::get(id).await?;
    Ok(match source {
        Source::Launcher => instance.dir().join("logs"),
        Source::Game => instance.game_dir().join("logs"),
        Source::Crash => instance.game_dir().join("crash-reports"),
    })
}

fn is_log(file: &str, source: Source) -> bool {
    match source {
        Source::Crash => file.ends_with(".txt"),
        _ => file.ends_with(".log") || file.ends_with(".log.gz"),
    }
}

/// Every log and crash report of an instance, newest first.
pub async fn list(id: &str) -> Result<Vec<LogFile>> {
    let mut out = Vec::new();
    for source in [Source::Launcher, Source::Game, Source::Crash] {
        let Ok(entries) = std::fs::read_dir(dir(id, source).await?) else {
            continue; // a folder that does not exist yet is not an error
        };
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            if !is_log(&file, source) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            out.push(LogFile { file, source, size: meta.len(), modified });
        }
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(out)
}

/// The tail of one file, as lines. `.gz` is decompressed on the way.
pub async fn read(id: &str, source: Source, file: &str) -> Result<Vec<String>> {
    let path = dir(id, source).await?.join(checked_name(file)?);
    if !is_log(file, source) {
        return Err(Error::msg("Not a log file."));
    }
    tail(file, &tokio::fs::read(&path).await?)
}

/// The last `MAX_LINES` lines of a log's bytes, gunzipping a rotated one. A
/// log is whatever the game wrote, so invalid UTF-8 is shown, not an error.
fn tail(file: &str, bytes: &[u8]) -> Result<Vec<String>> {
    let text = if file.ends_with(".gz") {
        use std::io::Read;
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes).read_to_end(&mut out)?;
        String::from_utf8_lossy(&out).into_owned()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    let lines: Vec<&str> = text.lines().collect();
    Ok(lines[lines.len().saturating_sub(MAX_LINES)..].iter().map(|s| s.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_logs_are_listed() {
        assert!(is_log("latest.log", Source::Launcher));
        assert!(is_log("2026-08-19-1.log.gz", Source::Game));
        assert!(!is_log("options.txt", Source::Game));
        assert!(is_log("crash-2026-08-19_12.00.00-client.txt", Source::Crash));
        assert!(!is_log("latest.log", Source::Crash));
    }

    #[test]
    fn gzipped_logs_are_decompressed_and_tailed() {
        use std::io::Write;
        let text: String = (0..MAX_LINES + 10).map(|i| format!("line {i}
")).collect();
        let mut gz =
            flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(text.as_bytes()).unwrap();
        let bytes = gz.finish().unwrap();

        let lines = tail("2026-08-19-1.log.gz", &bytes).unwrap();
        assert_eq!(lines.len(), MAX_LINES);
        assert_eq!(lines.last().unwrap(), &format!("line {}", MAX_LINES + 9));
        assert_eq!(tail("latest.log", b"a
b
").unwrap(), vec!["a", "b"]);
    }

    #[test]
    fn names_with_separators_are_refused() {
        assert!(checked_name("../../accounts.json").is_err());
    }
}
