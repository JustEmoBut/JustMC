use crate::error::{Error, Result};
use futures::{StreamExt, TryStreamExt};
use serde::Serialize;
use sha1::{Digest, Sha1};
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// At most this many downloads in flight. Mojang's CDN is happy with it and it
/// saturates a home connection without making progress reporting jumpy.
const PARALLEL: usize = 16;
/// How often the UI is told about progress. Fast enough to look live, slow
/// enough that a 5000-file install does not flood the webview with events.
const TICK: Duration = Duration::from_millis(200);

/// One file to fetch. `sha1` and `size` are optional because Fabric's Maven
/// entries carry neither.
#[derive(Debug, Clone)]
pub struct Job {
    pub url: String,
    pub path: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Serialize, Clone)]
pub struct Progress {
    pub stage: String,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    /// 0 when no job in this batch declared a size, so the UI can fall back to
    /// counting files instead of showing a bogus byte total.
    pub bytes_total: u64,
    pub bytes_per_sec: u64,
}

async fn sha1_of(path: &Path) -> Result<String> {
    let bytes = tokio::fs::read(path).await?;
    let mut hasher = Sha1::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

/// Whether the file is already on disk and looks right.
///
/// A size match is accepted as proof, without re-hashing. Every file is
/// verified against its SHA-1 at download time, so the only way a
/// right-sized wrong-content file exists is corruption after the fact.
/// Hashing instead would mean re-reading ~500 MB of assets before every
/// single launch.
///
/// ponytail: size-only revalidation; hash again here if silent disk
/// corruption ever turns out to be a real problem in practice.
async fn is_valid(job: &Job) -> bool {
    let Ok(meta) = tokio::fs::metadata(&job.path).await else {
        return false;
    };
    if let Some(size) = job.size {
        return meta.len() == size;
    }
    match &job.sha1 {
        Some(want) => matches!(sha1_of(&job.path).await, Ok(got) if got.eq_ignore_ascii_case(want)),
        // Nothing to check against: existence is all we have.
        None => true,
    }
}

/// Download one file, verifying its checksum, counting bytes as they arrive.
async fn fetch_one(client: &reqwest::Client, job: &Job, bytes_done: &AtomicU64) -> Result<()> {
    if let Some(parent) = job.path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut stream = client
        .get(&job.url)
        .send()
        .await?
        .error_for_status()?
        .bytes_stream();

    // Streamed rather than `.bytes()` so the speed readout updates during a
    // single large file — the client jar alone is 40 MB.
    let mut body = Vec::with_capacity(job.size.unwrap_or(0) as usize);
    let mut hasher = Sha1::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        body.extend_from_slice(&chunk);
        bytes_done.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }

    if let Some(want) = &job.sha1 {
        let got = hex::encode(hasher.finalize());
        if !got.eq_ignore_ascii_case(want) {
            return Err(Error::msg(format!(
                "checksum mismatch for {}: expected {want}, got {got}",
                job.url
            )));
        }
    }

    // Write to a temp file then rename, so an interrupted download never leaves
    // a truncated file that a later size check would happily accept. The suffix
    // is appended rather than replacing the extension: a JRE manifest contains
    // both `bin/java.exe` and `bin/java.dll`, which `with_extension` would
    // collapse onto the same `bin/java.part` and race.
    let mut tmp_name = job.path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".part");
    let tmp = job.path.with_file_name(tmp_name);
    tokio::fs::write(&tmp, &body).await?;
    tokio::fs::rename(&tmp, &job.path).await?;
    Ok(())
}

