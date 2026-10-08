// Public so the integration tests in `tests/` can drive version resolution and
// argument building against the real metadata services.
pub mod auth;
pub mod loader;
pub mod install;
pub mod instance;
pub mod jre;
pub mod pack;
pub mod launch;
pub mod mojang;
pub mod mrpack;
pub mod curseforge;

pub mod logs;
pub mod mods;
pub mod modrinth;

pub mod forge;
pub mod nbt;
pub mod screenshots;
pub mod servers;
pub mod settings;
pub mod shortcut;
pub mod skins;
pub mod worlds;
pub mod cleanup;

pub mod download;
mod error;
mod java;
mod paths;

use auth::{Account, DeviceCode};
use error::{Error, Result};
use instance::{Instance, Loader};
use serde::Serialize;

// ------------------------------------------------------------------- versions

#[derive(Serialize)]
struct VersionList {
    latest: mojang::Latest,
    versions: Vec<mojang::ManifestVersion>,
}

#[tauri::command]
async fn list_versions() -> Result<VersionList> {
    let m = mojang::manifest().await?;
    Ok(VersionList { latest: m.latest, versions: m.versions })
}

#[tauri::command]
async fn list_loaders(
    mc_version: String,
    loader: Loader,
) -> Result<Vec<loader::LoaderInfo>> {
    // `builds` and not `loaders`: Forge's list comes from a Maven index rather
    // than a metadata API, and the picker should not have to know that.
    loader::builds(loader, &mc_version).await
}

// ------------------------------------------------------------------ instances

#[tauri::command]
async fn list_instances() -> Vec<Instance> {
    instance::list().await
}

#[tauri::command]
async fn create_instance(
    name: String,
    mc_version: String,
    loader: Loader,
    loader_version: Option<String>,
) -> Result<Instance> {
    instance::create(&name, &mc_version, loader, loader_version.as_deref().unwrap_or_default())
        .await
}

#[tauri::command]
async fn update_instance(instance: Instance) -> Result<()> {
    // Reject a rename of the folder identity: the id is the directory name and
    // moving it here would orphan the on-disk data.
    let existing = instance::get(&instance.id).await?;
    if existing.id != instance.id {
        return Err(Error::msg("Instance id cannot be changed."));
    }
    let mut instance = instance;
    if instance.mc_version != existing.mc_version {
        // Libraries, assets and the client jar are all version-specific, and a
        // Fabric loader build is only listed for the versions it supports, so
        // both have to be resolved again from scratch on the next launch.
        instance.installed = false;
        instance.loader_version = String::new();
    }
    instance.save().await
}

#[tauri::command]
async fn delete_instance(id: String) -> Result<()> {
    instance::delete(&id).await
}

#[tauri::command]
async fn open_instance_folder(app: tauri::AppHandle, id: String) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = instance::get(&id).await?.game_dir();
    tokio::fs::create_dir_all(&dir).await?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| Error::msg(e.to_string()))
}

/// Open the instance's mods directory, creating it if the user has none yet.
#[tauri::command]
async fn open_mods_folder(app: tauri::AppHandle, id: String, kind: mods::Kind) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = instance::get(&id).await?.game_dir().join(kind.folder());
    tokio::fs::create_dir_all(&dir).await?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| Error::msg(e.to_string()))
}

#[tauri::command]
async fn export_instance(id: String) -> Result<String> {
    let inst = instance::get(&id).await?;
    // Zipping a world folder is blocking IO; keep it off the async runtime.
    let path = tokio::task::spawn_blocking(move || pack::export(&inst))
        .await
        .map_err(|e| Error::msg(e.to_string()))??;
    Ok(path.to_string_lossy().into_owned())
}

/// An imported instance, plus the pack files the user still has to download by
/// hand. Flattened so the reply still reads as an `Instance` on the wire.
#[derive(serde::Serialize)]
struct Imported {
    #[serde(flatten)]
    instance: Instance,
    missing: Vec<curseforge::Missing>,
}

