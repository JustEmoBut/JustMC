import { invoke } from "@tauri-apps/api/core";

export type Loader = "vanilla" | "fabric" | "quilt" | "forge" | "neoforge";

const LOADER_NAMES: Record<Loader, string> = {
  vanilla: "Vanilla",
  fabric: "Fabric",
  quilt: "Quilt",
  forge: "Forge",
  neoforge: "NeoForge",
};

/** Human label for a loader; "Vanilla" is what an unmodded instance is called. */
export function loaderName(loader: Loader) {
  return LOADER_NAMES[loader] ?? "Vanilla";
}

/**
 * Whether the picker's blank entry means "the newest build" rather than "the
 * newest recommended one". Quilt recommends nothing and ships betas as its
 * normal channel; every other loader points at a build.
 */
export function latestLabel(loader: Loader) {
  return loader === "quilt" ? "Latest" : "Latest stable";
}

/** Which instance folder a mods call acts on; the values are the folder names. */
export type ModKind = "mods" | "resourcepacks" | "shaderpacks";

/** Launcher-wide preferences; the first three are defaults for new instances. */
export interface Settings {
  memory_mb: number;
  java_path: string;
  jvm_args: string;
  minimise_on_play: boolean;
  /** Empty means CurseForge is off. */
  curseforge_api_key: string;
}

/** A pack file CurseForge will not let a launcher download. */
export interface MissingFile {
  file_name: string;
  /** The CurseForge page to download it from by hand. */
  url: string;
}

/** An import's reply: the instance, plus what the user still has to fetch. */
export type Imported = Instance & { missing: MissingFile[] };

export interface Instance {
  id: string;
  name: string;
  mc_version: string;
  loader: Loader;
  loader_version: string;
  memory_mb: number;
  java_path: string;
  jvm_args: string;
  /** Window size the game opens at; 0 leaves it to the game's own settings. */
  window_width: number;
  window_height: number;
  /** Shell command run before the game starts; a non-zero exit cancels it. */
  pre_launch: string;
  /** Shell command run after the game exits. */
  post_exit: string;
  last_played: number;
  /** Seconds the game has run in this instance, across every launch. */
  play_time: number;
  /** Whether a session here adds to the play time counters. */
  count_play_time: boolean;
  installed: boolean;
  /** The account this instance launches with; empty means the selected one. */
  account_id: string;
  /** Extra environment for the game and its hooks, one KEY=VALUE per line. */
  env: string;
  /** The Modrinth pack this instance was installed from, when one was recognised. */
  pack: PackSource | null;
}

export interface PackSource {
  project: string;
  version_id: string;
  version_number: string;
  /** Game-folder paths the pack installed; an update may remove these. */
  files: string[];
}

/** A server to join or a save to open on launch; Minecraft calls it Quick Play. */
export type QuickPlay =
  | { kind: "multiplayer"; value: string }
  | { kind: "singleplayer"; value: string };

export interface ManifestVersion {
  id: string;
  /** "release" | "snapshot" | "old_beta" | "old_alpha", straight from Mojang. */
  type: string;
  url: string;
  releaseTime: string;
}

export interface VersionList {
  latest: { release: string; snapshot: string };
  versions: ManifestVersion[];
}

export interface Account {
  id: string;
  name: string;
  kind: "microsoft" | "offline";
  /** The worn skin's texture URL, which the avatar is cut from; empty when unknown. */
  skin_url: string;
}

/** A skin or cape on a Microsoft account, as Mojang's profile API lists it. */
export interface SkinTexture {
  id: string;
  /** "ACTIVE" for the one being worn. */
  state: string;
  url: string;
  /** A skin's arm model, "CLASSIC" or "SLIM"; empty on capes. */
  variant: string;
  /** A cape's name; empty on skins. */
  alias: string;
}

export interface SkinProfile {
  id: string;
  name: string;
  skins: SkinTexture[];
  capes: SkinTexture[];
}

export interface DeviceCode {
  user_code: string;
  device_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
}

export interface JavaInstall {
  path: string;
  version: string;
  major: number;
  /** A 32-bit JVM cannot address more than ~2 GB of heap, whatever -Xmx says. */
  bit64: boolean;
}

export interface FabricLoader {
  version: string;
  stable: boolean;
}

export interface Screenshot {
  file: string;
  /** Absolute path; pass it through convertFileSrc to display it. */
  path: string;
  size: number;
  /** Unix seconds. */
  taken: number;
}

