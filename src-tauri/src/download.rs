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
/// How many times one request is tried before its error is given up on. The
/// first try plus two retries survives a blip and one rate-limit round trip
/// without turning a dead URL into a minute of waiting.
const ATTEMPTS: u32 = 3;
/// The first wait between attempts; each further one is four times longer, so
/// the whole ladder is 2 s then 8 s. Long enough for a rate limit to pass,
/// short enough that a retry does not feel like a hang.
const BACKOFF: Duration = Duration::from_secs(2);
/// The longest a server's `Retry-After` is obeyed for. A server that asks for
/// an hour is not going to be ready in this batch either way.
const MAX_SERVER_WAIT: Duration = Duration::from_secs(60);

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
/// Kept deliberately, not pending: hashing here would trade a real cost every
/// launch for a failure mode — silent disk corruption of an already-verified
/// file — that the game itself would report as a crash on next read.
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

/// Whether an HTTP status is worth another attempt. Anything else — 404, 403,
/// a redirect that landed on an error — is a statement about the URL, not
/// about the connection, and waiting will not change the answer.
fn retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

/// A network error is worth another attempt when the request never got a
/// complete answer: the connection failed, timed out or was cut mid-request.
/// A malformed URL or a TLS failure is just as permanent the second time.
fn retryable_error(error: &reqwest::Error) -> bool {
    error.is_connect() || error.is_timeout() || error.is_request()
}

/// Parse `Retry-After` in its delay-seconds form, capped. The HTTP-date form
/// exists in the spec but file CDNs send seconds; a date falls back to the
/// ordinary backoff ladder, which is close enough.
fn retry_after(header: Option<&reqwest::header::HeaderValue>) -> Option<Duration> {
    let seconds: u64 = header?.to_str().ok()?.trim().parse().ok()?;
    Some(Duration::from_secs(seconds).min(MAX_SERVER_WAIT))
}

/// The wait before attempt `n` (counted from 1) when the server did not name
/// one: 2 s, then 8 s.
fn backoff(attempt: u32) -> Duration {
    BACKOFF * 4u32.pow(attempt - 1)
}

/// What one attempt concluded.
enum Attempt {
    Done,
    /// Worth another try. `wait` is the delay the server asked for with
    /// `Retry-After`, if it did; `None` means the caller's own ladder decides.
    Again { error: Error, wait: Option<Duration> },
    /// Not worth another try: the URL, the disk or the data is the problem.
    Stop(Error),
}

impl From<std::io::Error> for Attempt {
    fn from(e: std::io::Error) -> Self {
        Attempt::Stop(e.into())
    }
}

/// Download one file, verifying its checksum, counting bytes as they arrive.
/// `bytes_done` is rolled back on a failed attempt, so a retried file does not
/// inflate the transfer readout by its own size.
async fn fetch_once(client: &reqwest::Client, job: &Job, bytes_done: &AtomicU64) -> Attempt {
    if let Some(parent) = job.path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return Attempt::Stop(e.into());
        }
    }

    let response = match client.get(&job.url).send().await {
        Ok(response) => response,
        Err(e) if retryable_error(&e) => {
            return Attempt::Again { error: e.into(), wait: None }
        }
        Err(e) => return Attempt::Stop(e.into()),
    };

    let status = response.status();
    if !status.is_success() {
        let error = Error::msg(format!("HTTP {} for {}", status.as_u16(), job.url));
        if !retryable_status(status) {
            return Attempt::Stop(error);
        }
        let wait = if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            retry_after(response.headers().get("retry-after"))
        } else {
            None
        };
        return Attempt::Again { error, wait };
    }

    // Streamed rather than `.bytes()` so the speed readout updates during a
    // single large file — the client jar alone is 40 MB.
    let mut counted = 0u64;
    let result = stream_to_file(response, job, bytes_done, &mut counted).await;
    if !matches!(result, Attempt::Done) {
        // Give back what this attempt told the UI about, so the retry counts
        // its bytes from zero rather than double-reporting the failed try.
        bytes_done.fetch_sub(counted, Ordering::Relaxed);
    }
    result
}