#[tauri::command]
async fn import_instance(app: tauri::AppHandle, path: String) -> Result<Imported> {
    import_archive(&app, std::path::Path::new(&path)).await
}

/// A Modrinth pack is a zip too, so the extension is what tells it apart; a
/// CurseForge pack and our own export are both `.zip`, so its manifest does.
async fn import_archive(app: &tauri::AppHandle, path: &std::path::Path) -> Result<Imported> {
    let plain = |instance| Imported { instance, missing: Vec::new() };
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mrpack")) {
        return mrpack::import(app, path).await.map(plain);
    }
    let owned = path.to_path_buf();
    if tokio::task::spawn_blocking(move || curseforge::is_pack(&owned))
        .await
        .map_err(|e| Error::msg(e.to_string()))?
    {
        let (instance, missing) = curseforge::import(app, path).await?;
        return Ok(Imported { instance, missing });
    }
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || pack::import(&path))
        .await
        .map_err(|e| Error::msg(e.to_string()))?
        .map(plain)
}

/// Import an archive the webview's file picker handed over as bytes.
///
/// A `<input type="file">` yields content, never a path, so the bytes are
/// staged in a temp file and the normal importer runs against that.
#[tauri::command]
async fn import_archive_bytes(
    app: tauri::AppHandle,
    name: String,
    bytes: Vec<u8>,
) -> Result<Imported> {
    // The name comes from the file the user picked; it only decides the temp
    // file, but it is still a name being joined onto a path.
    let staged = std::env::temp_dir().join(format!("justlauncher-{}", mods::checked_name(&name)?));
    tokio::fs::write(&staged, bytes).await?;
    let result = import_archive(&app, &staged).await;
    let _ = tokio::fs::remove_file(&staged).await;
    result
}

#[tauri::command]
async fn duplicate_instance(id: String, name: String) -> Result<Instance> {
    let inst = instance::get(&id).await?;
    // Copying a world folder is blocking IO; keep it off the async runtime.
    tokio::task::spawn_blocking(move || pack::duplicate(&inst, &name))
        .await
        .map_err(|e| Error::msg(e.to_string()))?
}

#[tauri::command]
async fn open_exports_folder(app: tauri::AppHandle) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = paths::exports();
    tokio::fs::create_dir_all(&dir).await?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| Error::msg(e.to_string()))
}

/// Put a shortcut on the desktop that starts this instance; returns its path.
#[tauri::command]
async fn create_shortcut(id: String) -> Result<String> {
    let inst = instance::get(&id).await?;
    Ok(shortcut::create(&inst).await?.to_string_lossy().into_owned())
}

/// The instance a desktop shortcut started the launcher for. Taken, not read:
/// the window is rebuilt after a game when it was closed for one, and the
/// rebuilt UI must not launch the same instance a second time.
static STARTUP_LAUNCH: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[tauri::command]
fn startup_launch() -> Option<String> {
    STARTUP_LAUNCH.lock().unwrap().take()
}

// -------------------------------------------------------------------- install

#[tauri::command]
async fn install_instance(app: tauri::AppHandle, id: String) -> Result<()> {
    let mut inst = instance::get(&id).await?;
    install::install(&app, &mut inst).await?;
    Ok(())
}

#[tauri::command]
async fn launch_instance(
    app: tauri::AppHandle,
    id: String,
    account_id: String,
    quick_play: Option<launch::QuickPlay>,
) -> Result<()> {
    let inst = instance::get(&id).await?;
    // The instance's own account wins while it still exists; a removed one
    // falls back to the launcher's selection rather than refusing to start.
    let own = !inst.account_id.is_empty()
        && auth::load_all().await.iter().any(|a| a.id == inst.account_id);
    let account = resolve_account(if own { &inst.account_id } else { &account_id }).await?;
    launch::launch(&app, inst, account, quick_play).await
}

