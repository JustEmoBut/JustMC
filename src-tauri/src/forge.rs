//! Forge and NeoForge.
//!
//! Fabric and Quilt publish a ready-made profile and installing one is a
//! download. Forge does not: it ships an **installer**, and the client jar it
//! wants does not exist anywhere until this machine builds it. That is the
//! whole reason this module is not `loader.rs`.
//!
//! What an installer holds:
//!
//! - `version.json` — an ordinary `inheritsFrom` profile, so once it exists
//!   `mojang::merge` stacks it exactly like Fabric's.
//! - `install_profile.json` — the build recipe: extra libraries, a `data` map
//!   of named inputs and outputs, and a list of `processors`, each a jar to run
//!   with a JVM. Roughly: download Mojang's mappings, rename the client jar
//!   into them, then apply a binary patch. The last processor's output is the
//!   patched client.
//! - `maven/` — artifacts published nowhere else (Forge's `universal` and
//!   `shim`), which have to be unpacked into the library store.
//!
//! Both projects use the same format (`spec` 1). Only the modern one is
//! supported: Forge before 1.13 used a different installer entirely, and is
//! refused with a message rather than failing halfway through.
//!
//! Three details that are easy to get wrong, each verified against a real
//! installer (Forge 1.21.1-52.1.16, NeoForge 21.1.248):
//!
//! - **A library with an empty `url` is an output, not a download.** Forge's
//!   `forge:<version>:client` is the patched jar: on the classpath, listed with
//!   a hash and a size, and produced locally. `Library::resolve` skips the job
//!   for it rather than fetching an empty URL.
//! - **NeoForge does not put its patched jar on the classpath at all.** FML
//!   finds it under `libraryDirectory` from the `--fml.*` arguments, so the
//!   only proof it was built is that the file exists.
//! - **Client and server share one recipe.** Processors carry a `sides` list
//!   and half of them are server-only (`BUNDLER_EXTRACT`, the run scripts);
//!   running those on a client install is wasted work at best.

use crate::download::{self, Job};
use crate::error::{Error, Result};
use crate::instance::Loader;
use crate::loader::LoaderInfo;
use crate::mojang::{self, VersionJson};
use crate::paths;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const NEO_VERSIONS: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";
const NEO_MAVEN: &str = "https://maven.neoforged.net/releases";
const FORGE_MAVEN: &str = "https://maven.minecraftforge.net";
const FORGE_METADATA: &str =
    "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";
const FORGE_PROMOS: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";

/// The name of the file inside every installer.
const PROFILE_NAME: &str = "install_profile.json";

// ------------------------------------------------------------------ versions

