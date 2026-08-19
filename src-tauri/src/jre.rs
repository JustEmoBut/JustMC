//! Downloading a Java runtime from Mojang.
//!
//! Without this a user with no JDK installed simply cannot play. Mojang ships
//! the exact runtimes the game is tested against, and every version JSON names
//! the one it wants in `javaVersion.component`, so there is no guessing and no
//! third-party JDK vendor involved.

use crate::download::{self, Job};
use crate::error::{Error, Result};
use crate::mojang::Artifact;
use crate::paths;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

const ALL_RUNTIMES_URL: &str =
    "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

/// Mojang's platform keys. `gamecore` is the Bedrock/console entry and is never
/// a candidate here.
fn platform() -> Result<&'static str> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("windows", "aarch64") => "windows-arm64",
        ("windows", "x86") => "windows-x86",
        ("linux", "x86_64") => "linux",
        ("linux", "x86") => "linux-i386",
        ("macos", "aarch64") => "mac-os-arm64",
        ("macos", _) => "mac-os",
        (os, arch) => {
            return Err(Error::msg(format!(
                "Mojang does not publish a Java runtime for {os}/{arch}. \
                 Install a JDK yourself and select it in the instance settings."
            )))
        }
    })
}

/// Fallback when a version JSON names no component, which is the case for
/// everything before 1.17.
fn component_for_major(major: u32) -> &'static str {
    match major {
        0..=8 => "jre-legacy",
        9..=16 => "java-runtime-alpha",
        17..=20 => "java-runtime-gamma",
        21..=24 => "java-runtime-delta",
        _ => "java-runtime-epsilon",
    }
}

#[derive(Deserialize)]
struct RuntimeEntry {
    manifest: ManifestRef,
}

#[derive(Deserialize)]
struct ManifestRef {
    url: String,
}

#[derive(Deserialize)]
struct FileManifest {
    files: HashMap<String, ManifestFile>,
}

#[derive(Deserialize)]
struct ManifestFile {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    downloads: HashMap<String, Artifact>,
    #[serde(default)]
    executable: bool,
    /// Symlink destination, relative to the link's own directory.
    #[serde(default)]
    target: Option<String>,
}

/// Where a given runtime lives once installed. Keyed by platform as well as
/// component so a synced or copied data directory cannot hand macOS binaries
/// to a Windows machine.
fn install_dir(component: &str) -> Result<PathBuf> {
    Ok(paths::java_runtimes().join(platform()?).join(component))
}

/// The java binary inside an installed runtime. macOS nests everything in a
/// bundle; the other platforms put it directly in `bin/`.
fn java_binary(dir: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        dir.join("jre.bundle/Contents/Home/bin/java")
    } else if cfg!(windows) {
        dir.join("bin/javaw.exe")
    } else {
        dir.join("bin/java")
    }
}

/// Runtime components Mojang publishes for this platform. Exposed so a test can
/// assert the ones the launcher depends on are still there.
pub async fn available_components() -> Result<Vec<String>> {
    let all: HashMap<String, HashMap<String, Vec<RuntimeEntry>>> =
        download::json(ALL_RUNTIMES_URL).await?;
    let mut names: Vec<String> = all
        .get(platform()?)
        .ok_or_else(|| Error::msg("Mojang published no runtimes for this platform."))?
        .iter()
        .filter(|(_, entries)| !entries.is_empty())
        .map(|(name, _)| name.clone())
        .collect();
    names.sort();
    Ok(names)
}

/// Everything needed to install one runtime, resolved but not yet fetched.
/// Split out from `ensure` so the manifest handling can be tested without a
/// running Tauri app.
pub struct Plan {
    pub component: String,
    pub dir: PathBuf,
    pub jobs: Vec<Job>,
    links: Vec<(PathBuf, String)>,
    executables: Vec<PathBuf>,
}

impl Plan {
    /// The java binary this plan will produce.
    pub fn binary(&self) -> PathBuf {
        java_binary(&self.dir)
    }
}