/// Whether this instance's Minecraft can be told what to join on launch.
///
/// Answered from the vanilla version metadata, which is a file read once it has
/// been installed. The loader profile is not resolved: it only ever adds
/// arguments, and resolving Forge's means downloading an installer.
#[tauri::command]
async fn quick_play_supported(id: String) -> Result<bool> {
    let inst = instance::get(&id).await?;
    let version = install::vanilla_version(&inst.mc_version).await?;
    Ok(launch::supports_quick_play(&version))
}

/// Ask the running game to quit. Silently does nothing if it already exited.
#[tauri::command]
fn stop_instance(id: String) -> bool {
    launch::stop(&id)
}

// -------------------------------------------------------------- screenshots

#[tauri::command]
async fn list_screenshots(
    app: tauri::AppHandle,
    id: String,
) -> Result<Vec<screenshots::Screenshot>> {
    screenshots::list(&app, &id).await
}

#[tauri::command]
async fn delete_screenshot(id: String, file: String) -> Result<()> {
    screenshots::delete(&id, &file).await
}

// ------------------------------------------------------------------- worlds

#[tauri::command]
async fn list_worlds(id: String) -> Result<Vec<worlds::World>> {
    worlds::list(&id).await
}

/// Returns the archive path so the UI can say where the backup went.
#[tauri::command]
async fn backup_world(id: String, folder: String) -> Result<String> {
    Ok(worlds::backup(&id, &folder).await?.to_string_lossy().into_owned())
}

#[tauri::command]
async fn list_world_backups(id: String) -> Result<Vec<worlds::Backup>> {
    worlds::backups(&id).await
}

/// Returns the world folder that was restored, for the message.
#[tauri::command]
async fn restore_world(id: String, file: String) -> Result<String> {
    worlds::restore(&id, &file).await
}

#[tauri::command]
async fn list_datapacks(id: String, folder: String) -> Result<Vec<mods::ModFile>> {
    worlds::list_datapacks(&id, &folder).await
}

#[tauri::command]
async fn set_datapack_enabled(id: String, folder: String, file: String, enabled: bool) -> Result<String> {
    worlds::set_datapack_enabled(&id, &folder, &file, enabled).await
}

#[tauri::command]
async fn delete_datapack(id: String, folder: String, file: String) -> Result<()> {
    worlds::delete_datapack(&id, &folder, &file).await
}

#[tauri::command]
async fn add_datapack(id: String, folder: String, name: String, bytes: Vec<u8>) -> Result<()> {
    worlds::add_datapack(&id, &folder, &name, &bytes).await
}

#[tauri::command]
async fn delete_world(id: String, folder: String) -> Result<()> {
    worlds::delete(&id, &folder).await
}

// ------------------------------------------------------------------ storage

/// What the shared store holds that no instance needs. Reports only.
#[tauri::command]
async fn scan_storage() -> Result<cleanup::Report> {
    cleanup::scan().await
}

/// Sweep it, and report what went. The UI confirms first; this does not.
#[tauri::command]
async fn clean_storage() -> Result<cleanup::Report> {
    cleanup::clean().await
}

// ------------------------------------------------------------------ options

/// The game's own settings file. Line-based `key:value`, so it is edited as
/// text -- a form per key would have to know all ~150 of them, and the game
/// adds more every version.
///
/// Missing is empty rather than an error: an instance that has never been
/// launched has no options.txt yet, and writing one is how it gets its first.
#[tauri::command]
async fn read_options(id: String) -> Result<String> {
    let path = instance::get(&id).await?.game_dir().join("options.txt");
    Ok(tokio::fs::read_to_string(path).await.unwrap_or_default())
}

/// The game rewrites this file wholesale when it exits, so the UI refuses
/// while it runs -- exactly as it does for worlds and the server list.
#[tauri::command]
async fn write_options(id: String, text: String) -> Result<()> {
    let dir = instance::get(&id).await?.game_dir();
    tokio::fs::create_dir_all(&dir).await?;
    tokio::fs::write(dir.join("options.txt"), text).await?;
    Ok(())
}