/// The NeoForge build numbers for a Minecraft version. There is no per-version
/// index to ask: the version is encoded in the build number, and Minecraft
/// changed how it numbers itself in 2026.
///
/// Under the old `1.x` scheme, `1.21.1` is served by `21.1.<build>` and `1.21`
/// by `21.0.<build>` — the leading `1.` is dropped and a missing patch is
/// zero. Under the new one the whole version is kept and padded to three
/// parts: `26.2` is served by `26.2.0.<build>`, `26.1.2` by `26.1.2.<build>`.
/// Both were read off real installers' `minecraft` field.
fn neo_prefix(mc: &str) -> Option<String> {
    // The old scheme's `1.` is a constant, not a major version, so what is
    // left is two parts; the new scheme keeps all three.
    let (rest, wanted) = match mc.strip_prefix("1.") {
        Some(rest) => (rest, 2),
        None => (mc, 3),
    };
    let mut parts: Vec<&str> = rest.split('.').collect();
    // A snapshot ("26.3-snapshot-9", "24w14a") is not a version NeoForge
    // publishes for, and neither is anything with more parts than the scheme.
    if parts.len() > wanted || parts.iter().any(|p| !p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    if parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    while parts.len() < wanted {
        parts.push("0");
    }
    Some(format!("{}.", parts.join(".")))
}

/// Compare two dotted build numbers numerically, so `21.1.9` sorts below
/// `21.1.10` — a string compare puts it above.
fn build_order(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |v: &str| -> Vec<u64> {
        v.split(['.', '-'])
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    parts(a).cmp(&parts(b))
}

/// Every `<version>` in a Maven metadata document. Written by hand because the
/// file has exactly one tag worth reading and the alternative is an XML parser.
fn maven_versions(xml: &str) -> Vec<&str> {
    xml.split("<version>")
        .skip(1)
        .filter_map(|rest| rest.split_once("</version>").map(|(v, _)| v.trim()))
        .collect()
}

/// The builds available for a Minecraft version, newest first, with the one to
/// recommend flagged.
///
/// Empty is an answer, not a failure: it is how the UI says a version has no
/// Forge yet, the same as Fabric's 400 and Quilt's 404.
pub async fn versions(loader: Loader, mc: &str) -> Result<Vec<LoaderInfo>> {
    let mut out: Vec<LoaderInfo> = match loader {
        Loader::NeoForge => {
            #[derive(Deserialize)]
            struct NeoVersions {
                versions: Vec<String>,
            }
            let Some(prefix) = neo_prefix(mc) else { return Ok(Vec::new()) };
            let all: NeoVersions = download::json(NEO_VERSIONS).await?;
            all.versions
                .into_iter()
                .filter(|v| v.starts_with(&prefix))
                // NeoForge marks nothing recommended; the newest build that is
                // not a beta stands in, and `-beta` is the flag it does set.
                .map(|version| LoaderInfo { stable: !version.contains("-beta"), version })
                .collect()
        }
        _ => {
            // Forge publishes no per-version index either; the Maven metadata
            // is the list, and `promotions_slim.json` is the only place it
            // says which build it recommends.
            let xml = download::text(FORGE_METADATA).await?;
            let wanted = format!("{mc}-");
            let recommended: Option<String> = {
                #[derive(Deserialize)]
                struct Promos {
                    promos: HashMap<String, String>,
                }
                let promos: Option<Promos> = download::json(FORGE_PROMOS).await.ok();
                promos.and_then(|p| p.promos.get(&format!("{mc}-recommended")).cloned())
            };
            maven_versions(&xml)
                .into_iter()
                .filter_map(|v| v.strip_prefix(&wanted))
                .map(|v| LoaderInfo {
                    stable: recommended.as_deref() == Some(v),
                    version: v.to_string(),
                })
                .collect()
        }
    };

    out.sort_by(|a, b| build_order(&b.version, &a.version));

    // Exactly one build is marked recommended, as with Fabric: the one the
    // project itself points at, or the newest when it names none.
    let recommended = out.iter().position(|l| l.stable).unwrap_or(0);
    for (i, entry) in out.iter_mut().enumerate() {
        entry.stable = i == recommended;
    }
    Ok(out)
}

/// The newest recommended build, for an instance created without a pin.
pub async fn latest(loader: Loader, mc: &str) -> Result<String> {
    let list = versions(loader, mc).await?;
    list.iter()
        .find(|l| l.stable)
        .or_else(|| list.first())
        .map(|l| l.version.clone())
        .ok_or_else(|| {
            Error::msg(format!("{} has no build for Minecraft {mc}.", loader.label()))
        })
}

// ------------------------------------------------------------------ profiles

/// The version id an installed profile carries, which is also the folder it
/// lives in. Both projects name it themselves and the format differs; these
/// are the names their own installers write.
pub fn profile_id(loader: Loader, mc: &str, version: &str) -> String {
    match loader {
        Loader::NeoForge => format!("neoforge-{version}"),
        _ => format!("{mc}-forge-{version}"),
    }
}

fn installer_url(loader: Loader, mc: &str, version: &str) -> String {
    match loader {
        Loader::NeoForge => format!(
            "{NEO_MAVEN}/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar"
        ),
        _ => format!(
            "{FORGE_MAVEN}/net/minecraftforge/forge/{mc}-{version}/forge-{mc}-{version}-installer.jar"
        ),
    }
}

/// Where an installed profile keeps its files: the profile JSON the launcher
/// reads, and the installer itself, which stays because the binary patch and
/// Forge's unpublished artifacts live inside it.
fn profile_dir(id: &str) -> PathBuf {
    paths::versions().join(id)
}

fn installer_path(id: &str) -> PathBuf {
    profile_dir(id).join("installer.jar")
}

/// Fetch the installer and unpack the two JSON files out of it, once.
///
/// This is the half that needs no JVM, so it can run wherever a version is
/// resolved; `patch` is the half that builds the jar.
pub async fn profile(loader: Loader, mc: &str, version: &str) -> Result<VersionJson> {
    let id = profile_id(loader, mc, version);
    let dir = profile_dir(&id);
    let json_path = dir.join(format!("{id}.json"));

    if let Ok(text) = tokio::fs::read_to_string(&json_path).await {
        if let Ok(parsed) = serde_json::from_str(&text) {
            return Ok(parsed);
        }
    }

    let installer = installer_path(&id);
    if !installer.is_file() {
        download::fetch(&installer_url(loader, mc, version), &installer).await?;
    }

    let dir_for_task = dir.clone();
    let installer_for_task = installer.clone();
    let read = tokio::task::spawn_blocking(move || read_installer(&installer_for_task, &dir_for_task))
        .await
        .map_err(|e| Error::msg(e.to_string()))?;
    let (profile_text, version_text) = match read {
        Ok(texts) => texts,
        Err(e) => {
            // An unreadable installer (one truncated by an older build, say)
            // would otherwise be trusted by the `is_file` check forever.
            let _ = tokio::fs::remove_file(&installer).await;
            return Err(e);
        }
    };

    tokio::fs::write(dir.join(PROFILE_NAME), &profile_text).await?;
    tokio::fs::write(&json_path, &version_text).await?;
    Ok(serde_json::from_str(&version_text)?)
}

/// Pull `install_profile.json` and the version profile it points at out of an
/// installer jar, refusing anything that is not the modern format.
fn read_installer(installer: &Path, _dir: &Path) -> Result<(String, String)> {
    let file = std::fs::File::open(installer)
        .map_err(|e| Error::msg(format!("Cannot open the installer: {e}")))?;
    let mut zip = zip::ZipArchive::new(file)?;

    let mut profile_text = String::new();
    {
        use std::io::Read;
        let mut entry = zip.by_name(PROFILE_NAME).map_err(|_| {
            Error::msg("That installer has no install_profile.json — it is too old for this launcher.")
        })?;
        entry.read_to_string(&mut profile_text)?;
    }

    let profile: InstallProfile = serde_json::from_str(&profile_text)?;
    if profile.spec != 1 {
        return Err(Error::msg(format!(
            "This build uses installer format {} — only Forge 1.13 and newer, and every NeoForge, are supported.",
            profile.spec
        )));
    }

    let mut version_text = String::new();
    {
        use std::io::Read;
        let name = profile.json.trim_start_matches('/');
        let mut entry = zip
            .by_name(name)
            .map_err(|_| Error::msg(format!("The installer names {name} but does not contain it.")))?;
        entry.read_to_string(&mut version_text)?;
    }
    Ok((profile_text, version_text))
}

// ------------------------------------------------------------------ patching

#[derive(Deserialize)]
struct InstallProfile {
    #[serde(default)]
    spec: u32,
    /// The profile id, e.g. `1.21.1-forge-52.1.16`.
    #[allow(dead_code)]
    version: String,
    /// Path inside the jar to the version profile, e.g. `/version.json`.
    json: String,
    /// A Maven coordinate the installer carries but no repository serves.
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    libraries: Vec<mojang::Library>,
    #[serde(default)]
    processors: Vec<Processor>,
    /// Named inputs and outputs, each with a client and a server value.
    #[serde(default)]
    data: HashMap<String, Sided>,
}

#[derive(Deserialize)]
struct Sided {
    client: String,
}

#[derive(Deserialize)]
struct Processor {
    /// Maven coordinate of the jar to run.
    jar: String,
    #[serde(default)]
    classpath: Vec<String>,
    #[serde(default)]
    args: Vec<String>,
    /// Which install this step belongs to. Absent means both.
    #[serde(default)]
    sides: Option<Vec<String>>,
}

impl Processor {
    fn runs_on_client(&self) -> bool {
        match &self.sides {
            Some(sides) => sides.iter().any(|s| s == "client"),
            None => true,
        }
    }
}

/// A `[group:artifact:version:classifier@ext]` reference to a file in the
/// library store.
fn library_path(coordinate: &str) -> Result<PathBuf> {
    Ok(paths::libraries().join(mojang::maven_path(coordinate)?))
}

/// Resolve one `data` entry to the string a processor argument wants.
///
/// Three shapes, and getting them confused silently produces a broken jar:
/// `[maven]` is a file in the library store, `/data/x.lzma` is a path inside
/// the installer -- named from its root, so `unpacked` is where the installer
/// was unpacked to, not its `data` subdirectory -- and `'quoted'` is a
/// literal.
fn resolve_data(value: &str, unpacked: &Path) -> Result<String> {
    if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        return Ok(library_path(inner)?.to_string_lossy().into_owned());
    }
    if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        return Ok(inner.to_string());
    }
    if let Some(name) = value.strip_prefix('/') {
        return Ok(unpacked.join(name).to_string_lossy().into_owned());
    }
    Ok(value.to_string())
}