/// Write the response body to the job's path with its SHA-1 verified. Only
/// disk errors stop here: anything off the network — a cut stream, a checksum
/// mismatch, usually a truncated body the server called complete — is worth
/// one more attempt.
async fn stream_to_file(
    response: reqwest::Response,
    job: &Job,
    bytes_done: &AtomicU64,
    counted: &mut u64,
) -> Attempt {
    let mut stream = response.bytes_stream();
    let mut body = Vec::with_capacity(job.size.unwrap_or(0) as usize);
    let mut hasher = Sha1::new();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(e) => return Attempt::Again { error: e.into(), wait: None },
        };
        hasher.update(&chunk);
        body.extend_from_slice(&chunk);
        *counted += chunk.len() as u64;
        bytes_done.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }

    if let Some(want) = &job.sha1 {
        let got = hex::encode(hasher.finalize());
        if !got.eq_ignore_ascii_case(want) {
            return Attempt::Again {
                error: Error::msg(format!(
                    "checksum mismatch for {}: expected {want}, got {got}",
                    job.url
                )),
                wait: None,
            };
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
    if let Err(e) = tokio::fs::write(&tmp, &body).await {
        return Attempt::Stop(e.into());
    }
    if let Err(e) = tokio::fs::rename(&tmp, &job.path).await {
        return Attempt::Stop(e.into());
    }
    Attempt::Done
}

/// Download one file with up to `ATTEMPTS` tries, waiting in between.
///
/// A home connection blips and CDNs rate-limit, and before this loop one
/// failed request took a 5000-file install down with it. Retrying is per file,
/// so the other fifteen keep flowing while one waits out its backoff.
async fn fetch_one(client: &reqwest::Client, job: &Job, bytes_done: &AtomicU64) -> Result<()> {
    let mut attempt = 1;
    loop {
        match fetch_once(client, job, bytes_done).await {
            Attempt::Done => return Ok(()),
            Attempt::Stop(e) => return Err(e),
            Attempt::Again { wait, .. } if attempt < ATTEMPTS => {
                tokio::time::sleep(wait.unwrap_or_else(|| backoff(attempt))).await;
                attempt += 1;
            }
            Attempt::Again { error, .. } => return Err(error),
        }
    }
}

/// Download every job that is not already present, at most `PARALLEL` at once,
/// reporting progress and transfer rate to the UI as it goes.
pub async fn run(app: &AppHandle, stage: &str, jobs: Vec<Job>) -> Result<()> {
    let client = client()?;

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

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("JustLauncher/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// Send a request with the same retry ladder the file fetcher uses. A manifest
/// or search reply is one request on the critical path of everything, so a
/// single blip there fails a whole step; `RequestBuilder::try_clone` decides
/// whether a body can even be re-sent (ours always can: plain JSON).
async fn send(request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
    // Every attempt but the last goes through a clone; the last one sends the
    // request itself, so a body that cannot be cloned still gets its one shot.
    for attempt in 1..ATTEMPTS {
        let Some(sendable) = request.try_clone() else { break };

        let response = match sendable.send().await {
            Ok(response) => response,
            Err(e) if retryable_error(&e) => {
                tokio::time::sleep(backoff(attempt)).await;
                continue;
            }
            Err(e) => return Err(e.into()),
        };

        let status = response.status();
        // A success is not a retryable status either, so this is the way out.
        if !retryable_status(status) {
            return Ok(response.error_for_status()?);
        }
        let wait = if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            retry_after(response.headers().get("retry-after"))
        } else {
            None
        };
        tokio::time::sleep(wait.unwrap_or_else(|| backoff(attempt))).await;
    }
    Ok(request.send().await?.error_for_status()?)
}

/// Fetch and deserialise JSON, with the launcher's user agent.
pub async fn json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T> {
    Ok(send(client()?.get(url)).await?.json().await?)
}

/// POST a JSON body and decode the JSON reply. Modrinth's bulk update check is
/// the only caller: it takes a list of hashes too long for a query string.
pub async fn post_json<T: serde::de::DeserializeOwned>(
    url: &str,
    body: &serde_json::Value,
) -> Result<T> {
    Ok(send(client()?.post(url).json(body)).await?.json().await?)
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

    #[test]
    fn only_connection_trouble_is_worth_trying_again() {
        use reqwest::StatusCode;
        for retryable in [
            StatusCode::REQUEST_TIMEOUT,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::BAD_GATEWAY,
            StatusCode::SERVICE_UNAVAILABLE,
        ] {
            assert!(retryable_status(retryable), "{retryable}");
        }
        for permanent in [
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
            StatusCode::NOT_FOUND,
            StatusCode::GONE,
        ] {
            assert!(!retryable_status(permanent), "{permanent}");
        }
    }

    #[test]
    fn retry_after_is_read_in_seconds_and_capped() {
        let of = |s: &str| reqwest::header::HeaderValue::from_str(s).unwrap();
        assert_eq!(retry_after(Some(&of("30"))), Some(Duration::from_secs(30)));
        // A server that asks for an hour is refused past the cap; one that
        // sends a date, or nonsense, gets the ordinary backoff ladder instead.
        assert_eq!(retry_after(Some(&of("3600"))), Some(MAX_SERVER_WAIT));
        assert_eq!(retry_after(Some(&of("Wed, 21 Oct 2015 07:28:00 GMT"))), None);
        assert_eq!(retry_after(Some(&of("soon"))), None);
        assert_eq!(retry_after(None), None);
    }

    #[test]
    fn the_backoff_ladder_grows() {
        assert_eq!(backoff(1), Duration::from_secs(2));
        assert_eq!(backoff(2), Duration::from_secs(8));
    }
}