// ------------------------------------------------------------------ servers

#[tauri::command]
async fn list_servers(id: String) -> Result<Vec<servers::Server>> {
    servers::list(&id).await
}

#[tauri::command]
async fn add_server(id: String, name: String, ip: String) -> Result<()> {
    servers::add(&id, &name, &ip).await
}

/// The address is sent back with the position so a list that moved under the
/// user deletes nothing.
#[tauri::command]
async fn remove_server(id: String, index: usize, ip: String) -> Result<()> {
    servers::remove(&id, index, &ip).await
}

/// Ask one server for its status. One command per server, so the UI can show
/// each answer as it lands instead of waiting for the slowest.
#[tauri::command]
async fn ping_server(address: String) -> Result<servers::Status> {
    servers::ping(address).await
}

// ------------------------------------------------------- logs and crash reports

#[tauri::command]
async fn list_logs(id: String) -> Result<Vec<logs::LogFile>> {
    logs::list(&id).await
}

#[tauri::command]
async fn read_log(id: String, source: logs::Source, file: String) -> Result<Vec<String>> {
    logs::read(&id, source, &file).await
}

// ------------------------------------------------- mods, resource and shaders

// `kind` selects the instance folder these act on: mods, resourcepacks or
// shaderpacks. It also carries the Modrinth project type and the loaders a
// build of that kind is tagged with, so one set of commands covers all three.

#[tauri::command]
async fn list_mods(id: String, kind: mods::Kind) -> Result<Vec<mods::ModFile>> {
    mods::list(&id, kind).await
}

#[tauri::command]
async fn set_mod_enabled(
    id: String,
    kind: mods::Kind,
    file: String,
    enabled: bool,
) -> Result<String> {
    mods::set_enabled(&id, kind, &file, enabled).await
}

#[tauri::command]
async fn delete_mod(id: String, kind: mods::Kind, file: String) -> Result<()> {
    mods::delete(&id, kind, &file).await
}

#[tauri::command]
async fn search_mods(
    query: String,
    mc_version: String,
    sort: String,
    category: Option<String>,
    offset: u32,
    kind: mods::Kind,
    loader: Loader,
    source: mods::Source,
) -> Result<modrinth::SearchPage> {
    if source == mods::Source::Curseforge {
        return curseforge::search(&query, &mc_version, &sort, category.as_deref(), offset, kind, loader).await;
    }
    // Only mods narrow by loader; a resource pack or shader has none to filter
    // on, and adding one there returns an empty catalogue.
    let loaders: &[&str] = if kind == mods::Kind::Mods { loader.mod_loaders() } else { &[] };
    modrinth::search(
        &query,
        Some(&mc_version),
        &sort,
        category.as_deref(),
        offset,
        20,
        kind.project_type(),
        loaders,
    )
    .await
}

/// Search Modrinth's modpack catalogue. A pack brings its own Minecraft
/// version and loader, so there is nothing to narrow by here — the choice the
/// user is in the middle of making is the thing being browsed for.
#[tauri::command]
async fn search_modpacks(
    query: String,
    sort: String,
    category: Option<String>,
    offset: u32,
) -> Result<modrinth::SearchPage> {
    modrinth::search(&query, None, &sort, category.as_deref(), offset, 20, "modpack", &[]).await
}

/// Install a Modrinth pack into a fresh instance: fetch the chosen build's
/// `.mrpack` and hand it to the ordinary importer, which owns the whole
/// loaders-and-downloads pipeline.
#[tauri::command]
async fn install_modpack(
    app: tauri::AppHandle,
    project: String,
    version_id: Option<String>,
) -> Result<Instance> {
    let version = match version_id {
        Some(id) => modrinth::version(&id).await?,
        None => {
            // Packs are not narrowed by Minecraft version, so "latest" is
            // simply the newest release of the project, whatever it targets.
            let all = modrinth::all_versions(&project).await?;
            all.iter()
                .find(|v| v.version_type == "release")
                .or_else(|| all.first())
                .cloned()
                .ok_or_else(|| Error::msg("This pack has no releases."))?
        }
    };

    let archive = mrpack::fetch(&app, &version).await?;
    let result = mrpack::import(&app, &archive).await;
    let _ = tokio::fs::remove_file(&archive).await;
    result
}