/// Substitute one processor argument.
fn resolve_arg(arg: &str, data: &HashMap<String, String>) -> Result<String> {
    if let Some(key) = arg.strip_prefix('{').and_then(|a| a.strip_suffix('}')) {
        return data
            .get(key)
            .cloned()
            .ok_or_else(|| Error::msg(format!("The installer asked for {{{key}}}, which it never defined.")));
    }
    if let Some(inner) = arg.strip_prefix('[').and_then(|a| a.strip_suffix(']')) {
        return Ok(library_path(inner)?.to_string_lossy().into_owned());
    }
    if let Some(inner) = arg.strip_prefix('\'').and_then(|a| a.strip_suffix('\'')) {
        return Ok(inner.to_string());
    }
    Ok(arg.to_string())
}

/// The `Main-Class` a processor jar declares. Without it there is nothing to
/// run: these jars are tools, not libraries.
fn main_class(jar: &Path) -> Result<String> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(jar)?)?;
    let mut manifest = String::new();
    zip.by_name("META-INF/MANIFEST.MF")
        .map_err(|_| Error::msg(format!("{} has no manifest.", jar.display())))?
        .read_to_string(&mut manifest)?;
    manifest
        .lines()
        .find_map(|line| line.strip_prefix("Main-Class:"))
        .map(|c| c.trim().to_string())
        .ok_or_else(|| Error::msg(format!("{} declares no Main-Class.", jar.display())))
}