export interface World {
  /** The save directory name; every world command takes this. */
  folder: string;
  /** What level.dat calls it, falling back to the folder name. */
  name: string;
  size: number;
  /** Unix seconds, from level.dat's modification time. */
  last_played: number;
}

/** What a sweep of the shared store would remove, or did. */
export interface StorageReport {
  /** Version directories no instance launches. */
  versions: { name: string; size: number }[];
  /** Downloaded Java runtimes no kept version asks for. */
  runtimes: { name: string; size: number }[];
  /** Asset objects and index files, counted rather than listed. */
  asset_files: number;
  asset_bytes: number;
  total: number;
}

/** A zip in the exports folder that a world backup wrote. */
export interface WorldBackup {
  /** The archive's file name, and the handle restore takes. */
  file: string;
  /** The world folder inside it, which restoring replaces. */
  folder: string;
  size: number;
  /** Unix seconds the backup was taken. */
  made: number;
}

/** One entry of an instance's `servers.dat`. */
export interface Server {
  /** Position in the file, and the handle the remove command takes. */
  index: number;
  name: string;
  /** As stored, port included; this is what gets pinged. */
  ip: string;
  /** The icon the game cached, already a data URI. */
  icon: string | null;
}

/** What a server answered when asked for its status. */
export interface ServerStatus {
  version: string;
  online: number;
  max: number;
  /** The MOTD, flattened to plain text. */
  motd: string;
  favicon: string | null;
  latency_ms: number;
}

/** Which directory a log came from; also how the backend finds it again. */
export type LogSource = "launcher" | "game" | "crash";

export interface LogFile {
  file: string;
  source: LogSource;
  size: number;
  /** Unix seconds. */
  modified: number;
}

export interface ModFile {
  file: string;
  name: string;
  version: string;
  enabled: boolean;
  size: number;
  /** Icon from the jar's fabric.mod.json as a data: URI, when it ships one. */
  icon: string | null;
  /** A pack unpacked into a folder; deleting one removes a whole tree. */
  dir: boolean;
}

/** Which catalogue a project is browsed in, or recognises an installed file. */
export type ModSource = "modrinth" | "curseforge";

/** A CurseForge category; `id` is what the search filter takes. */
export interface ModCategory {
  id: string;
  name: string;
}

export interface ModHit {
  project_id: string;
  slug: string;
  title: string;
  description: string;
  author: string;
  downloads: number;
  follows: number;
  categories: string[];
  icon_url: string | null;
}

export interface ModSearchPage {
  hits: ModHit[];
  total_hits: number;
}

export interface ModProject {
  id: string;
  slug: string;
  title: string;
  description: string;
  /** Modrinth-flavoured markdown, rendered as plain text. */
  body: string;
  downloads: number;
  followers: number;
  categories: string[];
  icon_url: string | null;
  source_url: string | null;
  issues_url: string | null;
  wiki_url: string | null;
  /** Set for a CurseForge project; a Modrinth page is built from the slug. */
  page_url: string | null;
}

export interface ModVersion {
  id: string;
  name: string;
  version_number: string;
  /** "release", "beta" or "alpha". */
  version_type: string;
  date_published: string;
  downloads: number;
}

export interface ModUpdate {
  file: string;
  name: string;
  current_version: string;
  new_version: string;
  version_id: string;
  /** Release notes for the new build, markdown, often absent. */
  changelog: string | null;
}