/// Download every job that is not already present, at most `PARALLEL` at once,
/// reporting progress and transfer rate to the UI as it goes.
pub async fn run(app: &AppHandle, stage: &str, jobs: Vec<Job>) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("JustLauncher/", env!("CARGO_PKG_VERSION")))
        .build()?;

    // Filter first so the totals describe the actual work, not the whole
    // version. On a reinstall this usually leaves the list empty.
    let pending: Vec<Job> = futures::stream::iter(jobs)
        .map(|job| async move { (is_valid(&job).await, job) })
        .buffer_unordered(PARALLEL * 4)
        .filter_map(|(valid, job)| async move { (!valid).then_some(job) })
        .collect()
        .await;

    let files_total = pending.len() as u64;
    let bytes_total: u64 = pending.iter().filter_map(|j| j.size).sum();
    if files_total == 0 {
        return Ok(());
    }

    let files_done = Arc::new(AtomicU64::new(0));
    let bytes_done = Arc::new(AtomicU64::new(0));

    let ticker = {
        let (app, stage) = (app.clone(), stage.to_string());
        let (files_done, bytes_done) = (files_done.clone(), bytes_done.clone());
        tokio::spawn(async move {
            let mut last_bytes = 0u64;
            let mut last_at = Instant::now();
            let mut smoothed = 0f64;
            loop {
                tokio::time::sleep(TICK).await;
                let bytes = bytes_done.load(Ordering::Relaxed);
                let elapsed = last_at.elapsed().as_secs_f64();
                if elapsed > 0.0 {
                    let instant = (bytes - last_bytes) as f64 / elapsed;
                    // Exponential smoothing: raw per-tick rates swing wildly as
                    // individual files start and finish, which makes the number
                    // unreadable.
                    smoothed = if smoothed == 0.0 {
                        instant
                    } else {
                        smoothed * 0.7 + instant * 0.3
                    };
                }
                last_bytes = bytes;
                last_at = Instant::now();

                let _ = app.emit(
                    "install-progress",
                    Progress {
                        stage: stage.clone(),
                        files_done: files_done.load(Ordering::Relaxed),
                        files_total,
                        bytes_done: bytes,
                        bytes_total,
                        bytes_per_sec: smoothed as u64,
                    },
                );
            }
        })
    };

    let result = futures::stream::iter(pending)
        .map(Ok::<_, Error>)
        .try_for_each_concurrent(PARALLEL, |job| {
            let (client, files_done, bytes_done) = (&client, &files_done, &bytes_done);
            async move {
                fetch_one(client, &job, bytes_done).await?;
                files_done.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
        })
        .await;

    ticker.abort();
    result?;

    // A final event so the bar always lands on 100% rather than wherever the
    // last tick happened to fall.
    let _ = app.emit(
        "install-progress",
        Progress {
            stage: stage.to_string(),
            files_done: files_total,
            files_total,
            bytes_done: bytes_done.load(Ordering::Relaxed),
            bytes_total,
            bytes_per_sec: 0,
        },
    );
    Ok(())
}

/// Fetch and deserialise JSON, with the launcher's user agent.
pub async fn json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("JustLauncher/", env!("CARGO_PKG_VERSION")))
        .build()?;
    Ok(client.get(url).send().await?.error_for_status()?.json().await?)
}

/// POST a JSON body and decode the JSON reply. Modrinth's bulk update check is
/// the only caller: it takes a list of hashes too long for a query string.
pub async fn post_json<T: serde::de::DeserializeOwned>(
    url: &str,
    body: &serde_json::Value,
) -> Result<T> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("JustLauncher/", env!("CARGO_PKG_VERSION")))
        .build()?;
    Ok(client.post(url).json(body).send().await?.error_for_status()?.json().await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch file with `len` bytes of content, cleaned up by the OS temp dir.
    fn temp_file(name: &str, len: usize) -> PathBuf {
        let path = std::env::temp_dir().join(format!("jl-test-{name}"));
        std::fs::write(&path, vec![b'x'; len]).unwrap();
        path
    }

    fn job(path: PathBuf, sha1: Option<&str>, size: Option<u64>) -> Job {
        Job {
            url: "http://example.invalid/x".into(),
            path,
            sha1: sha1.map(str::to_string),
            size,
        }
    }

    #[tokio::test]
    async fn missing_file_is_never_valid() {
        let path = std::env::temp_dir().join("jl-test-absent");
        let _ = std::fs::remove_file(&path);
        assert!(!is_valid(&job(path, None, Some(10))).await);
    }

    #[tokio::test]
    async fn declared_size_decides_without_hashing() {
        let path = temp_file("size", 10);
        // A deliberately wrong hash: a size match must short-circuit past it,
        // which is the whole point of not re-reading every asset each launch.
        assert!(is_valid(&job(path.clone(), Some("deadbeef"), Some(10))).await);
        assert!(!is_valid(&job(path, Some("deadbeef"), Some(11))).await);
    }

    #[tokio::test]
    async fn falls_back_to_hash_when_size_is_unknown() {
        let path = temp_file("hash", 3);
        // sha1("xxx")
        let digest = "b60d121b438a380c343d5ec3c2037564b82ffef3";
        assert!(is_valid(&job(path.clone(), Some(digest), None)).await);
        assert!(!is_valid(&job(path, Some("deadbeef"), None)).await);
    }

    #[tokio::test]
    async fn existence_alone_suffices_when_nothing_is_declared() {
        let path = temp_file("bare", 5);
        assert!(is_valid(&job(path, None, None)).await);
    }
}