/// Unpack the installer's own `maven/` tree into the library store, and the
/// `data/` files the processors read. Both are things no repository serves.
fn unpack_installer(installer: &Path, data_dir: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(installer)?)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        // The archive is downloaded, so its names are untrusted: `enclosed_name`
        // is what stops one writing outside the directory it is aimed at.
        let Some(name) = entry.enclosed_name() else { continue };
        if entry.is_dir() {
            continue;
        }
        let text = name.to_string_lossy().replace('\\', "/");
        let out = if let Some(rest) = text.strip_prefix("maven/") {
            paths::libraries().join(rest)
        } else if let Some(rest) = text.strip_prefix("data/") {
            data_dir.join(rest)
        } else {
            continue;
        };
        if out.exists() {
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::io::copy(&mut entry, &mut std::fs::File::create(&out)?)?;
    }
    Ok(())
}

/// Read the recipe back off disk. `profile` put it there.
async fn install_profile(id: &str) -> Result<InstallProfile> {
    let text = tokio::fs::read_to_string(profile_dir(id).join(PROFILE_NAME)).await?;
    Ok(serde_json::from_str(&text)?)
}

/// Where the patched client jar lands, if this installer builds one.
fn patched_jar(profile: &InstallProfile) -> Result<Option<PathBuf>> {
    match profile.data.get("PATCHED") {
        Some(sided) => Ok(Some(library_path(sided.client.trim_matches(['[', ']']))?)),
        // Not a shape either project ships today; an installer with no patch
        // step has nothing to build.
        None => Ok(None),
    }
}