export const api = {
  listVersions: () => invoke<VersionList>("list_versions"),
  listLoaders: (mcVersion: string, loader: Loader) =>
    invoke<FabricLoader[]>("list_loaders", { mcVersion, loader }),

  listInstances: () => invoke<Instance[]>("list_instances"),
  /** `loaderVersion` null means "resolve the loader build at install time". */
  createInstance: (
    name: string,
    mcVersion: string,
    loader: Loader,
    loaderVersion: string | null
  ) => invoke<Instance>("create_instance", { name, mcVersion, loader, loaderVersion }),
  updateInstance: (instance: Instance) => invoke<void>("update_instance", { instance }),
  deleteInstance: (id: string) => invoke<void>("delete_instance", { id }),
  openInstanceFolder: (id: string) => invoke<void>("open_instance_folder", { id }),
  exportInstance: (id: string) => invoke<string>("export_instance", { id }),
  importInstance: (path: string) => invoke<Imported>("import_instance", { path }),
  /** For a file the webview picked: it hands over content, never a path. */
  importArchiveBytes: (name: string, bytes: Uint8Array) =>
    invoke<Imported>("import_archive_bytes", { name, bytes }),
  duplicateInstance: (id: string, name: string) =>
    invoke<Instance>("duplicate_instance", { id, name }),
  openExportsFolder: () => invoke<void>("open_exports_folder"),
  /** Writes a desktop shortcut that launches the instance; returns its path. */
  createShortcut: (id: string) => invoke<string>("create_shortcut", { id }),
  /** The instance a desktop shortcut started the launcher for, once; null otherwise. */
  startupLaunch: () => invoke<string | null>("startup_launch"),
  installInstance: (id: string) => invoke<void>("install_instance", { id }),
  /** `quickPlay` joins a server or opens a save instead of stopping at the menu. */
  launchInstance: (id: string, accountId: string, quickPlay: QuickPlay | null = null) =>
    invoke<void>("launch_instance", { id, accountId, quickPlay }),

  /**
   * Whether this instance can be told what to join. Asked of the version's own
   * metadata, never of its number: Minecraft's 2026 scheme means "26.2" would
   * read as older than 1.20.
   */
  quickPlaySupported: (id: string) => invoke<boolean>("quick_play_supported", { id }),

  stopInstance: (id: string) => invoke<boolean>("stop_instance", { id }),
  listAccounts: () => invoke<Account[]>("list_accounts"),
  beginMicrosoftLogin: () => invoke<DeviceCode>("begin_microsoft_login"),
  completeMicrosoftLogin: (code: DeviceCode) =>
    invoke<Account>("complete_microsoft_login", { code }),
  addOfflineAccount: (name: string) => invoke<Account>("add_offline_account", { name }),
  removeAccount: (id: string) => invoke<void>("remove_account", { id }),
  /** Skins and capes, read fresh from Mojang; Microsoft accounts only. */
  accountProfile: (accountId: string) => invoke<SkinProfile>("account_profile", { accountId }),
  uploadSkin: (accountId: string, variant: "classic" | "slim", bytes: Uint8Array) =>
    invoke<SkinProfile>("upload_skin", { accountId, variant, bytes }),
  resetSkin: (accountId: string) => invoke<SkinProfile>("reset_skin", { accountId }),
  /** `capeId` null takes the cape off. */
  setCape: (accountId: string, capeId: string | null) =>
    invoke<SkinProfile>("set_cape", { accountId, capeId }),

  listScreenshots: (id: string) => invoke<Screenshot[]>("list_screenshots", { id }),
  deleteScreenshot: (id: string, file: string) =>
    invoke<void>("delete_screenshot", { id, file }),

  listWorlds: (id: string) => invoke<World[]>("list_worlds", { id }),
  /** Zips the world into the exports folder; returns the archive path. */
  backupWorld: (id: string, folder: string) =>
    invoke<string>("backup_world", { id, folder }),
  deleteWorld: (id: string, folder: string) => invoke<void>("delete_world", { id, folder }),
  listWorldBackups: (id: string) => invoke<WorldBackup[]>("list_world_backups", { id }),
  /** Replaces the world the backup came from; returns its folder name. */
  restoreWorld: (id: string, file: string) => invoke<string>("restore_world", { id, file }),

  /** A world's data packs, read like a resource pack folder. */
  listDatapacks: (id: string, folder: string) =>
    invoke<ModFile[]>("list_datapacks", { id, folder }),
  setDatapackEnabled: (id: string, folder: string, file: string, enabled: boolean) =>
    invoke<string>("set_datapack_enabled", { id, folder, file, enabled }),
  deleteDatapack: (id: string, folder: string, file: string) =>
    invoke<void>("delete_datapack", { id, folder, file }),
  /** For a file the webview picked: it hands over content, never a path. */
  addDatapack: (id: string, folder: string, name: string, bytes: Uint8Array) =>
    invoke<void>("add_datapack", { id, folder, name, bytes }),

  /** What the shared store holds that nothing needs. Deletes nothing. */
  scanStorage: () => invoke<StorageReport>("scan_storage"),
  /** Sweeps it, and reports what went. */
  cleanStorage: () => invoke<StorageReport>("clean_storage"),

  /** The game's own options.txt, as text; empty when it has none yet. */
  readOptions: (id: string) => invoke<string>("read_options", { id }),
  writeOptions: (id: string, text: string) => invoke<void>("write_options", { id, text }),

  listServers: (id: string) => invoke<Server[]>("list_servers", { id }),
  addServer: (id: string, name: string, ip: string) =>
    invoke<void>("add_server", { id, name, ip }),
  /** The address goes with the position so a list that moved deletes nothing. */
  removeServer: (id: string, index: number, ip: string) =>
    invoke<void>("remove_server", { id, index, ip }),
  /** Status of one server; one call each, so answers can land as they arrive. */
  pingServer: (address: string) => invoke<ServerStatus>("ping_server", { address }),

  /** Past logs and crash reports of an instance, newest first. */
  listLogs: (id: string) => invoke<LogFile[]>("list_logs", { id }),
  /** The tail of one of them, decompressed when gzipped. */
  readLog: (id: string, source: LogSource, file: string) =>
    invoke<string[]>("read_log", { id, source, file }),

  openModsFolder: (id: string, kind: ModKind) =>
    invoke<void>("open_mods_folder", { id, kind }),
  listMods: (id: string, kind: ModKind) => invoke<ModFile[]>("list_mods", { id, kind }),
  setModEnabled: (id: string, kind: ModKind, file: string, enabled: boolean) =>
    invoke<string>("set_mod_enabled", { id, kind, file, enabled }),
  deleteMod: (id: string, kind: ModKind, file: string) =>
    invoke<void>("delete_mod", { id, kind, file }),
  searchMods: (
    query: string,
    mcVersion: string,
    sort: string,
    category: string | null,
    offset: number,
    kind: ModKind,
    loader: Loader,
    source: ModSource
  ) =>
    invoke<ModSearchPage>("search_mods", {
      query,
      mcVersion,
      sort,
      category,
      offset,
      kind,
      loader,
      source,
    }),
  modProject: (id: string, source: ModSource) =>
    invoke<ModProject>("mod_project", { id, source }),
  modVersions: (
    project: string,
    mcVersion: string,
    kind: ModKind,
    loader: Loader,
    source: ModSource
  ) => invoke<ModVersion[]>("mod_versions", { project, mcVersion, kind, loader, source }),
  /** Installs the file plus, for mods, its required dependencies; returns every file added. */
  installMod: (
    id: string,
    kind: ModKind,
    project: string,
    versionId: string | null,
    source: ModSource
  ) => invoke<string[]>("install_mod", { id, kind, project, versionId, source }),

  /** Search Modrinth's modpack catalogue; a pack brings its own version and loader. */
  searchModpacks: (query: string, sort: string, category: string | null, offset: number) =>
    invoke<ModSearchPage>("search_modpacks", { query, sort, category, offset }),
  /** Every build of a pack, unfiltered by Minecraft version. */
  packVersions: (project: string) => invoke<ModVersion[]>("pack_versions", { project }),
  /** Downloads the pack's `.mrpack` and imports it as a new instance. */
  installModpack: (project: string, versionId: string | null) =>
    invoke<Instance>("install_modpack", { project, versionId }),
  /** The newest build of the instance's pack when it is not the installed one. */
  checkPackUpdate: (id: string) => invoke<ModVersion | null>("check_pack_update", { id }),
  /** Moves the instance to another build of its pack; returns the saved instance. */
  updatePack: (id: string, versionId: string) =>
    invoke<Instance>("update_pack", { id, versionId }),
  /** Exports in Modrinth's `.mrpack` format; returns the archive path. */
  exportMrpack: (id: string) => invoke<string>("export_instance_mrpack", { id }),
  /** Opens an http(s) link in the user's browser; other schemes are refused. */
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  addModFile: (id: string, kind: ModKind, path: string) =>
    invoke<string>("add_mod_file", { id, kind, path }),
  /** Project ids of what is already installed, in `source`'s numbering. */
  installedModProjects: (id: string, kind: ModKind, source: ModSource) =>
    invoke<string[]>("installed_mod_projects", { id, kind, source }),
  /** CurseForge's categories for one folder; Modrinth's are a fixed list. */
  modCategories: (kind: ModKind) => invoke<ModCategory[]>("mod_categories", { kind }),
  /** File name -> the catalogues that recognise it; absent means neither does. */
  modSources: (id: string, kind: ModKind) =>
    invoke<Record<string, ModSource[]>>("mod_sources", { id, kind }),
  checkModUpdates: (id: string, kind: ModKind) =>
    invoke<ModUpdate[]>("check_mod_updates", { id, kind }),
  updateMod: (id: string, kind: ModKind, file: string, versionId: string) =>
    invoke<string>("update_mod", { id, kind, file, versionId }),

  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),

  listJava: () => invoke<JavaInstall[]>("list_java"),
  /** Physical RAM in MB, or null when the platform will not say. */
  systemMemoryMb: () => invoke<number | null>("system_memory_mb"),
};

/** Tauri rejects with the backend's error string; normalise it for display. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