/// The newest build of the pack this instance came from, if it is not the one
/// installed; `None` for an instance that is not a recognised pack.
#[tauri::command]
async fn check_pack_update(id: String) -> Result<Option<modrinth::Version>> {
    mrpack::newer(&instance::get(&id).await?).await
}

/// Move a pack instance to another build of its pack. Refused while the game
/// runs: it replaces the jars the running game has open.
#[tauri::command]
async fn update_pack(app: tauri::AppHandle, id: String, version_id: String) -> Result<Instance> {
    if launch::is_running(&id) {
        return Err(Error::msg("Stop the game before updating its pack."));
    }
    let mut inst = instance::get(&id).await?;
    mrpack::update(&app, &mut inst, &version_id).await?;
    Ok(inst)
}

/// Export an instance in Modrinth's own pack format, so any launcher that
/// speaks it — including this one — can install it back.
#[tauri::command]
async fn export_instance_mrpack(id: String) -> Result<String> {
    let inst = instance::get(&id).await?;
    let path = mrpack::export(&inst).await?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
async fn mod_project(id: String, source: mods::Source) -> Result<modrinth::Project> {
    match source {
        mods::Source::Modrinth => modrinth::project(&id).await,
        mods::Source::Curseforge => curseforge::project(&id).await,
    }
}

#[tauri::command]
async fn mod_versions(
    project: String,
    mc_version: String,
    kind: mods::Kind,
    loader: Loader,
    source: mods::Source,
) -> Result<Vec<modrinth::Version>> {
    match source {
        mods::Source::Modrinth => modrinth::versions(&project, &mc_version, kind.loaders(loader)).await,
        mods::Source::Curseforge => curseforge::versions(&project, &mc_version, kind, loader).await,
    }
}

/// Every build of a pack, unfiltered: each one names its own Minecraft
/// version, so the browser's version picker must not narrow by one.
#[tauri::command]
async fn pack_versions(project: String) -> Result<Vec<modrinth::Version>> {
    modrinth::all_versions(&project).await
}

/// Install a Modrinth project into an instance, with the required dependencies
/// it declares. `version_id` pins a build; omitting it takes the newest release.
///
/// Returns every file that landed in the folder, so the UI can say what a
/// single click actually installed.
#[tauri::command]
async fn install_mod(
    app: tauri::AppHandle,
    id: String,
    kind: mods::Kind,
    project: String,
    version_id: Option<String>,
    source: mods::Source,
) -> Result<Vec<String>> {
    let instance = instance::get(&id).await?;
    // Both catalogues answer in Modrinth's shapes, so one loop serves both;
    // only where a version comes from differs.
    let latest = |project: String| {
        let instance = instance.clone();
        async move {
            match source {
                mods::Source::Modrinth => {
                    modrinth::latest_version(&project, &instance.mc_version, kind.loaders(instance.loader)).await
                }
                mods::Source::Curseforge => {
                    curseforge::latest_version(&project, &instance.mc_version, kind, instance.loader).await
                }
            }
        }
    };
    let version = match (version_id, source) {
        (Some(v), mods::Source::Modrinth) => modrinth::version(&v).await?,
        (Some(v), mods::Source::Curseforge) => curseforge::file(&project, &v).await?,
        (None, _) => latest(project.clone()).await?,
    };

    let mut installed = Vec::new();
    let mut queue = vec![version];
    let mut seen = std::collections::HashSet::new();

    // Breadth-first over required dependencies. Modrinth graphs are shallow
    // (a mod needs Fabric API, which needs nothing), and `seen` stops a cycle
    // or a diamond from fetching the same jar twice.
    while let Some(version) = queue.pop() {
        if !seen.insert(version.id.clone()) {
            continue;
        }
        if let Some(jar) = version.jar() {
            // CurseForge leaves the URL empty when the author forbids launchers
            // from downloading the file: the page is the only way to get it.
            if jar.url.is_empty() {
                let page = curseforge::page(&version.project_id).await?;
                let _ = open_url(app.clone(), page).await;
                return Err(Error::msg(format!(
                    "{}'s author only allows downloading it from CurseForge's site; its page was opened. Put the file in the {} folder.",
                    jar.filename,
                    kind.folder()
                )));
            }
            mods::remove_other_versions(&id, kind, source, &version.project_id, &jar.filename).await?;
            mods::fetch(
                &app,
                &id,
                kind,
                &jar.url,
                &jar.filename,
                jar.hashes.sha1.clone(),
                Some(jar.size),
            )
            .await?;
            installed.push(jar.filename.clone());
        }
        if !kind.installs_dependencies() {
            continue;
        }
        for dep in version.dependencies.iter().filter(|d| d.dependency_type == "required") {
            let resolved = match (&dep.version_id, &dep.project_id) {
                (Some(v), _) => modrinth::version(v).await,
                (None, Some(p)) => latest(p.clone()).await,
                (None, None) => continue,
            };
            // A dependency with no build for this Minecraft version must not
            // sink the whole install: the mod itself is already downloaded and
            // the user can be told which piece is missing.
            match resolved {
                Ok(version) => queue.push(version),
                Err(e) => return Err(Error::msg(format!("Dependency could not be installed: {e}"))),
            }
        }
    }
    Ok(installed)
}

/// Open an http(s) link in the user's browser. Anything else is refused: this
/// is reachable from Modrinth-supplied URLs, and `open_path` on an arbitrary
/// string would launch whatever the OS associates with it.
#[tauri::command]
async fn open_url(app: tauri::AppHandle, url: String) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(Error::msg("Only web links can be opened."));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| Error::msg(e.to_string()))
}