/// Resolve a component to a concrete list of files to download.
///
/// `component` is the name from the version JSON; when it is missing or the
/// platform does not publish it, we fall back to the component that normally
/// carries that major version.
pub async fn plan(component: Option<&str>, major: u32) -> Result<Plan> {
    let fallback = component_for_major(major);
    let wanted = component.filter(|c| !c.is_empty()).unwrap_or(fallback);

    let all: HashMap<String, HashMap<String, Vec<RuntimeEntry>>> =
        download::json(ALL_RUNTIMES_URL).await?;
    let platform_runtimes = all
        .get(platform()?)
        .ok_or_else(|| Error::msg("Mojang published no runtimes for this platform."))?;

    let (component, entry) = [wanted, fallback]
        .iter()
        .find_map(|name| {
            platform_runtimes
                .get(*name)
                .and_then(|entries| entries.first())
                .map(|entry| (*name, entry))
        })
        .ok_or_else(|| {
            Error::msg(format!(
                "Mojang publishes no Java {major} runtime for this platform.                  Install a JDK yourself and select it in the instance settings."
            ))
        })?;

    let dir = install_dir(component)?;
    let manifest: FileManifest = download::json(&entry.manifest.url).await?;

    let mut jobs = Vec::new();
    let mut links = Vec::new();
    let mut executables = Vec::new();

    for (name, file) in &manifest.files {
        // Reject absolute paths and traversal before joining anything from a
        // downloaded manifest onto a local directory.
        let relative = Path::new(name);
        if relative.is_absolute() || relative.components().any(|c| c.as_os_str() == "..") {
            return Err(Error::msg(format!("unsafe path in Java manifest: {name}")));
        }
        let path = dir.join(relative);

        match file.kind.as_str() {
            "file" => {
                let artifact = file
                    .downloads
                    .get("raw")
                    .ok_or_else(|| Error::msg(format!("no raw download for {name}")))?;
                jobs.push(Job {
                    url: artifact.url.clone(),
                    path: path.clone(),
                    sha1: artifact.sha1.clone(),
                    size: artifact.size,
                });
                if file.executable {
                    executables.push(path);
                }
            }
            "link" => {
                if let Some(target) = &file.target {
                    links.push((path, target.clone()));
                }
            }
            // Directories are created implicitly by the files inside them.
            _ => {}
        }
    }

    Ok(Plan { component: component.to_string(), dir, jobs, links, executables })
}

/// Ensure a runtime for this version is installed, returning the java binary.
pub async fn ensure(app: &AppHandle, component: Option<&str>, major: u32) -> Result<String> {
    let fallback = component_for_major(major);
    let wanted = component.filter(|c| !c.is_empty()).unwrap_or(fallback);

    // Already installed: skip the two metadata requests entirely, so a normal
    // launch costs nothing.
    for candidate in [wanted, fallback] {
        let bin = java_binary(&install_dir(candidate)?);
        if bin.exists() {
            return Ok(bin.to_string_lossy().into_owned());
        }
    }

    let plan = plan(component, major).await?;
    download::run(app, "Java runtime", plan.jobs.clone()).await?;
    finish_install(&plan.links, &plan.executables)?;

    let bin = plan.binary();
    if !bin.exists() {
        return Err(Error::msg(format!(
            "Java runtime {} installed but {} is missing.",
            plan.component,
            bin.display()
        )));
    }
    Ok(bin.to_string_lossy().into_owned())
}

/// Recreate symlinks and restore the executable bit. Both are Unix-only: the
/// Windows manifests contain neither.
#[cfg(unix)]
fn finish_install(links: &[(PathBuf, String)], executables: &[PathBuf]) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    for (path, target) in links {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Replace rather than fail: a half-finished previous run may have left
        // the link behind.
        let _ = std::fs::remove_file(path);
        std::os::unix::fs::symlink(target, path)?;
    }
    for path in executables {
        // A JRE whose `java` is not +x fails at launch with a bare permission
        // error that gives the user nothing to work with.
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(perms.mode() | 0o111);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn finish_install(_links: &[(PathBuf, String)], _executables: &[PathBuf]) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn majors_map_to_the_runtime_that_carries_them() {
        assert_eq!(component_for_major(8), "jre-legacy");
        assert_eq!(component_for_major(17), "java-runtime-gamma");
        assert_eq!(component_for_major(21), "java-runtime-delta");
        assert_eq!(component_for_major(25), "java-runtime-epsilon");
    }

    #[test]
    fn binary_lives_under_the_install_dir() {
        let dir = Path::new("root");
        assert!(java_binary(dir).starts_with(dir));
        assert!(java_binary(dir).to_string_lossy().contains("java"));
    }
}
