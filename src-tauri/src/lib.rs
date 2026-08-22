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

pub mod logs;
pub mod mods;
pub mod modrinth;

pub mod screenshots;
pub mod settings;
pub mod worlds;

mod download;
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
    loader::loaders(loader, &mc_version).await
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

#[tauri::command]
async fn import_instance(app: tauri::AppHandle, path: String) -> Result<Instance> {
    import_archive(&app, std::path::Path::new(&path)).await
}

/// A Modrinth pack is a zip too, so the extension is what tells the two formats
/// apart before either parser is handed the file.
async fn import_archive(app: &tauri::AppHandle, path: &std::path::Path) -> Result<Instance> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mrpack")) {
        return mrpack::import(app, path).await;
    }
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || pack::import(&path))
        .await
        .map_err(|e| Error::msg(e.to_string()))?
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
) -> Result<Instance> {
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

// -------------------------------------------------------------------- install

#[tauri::command]
async fn install_instance(app: tauri::AppHandle, id: String) -> Result<()> {
    let mut inst = instance::get(&id).await?;
    install::install(&app, &mut inst).await?;
    Ok(())
}

#[tauri::command]
async fn launch_instance(app: tauri::AppHandle, id: String, account_id: String) -> Result<()> {
    let inst = instance::get(&id).await?;
    let account = resolve_account(&account_id).await?;
    launch::launch(&app, inst, account).await
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
async fn delete_world(id: String, folder: String) -> Result<()> {
    worlds::delete(&id, &folder).await
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
) -> Result<modrinth::SearchPage> {
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

    let file = version.jar().ok_or_else(|| Error::msg("That pack build has no file."))?;
    let archive = std::env::temp_dir().join(format!("justlauncher-{}.mrpack", version.id));
    download::run(
        &app,
        "Pack",
        vec![download::Job {
            url: file.url.clone(),
            path: archive.clone(),
            sha1: file.hashes.sha1.clone(),
            size: Some(file.size),
        }],
    )
    .await?;
    let result = mrpack::import(&app, &archive).await;
    let _ = tokio::fs::remove_file(&archive).await;
    result
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
async fn mod_project(id: String) -> Result<modrinth::Project> {
    modrinth::project(&id).await
}

#[tauri::command]
async fn mod_versions(
    project: String,
    mc_version: String,
    kind: mods::Kind,
    loader: Loader,
) -> Result<Vec<modrinth::Version>> {
    modrinth::versions(&project, &mc_version, kind.loaders(loader)).await
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
) -> Result<Vec<String>> {
    let instance = instance::get(&id).await?;
    let version = match version_id {
        Some(v) => modrinth::version(&v).await?,
        None => {
            modrinth::latest_version(&project, &instance.mc_version, kind.loaders(instance.loader))
                .await?
        }
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
            mods::remove_other_versions(&id, kind, &version.project_id, &jar.filename).await?;
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
                (None, Some(p)) => {
                    modrinth::latest_version(p, &instance.mc_version, kind.loaders(instance.loader))
                        .await
                }
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

/// Modrinth project ids of what is already installed, for marking search
/// results as installed.
#[tauri::command]
async fn installed_mod_projects(id: String, kind: mods::Kind) -> Result<Vec<String>> {
    mods::installed_projects(&id, kind).await
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
    let mut accounts = auth::load_all().await;
    accounts.retain(|a| a.id != id);
    auth::save_all(&accounts).await
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
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_versions,
            list_loaders,
            list_instances,
            create_instance,
            update_instance,
            delete_instance,
            export_instance,
            import_instance,
            duplicate_instance,
            import_archive_bytes,
            open_exports_folder,
            open_instance_folder,
            open_mods_folder,
            install_instance,
            launch_instance,
            stop_instance,
            list_screenshots,
            delete_screenshot,
            list_worlds,
            backup_world,
            delete_world,
            list_logs,
            read_log,
            list_mods,
            set_mod_enabled,
            delete_mod,
            search_mods,
            search_modpacks,
            install_modpack,
            export_instance_mrpack,
            pack_versions,
            mod_project,
            mod_versions,
            install_mod,
            add_mod_file,
            open_url,
            installed_mod_projects,
            check_mod_updates,
            update_mod,
            list_accounts,
            begin_microsoft_login,
            complete_microsoft_login,
            add_offline_account,
            remove_account,
            list_java,
            system_memory_mb,
            get_settings,
            save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running JustLauncher");
}