#[tauri::command]
async fn add_mod_file(id: String, kind: mods::Kind, path: String) -> Result<String> {
    mods::add_file(&id, kind, std::path::Path::new(&path)).await
}

/// Project ids of what is already installed, in `source`'s numbering, for
/// marking search results as installed.
#[tauri::command]
async fn installed_mod_projects(id: String, kind: mods::Kind, source: mods::Source) -> Result<Vec<String>> {
    mods::installed_projects(&id, kind, source).await
}

/// CurseForge's categories for one folder. Modrinth's are a fixed list the
/// frontend carries itself.
#[tauri::command]
async fn mod_categories(kind: mods::Kind) -> Result<Vec<curseforge::Category>> {
    curseforge::categories(kind).await
}

/// Which catalogues recognise each installed file, keyed by file name.
#[tauri::command]
async fn mod_sources(
    id: String,
    kind: mods::Kind,
) -> Result<std::collections::HashMap<String, Vec<mods::Source>>> {
    mods::sources(&id, kind).await
}

#[tauri::command]
async fn check_mod_updates(id: String, kind: mods::Kind) -> Result<Vec<mods::ModUpdate>> {
    mods::check_updates(&id, kind).await
}

#[tauri::command]
async fn update_mod(
    app: tauri::AppHandle,
    id: String,
    kind: mods::Kind,
    file: String,
    version_id: String,
) -> Result<String> {
    mods::update_to(&app, &id, kind, &file, &version_id).await
}

// -------------------------------------------------------------------- updates

#[derive(Serialize)]
struct LauncherUpdate {
    version: String,
    notes: Option<String>,
}