/// The tool jars `patch` will run, as download jobs, plus the unpacking of
/// what only the installer has. Empty when the patched jar is already built.
///
/// Split from `patch` so the download goes through the caller's progress
/// window: this module never sees an `AppHandle`, which also keeps it usable
/// from a test binary — one that so much as names that type will not start.
pub async fn tool_jobs(loader: Loader, mc: &str, version: &str) -> Result<Vec<Job>> {
    let id = profile_id(loader, mc, version);
    let profile = install_profile(&id).await?;
    let Some(patched) = patched_jar(&profile)? else { return Ok(Vec::new()) };
    if patched.is_file() {
        return Ok(Vec::new());
    }

    let installer = installer_path(&id);
    let data_dir = profile_dir(&id).join("data");
    tokio::task::spawn_blocking(move || unpack_installer(&installer, &data_dir))
        .await
        .map_err(|e| Error::msg(e.to_string()))??;

    // The tools are libraries like any other and go through the same verified
    // downloader as the rest of the install.
    let mut jobs = Vec::new();
    for lib in &profile.libraries {
        if let Some(resolved) = lib.resolve()? {
            if let Some(job) = resolved.job {
                jobs.push(job);
            }
        }
    }
    Ok(jobs)
}

/// Build the patched client jar by running the installer's processors.
///
/// Idempotent like the rest of the install path: the whole point of `PATCHED`
/// is that its absence is the only reason to run a series of JVMs.
pub async fn patch(loader: Loader, mc: &str, version: &str, java: &Path) -> Result<()> {
    let id = profile_id(loader, mc, version);
    let dir = profile_dir(&id);
    let profile = install_profile(&id).await?;

    let Some(patched) = patched_jar(&profile)? else { return Ok(()) };
    if patched.is_file() {
        return Ok(());
    }

    let installer = installer_path(&id);

    // The installer's own artifact, when it names one that no repository has.
    if let Some(coordinate) = &profile.path {
        let target = library_path(coordinate)?;
        if !target.is_file() {
            return Err(Error::msg(format!(
                "The installer promised {} and did not deliver it.",
                target.display()
            )));
        }
    }

    let mut data = HashMap::new();
    for (key, value) in &profile.data {
        data.insert(key.clone(), resolve_data(&value.client, &dir)?);
    }
    data.insert("SIDE".to_string(), "client".to_string());
    data.insert(
        "MINECRAFT_JAR".to_string(),
        crate::install::client_jar(mc).to_string_lossy().into_owned(),
    );
    data.insert("INSTALLER".to_string(), installer.to_string_lossy().into_owned());
    // A client install has no server root; the steps that use it are
    // server-only and never run here.
    data.insert("ROOT".to_string(), dir.to_string_lossy().into_owned());

    let separator = if cfg!(windows) { ";" } else { ":" };
    for processor in profile.processors.iter().filter(|p| p.runs_on_client()) {
        let jar = library_path(&processor.jar)?;
        let mut classpath = vec![jar.to_string_lossy().into_owned()];
        for entry in &processor.classpath {
            classpath.push(library_path(entry)?.to_string_lossy().into_owned());
        }
        let main = main_class(&jar)?;
        let mut args = vec![
            "-cp".to_string(),
            classpath.join(separator),
            main,
        ];
        for arg in &processor.args {
            args.push(resolve_arg(arg, &data)?);
        }

        let mut command = tokio::process::Command::new(java);
        command.args(&args);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let output = command.output().await?;
        if !output.status.success() {
            // The tool's own message is the only useful part; the exit code
            // alone tells a user nothing.
            let tail: String = String::from_utf8_lossy(&output.stdout)
                .lines()
                .chain(String::from_utf8_lossy(&output.stderr).lines())
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .join("\n");
            return Err(Error::msg(format!(
                "{} install step failed ({}):\n{tail}",
                loader.label(),
                processor.jar
            )));
        }
    }

    if !patched.is_file() {
        return Err(Error::msg(format!(
            "The {} install ran but produced no patched client jar.",
            loader.label()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neoforge_build_numbers_follow_the_minecraft_version() {
        // The old `1.x` scheme: the leading 1 is dropped.
        assert_eq!(neo_prefix("1.21.1").as_deref(), Some("21.1."));
        assert_eq!(neo_prefix("1.21").as_deref(), Some("21.0."));
        assert_eq!(neo_prefix("1.20.4").as_deref(), Some("20.4."));
        // The 2026 scheme: the whole version, padded to three parts. Taken
        // from the installers themselves -- neoforge-26.2.0.64 says its
        // Minecraft is "26.2".
        assert_eq!(neo_prefix("26.2").as_deref(), Some("26.2.0."));
        assert_eq!(neo_prefix("26.1.2").as_deref(), Some("26.1.2."));
        assert_eq!(neo_prefix("26.1").as_deref(), Some("26.1.0."));
        // Snapshots and anything unparseable have no NeoForge at all.
        assert_eq!(neo_prefix("24w14a"), None);
        assert_eq!(neo_prefix("26.3-snapshot-9"), None);
        assert_eq!(neo_prefix("1.21.1-pre1"), None);
        assert_eq!(neo_prefix("1.21.1.2"), None, "too many parts for the old scheme");
    }

    #[test]
    fn builds_sort_numerically_not_alphabetically() {
        let mut list = ["21.1.9", "21.1.10", "21.1.100", "21.1.2"];
        list.sort_by(|a, b| build_order(b, a));
        assert_eq!(list, ["21.1.100", "21.1.10", "21.1.9", "21.1.2"]);
    }

    #[test]
    fn maven_metadata_yields_its_versions() {
        let xml = "<metadata><versioning><versions>\
            <version>1.21.1-52.1.0</version>\n<version>1.21.1-52.1.16</version>\
            </versions></versioning></metadata>";
        assert_eq!(maven_versions(xml), ["1.21.1-52.1.0", "1.21.1-52.1.16"]);
        assert!(maven_versions("<metadata/>").is_empty());
    }

    #[test]
    fn profile_ids_match_what_the_installers_write() {
        // Both taken from real installers rather than derived.
        assert_eq!(profile_id(Loader::Forge, "1.21.1", "52.1.16"), "1.21.1-forge-52.1.16");
        assert_eq!(profile_id(Loader::NeoForge, "1.21.1", "21.1.248"), "neoforge-21.1.248");
    }

    #[test]
    fn installer_urls_follow_each_project_layout() {
        assert_eq!(
            installer_url(Loader::Forge, "1.21.1", "52.1.16"),
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.21.1-52.1.16/forge-1.21.1-52.1.16-installer.jar"
        );
        assert_eq!(
            installer_url(Loader::NeoForge, "1.21.1", "21.1.248"),
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.248/neoforge-21.1.248-installer.jar"
        );
    }

    #[test]
    fn data_entries_resolve_by_their_shape() {
        let data = Path::new("/tmp/data");
        let maven = resolve_data("[net.minecraft:client:1.21.1:slim]", data).unwrap();
        assert!(maven.ends_with("client-1.21.1-slim.jar"), "{maven}");
        assert!(maven.starts_with(&*paths::libraries().to_string_lossy()));

        // A quoted value is a literal, and an installer path points inside the
        // extracted data directory.
        assert_eq!(resolve_data("'1.21.1-20240808'", data).unwrap(), "1.21.1-20240808");
        // Named from the installer's root, which is why the "data" segment is
        // part of the value rather than part of the directory.
        let lzma = resolve_data("/data/client.lzma", Path::new("/unpacked")).unwrap();
        let lzma = lzma.replace('\\', "/");
        assert!(lzma.ends_with("/unpacked/data/client.lzma"), "{lzma}");
    }

    #[test]
    fn arguments_take_their_value_from_the_data_map() {
        let mut data = HashMap::new();
        data.insert("PATCHED".to_string(), "/out/patched.jar".to_string());
        assert_eq!(resolve_arg("{PATCHED}", &data).unwrap(), "/out/patched.jar");
        assert_eq!(resolve_arg("--apply", &data).unwrap(), "--apply");
        assert_eq!(resolve_arg("'literal'", &data).unwrap(), "literal");
        // A key the installer never defined is a broken installer, not a
        // placeholder to pass through: the tool would write the wrong file.
        assert!(resolve_arg("{NOPE}", &data).is_err());
    }

    #[test]
    fn only_client_steps_run_here() {
        let client = |sides: Option<Vec<&str>>| Processor {
            jar: "a:b:1".into(),
            classpath: vec![],
            args: vec![],
            sides: sides.map(|s| s.into_iter().map(String::from).collect()),
        };
        assert!(client(None).runs_on_client(), "no sides means both");
        assert!(client(Some(vec!["client"])).runs_on_client());
        assert!(client(Some(vec!["client", "server"])).runs_on_client());
        assert!(!client(Some(vec!["server"])).runs_on_client());
    }
}