/// A newer release on GitHub, or `None`. Releases are verified against the
/// public key in `tauri.conf.json`; it is public, so it lives in the config,
/// and the bundler refuses to build updater artifacts without it.
#[tauri::command]
async fn check_launcher_update(app: tauri::AppHandle) -> Result<Option<LauncherUpdate>> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| Error::msg(e.to_string()))?;
    let found = updater.check().await.map_err(|e| Error::msg(e.to_string()))?;
    Ok(found.map(|u| LauncherUpdate { version: u.version, notes: u.body }))
}

/// Download, verify and install the newest release, then restart into it.
/// Refused while a game runs: the restart would orphan its supervisor, and
/// with it the play time and the post-exit command.
#[tauri::command]
async fn install_launcher_update(app: tauri::AppHandle) -> Result<()> {
    use tauri_plugin_updater::UpdaterExt;
    if launch::any_running() {
        return Err(Error::msg("Close the game before updating the launcher."));
    }
    let updater = app.updater().map_err(|e| Error::msg(e.to_string()))?;
    let update = updater
        .check()
        .await
        .map_err(|e| Error::msg(e.to_string()))?
        .ok_or_else(|| Error::msg("There is no newer release."))?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| Error::msg(e.to_string()))?;
    app.restart()
}

// ------------------------------------------------------------------- settings

#[tauri::command]
async fn get_settings() -> settings::Settings {
    settings::load().await
}

#[tauri::command]
async fn save_settings(settings: settings::Settings) -> Result<()> {
    settings::save(&settings).await
}

// ------------------------------------------------------------------- accounts

#[tauri::command]
async fn list_accounts() -> Vec<Account> {
    auth::load_all().await
}

#[tauri::command]
async fn begin_microsoft_login() -> Result<DeviceCode> {
    auth::start_device_code().await
}

/// Blocks until the user completes the browser sign-in, then stores the account.
#[tauri::command]
async fn complete_microsoft_login(code: DeviceCode) -> Result<Account> {
    let account = auth::poll_device_code(code).await?;
    store_account(account.clone()).await?;
    Ok(account)
}

#[tauri::command]
async fn add_offline_account(name: String) -> Result<Account> {
    let account = auth::offline_account(&name)?;
    store_account(account.clone()).await?;
    Ok(account)
}

#[tauri::command]
async fn remove_account(id: String) -> Result<()> {
    // The keychain entry goes with the account; a removed login should not
    // outlive it in the OS store.
    auth::forget(&id).await;
    let mut accounts = auth::load_all().await;
    accounts.retain(|a| a.id != id);
    auth::save_all(&accounts).await
}

/// The account's skins and capes, read fresh from Mojang.
#[tauri::command]
async fn account_profile(account_id: String) -> Result<skins::Profile> {
    let account = resolve_account(&account_id).await?;
    let profile = skins::profile(&account).await?;
    remember_skin(account, profile).await
}

#[tauri::command]
async fn upload_skin(account_id: String, variant: skins::Variant, bytes: Vec<u8>) -> Result<skins::Profile> {
    let account = resolve_account(&account_id).await?;
    let profile = skins::upload(&account, variant, &bytes).await?;
    remember_skin(account, profile).await
}

#[tauri::command]
async fn reset_skin(account_id: String) -> Result<skins::Profile> {
    let account = resolve_account(&account_id).await?;
    let profile = skins::reset(&account).await?;
    remember_skin(account, profile).await
}

/// `cape_id` null takes the cape off.
#[tauri::command]
async fn set_cape(account_id: String, cape_id: Option<String>) -> Result<skins::Profile> {
    let account = resolve_account(&account_id).await?;
    let profile = skins::set_cape(&account, cape_id.as_deref()).await?;
    remember_skin(account, profile).await
}

/// Keep the stored avatar source in step with what Mojang just said is worn.
async fn remember_skin(mut account: Account, profile: skins::Profile) -> Result<skins::Profile> {
    let url = profile.skin_url();
    if account.skin_url != url {
        account.skin_url = url;
        store_account(account).await?;
    }
    Ok(profile)
}

async fn store_account(account: Account) -> Result<()> {
    let mut accounts = auth::load_all().await;
    // Signing in again with the same account replaces the stale tokens rather
    // than adding a duplicate row.
    accounts.retain(|a| a.id != account.id);
    accounts.push(account);
    auth::save_all(&accounts).await
}

/// Look up a stored account, refreshing its Microsoft token if it has expired.
async fn resolve_account(id: &str) -> Result<Account> {
    let accounts = auth::load_all().await;
    let account = accounts
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| Error::msg("Selected account no longer exists."))?;

    if !account.is_expired() {
        return Ok(account.clone());
    }
    let refreshed = auth::refresh(account).await?;
    store_account(refreshed.clone()).await?;
    Ok(refreshed)
}

// ----------------------------------------------------------------------- java

/// Physical RAM in MB, so the memory slider cannot promise more than exists.
#[tauri::command]
fn system_memory_mb() -> Option<u32> {
    java::physical_memory_mb()
}

#[tauri::command]
fn list_java() -> Vec<java::JavaInstall> {
    java::discover()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    *STARTUP_LAUNCH.lock().unwrap() = shortcut::launch_arg(std::env::args().skip(1));
    tauri::Builder::default()
        // First, as the plugin requires. A second start -- a shortcut clicked
        // while the launcher is open -- must not become a second launcher: two
        // of them would each think an instance is free and run it twice over
        // one `.minecraft`.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            use tauri::{Emitter, Manager};
            let request = shortcut::launch_arg(args.into_iter().skip(1));
            match app.get_webview_window("main") {
                Some(window) => {
                    if let Err(e) = window.unminimize().and_then(|_| window.set_focus()) {
                        eprintln!("Could not bring the launcher forward: {e}");
                    }
                    if let Some(id) = request {
                        let _ = app.emit("launch-request", id);
                    }
                }
                // The window is closed for a running game; the rebuilt UI
                // asks `startup_launch` once that game ends.
                None => {
                    if request.is_some() {
                        *STARTUP_LAUNCH.lock().unwrap() = request;
                    }
                }
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            list_versions,
            list_loaders,
            list_instances,
            create_instance,
            update_instance,
            delete_instance,
            create_shortcut,
            startup_launch,
            export_instance,
            import_instance,
            duplicate_instance,
            import_archive_bytes,
            open_exports_folder,
            open_instance_folder,
            open_mods_folder,
            install_instance,
            launch_instance,
            quick_play_supported,
            stop_instance,
            list_screenshots,
            delete_screenshot,
            list_servers,
            add_server,
            remove_server,
            ping_server,
            list_worlds,
            backup_world,
            list_world_backups,
            restore_world,
            delete_world,
            list_datapacks,
            set_datapack_enabled,
            delete_datapack,
            add_datapack,
            read_options,
            write_options,
            scan_storage,
            clean_storage,
            list_logs,
            read_log,
            list_mods,
            set_mod_enabled,
            delete_mod,
            search_mods,
            search_modpacks,
            install_modpack,
            export_instance_mrpack,
            check_pack_update,
            update_pack,
            pack_versions,
            mod_project,
            mod_versions,
            install_mod,
            add_mod_file,
            open_url,
            installed_mod_projects,
            mod_sources,
            mod_categories,
            check_mod_updates,
            update_mod,
            list_accounts,
            begin_microsoft_login,
            complete_microsoft_login,
            add_offline_account,
            remove_account,
            account_profile,
            upload_skin,
            reset_skin,
            set_cape,
            list_java,
            system_memory_mb,
            get_settings,
            check_launcher_update,
            install_launcher_update,
            save_settings,
        ])
        .build(tauri::generate_context!())
        .expect("error while running JustLauncher")
        .run(|_, event| {
            // `code: None` is the last window going away. While that was the
            // launcher stepping aside for a game, the game is still supervised
            // from here and the window comes back when it ends.
            if let tauri::RunEvent::ExitRequested { code: None, api, .. } = event {
                if launch::window_closed_for_game() {
                    api.prevent_exit();
                }
            }
        });
}
